//! Flux Artifact Registry (2.4).
//!
//! A local, filesystem-backed registry for build outputs. Artifacts are stored
//! under `.flux-cache/artifacts/<name>/<version>/<platform>/`, and releases
//! bundle a version's artifacts into a listing of downloads.
//!
//! ```text
//! my-api
//!  └── v1.0.0
//!       ├── linux-x64
//!       ├── windows-x64
//!       └── docker-image
//! ```

use std::io;
use std::path::{Path, PathBuf};

/// The host platform label, e.g. `windows-x64`.
pub fn host_platform() -> String {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

/// A request to push an artifact.
pub struct PushSpec {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub source: PathBuf,
}

/// A stored artifact.
#[derive(Debug, Clone)]
pub struct Artifact {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub bytes: u64,
}

/// The artifact registry rooted at a project.
pub struct Registry {
    root: PathBuf,
}

impl Registry {
    pub fn new(project_root: &Path) -> Self {
        Registry {
            root: project_root.join(".flux-cache").join("artifacts"),
        }
    }

    /// Copy `spec.source` into the registry and record it.
    pub fn push(&self, spec: &PushSpec) -> io::Result<Artifact> {
        let dest_dir = self
            .root
            .join(&spec.name)
            .join(&spec.version)
            .join(&spec.platform);
        std::fs::create_dir_all(&dest_dir)?;

        let bytes = if spec.source.is_dir() {
            copy_dir(&spec.source, &dest_dir)?
        } else if spec.source.is_file() {
            let file_name = spec.source.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "source has no filename")
            })?;
            let dest = dest_dir.join(file_name);
            std::fs::copy(&spec.source, &dest)?
        } else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("artifact source not found: {}", spec.source.display()),
            ));
        };

        // Record a small manifest alongside the files.
        let manifest = format!(
            "name = {}\nversion = {}\nplatform = {}\nbytes = {}\n",
            spec.name, spec.version, spec.platform, bytes
        );
        std::fs::write(dest_dir.join(".artifact"), manifest)?;

        Ok(Artifact {
            name: spec.name.clone(),
            version: spec.version.clone(),
            platform: spec.platform.clone(),
            bytes,
        })
    }

    /// List every stored artifact.
    pub fn list(&self) -> io::Result<Vec<Artifact>> {
        let mut out = Vec::new();
        if !self.root.exists() {
            return Ok(out);
        }
        for name_entry in read_dirs(&self.root)? {
            let name = dir_name(&name_entry);
            if name == "releases" {
                continue;
            }
            for ver_entry in read_dirs(&name_entry)? {
                let version = dir_name(&ver_entry);
                for plat_entry in read_dirs(&ver_entry)? {
                    let platform = dir_name(&plat_entry);
                    let bytes = dir_size(&plat_entry);
                    out.push(Artifact {
                        name: name.clone(),
                        version: version.clone(),
                        platform,
                        bytes,
                    });
                }
            }
        }
        out.sort_by(|a, b| {
            (a.name.as_str(), a.version.as_str(), a.platform.as_str()).cmp(&(
                b.name.as_str(),
                b.version.as_str(),
                b.platform.as_str(),
            ))
        });
        Ok(out)
    }

    /// Create a release bundling the artifacts for `version` (all of them when
    /// none match that exact version). Returns the download entries.
    pub fn create_release(&self, project: &str, version: &str) -> io::Result<Vec<Artifact>> {
        let all = self.list()?;
        let mut downloads: Vec<Artifact> = all
            .iter()
            .filter(|a| a.version == version)
            .cloned()
            .collect();
        if downloads.is_empty() {
            downloads = all;
        }

        let release_dir = self.root.join("releases").join(version);
        std::fs::create_dir_all(&release_dir)?;

        let mut manifest = format!("project = {project}\nrelease = {version}\n\ndownloads:\n");
        for a in &downloads {
            manifest.push_str(&format!(
                "  - {}/{} ({} bytes) [{}]\n",
                a.name, a.platform, a.bytes, a.version
            ));
        }
        std::fs::write(release_dir.join("release.txt"), manifest)?;
        Ok(downloads)
    }
}

fn read_dirs(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            out.push(entry.path());
        }
    }
    Ok(out)
}

fn dir_name(p: &Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string()
}

/// Recursively copy `src` into `dst`; returns total bytes copied.
fn copy_dir(src: &Path, dst: &Path) -> io::Result<u64> {
    let mut total = 0;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            total += copy_dir(&path, &target)?;
        } else if entry.file_type()?.is_file() {
            total += std::fs::copy(&path, &target)?;
        }
    }
    Ok(total)
}

/// Total size of files directly and recursively under `dir`.
fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                // Don't count the internal manifest marker.
                if path.file_name().and_then(|n| n.to_str()) == Some(".artifact") {
                    continue;
                }
                total += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            } else if path.is_dir() {
                total += dir_size(&path);
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("flux-artifact-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn push_and_list() {
        let root = temp_dir("push");
        std::fs::write(root.join("bin"), b"hello-binary").unwrap();
        let reg = Registry::new(&root);
        reg.push(&PushSpec {
            name: "my-api".into(),
            version: "v1.0.0".into(),
            platform: "linux-x64".into(),
            source: root.join("bin"),
        })
        .unwrap();

        let list = reg.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "my-api");
        assert_eq!(list[0].platform, "linux-x64");
        assert_eq!(list[0].bytes, 12);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn release_lists_downloads() {
        let root = temp_dir("release");
        std::fs::write(root.join("bin"), b"x").unwrap();
        let reg = Registry::new(&root);
        for plat in ["linux-x64", "windows-x64"] {
            reg.push(&PushSpec {
                name: "myapp".into(),
                version: "v1.0".into(),
                platform: plat.into(),
                source: root.join("bin"),
            })
            .unwrap();
        }
        let downloads = reg.create_release("myapp", "v1.0").unwrap();
        assert_eq!(downloads.len(), 2);
        assert!(root
            .join(".flux-cache/artifacts/releases/v1.0/release.txt")
            .exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
