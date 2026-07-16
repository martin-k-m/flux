//! Dependency inventory across the supported manifests.
//!
//! This is analysis, not resolution: we count and name *declared* direct
//! dependencies by reading the manifest. We deliberately don't hit the network
//! to check for newer versions (Flux stays offline and `windows-sys`-free), so
//! "outdated" is never faked — [`crate::intel::health`] only reasons about the
//! count and presence of a lockfile.

use std::path::Path;

/// A repository's declared direct dependencies.
#[derive(Debug, Clone, Default)]
pub struct Dependencies {
    /// Number of declared direct dependencies.
    pub total: usize,
    /// The dependency names (sorted).
    pub names: Vec<String>,
    /// Whether a lockfile pins exact versions.
    pub locked: bool,
    /// The manifest we read, for display (e.g. `Cargo.toml`).
    pub source: Option<String>,
}

/// Inventory dependencies for `root`, using `language` to pick the manifest.
pub fn analyze(root: &Path, language: Option<&str>) -> Dependencies {
    match language {
        Some("rust") => cargo(root),
        Some("node") => package_json(root),
        Some("python") => requirements(root),
        Some("go") => go_mod(root),
        _ => detect_any(root),
    }
}

/// When the language is unknown, try each manifest in turn.
fn detect_any(root: &Path) -> Dependencies {
    for probe in [cargo, package_json, requirements, go_mod] {
        let d = probe(root);
        if d.source.is_some() {
            return d;
        }
    }
    Dependencies::default()
}

fn cargo(root: &Path) -> Dependencies {
    let path = root.join("Cargo.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Dependencies::default();
    };
    let mut names = Vec::new();
    let mut in_deps = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            // `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`.
            in_deps = t.contains("dependencies]");
            continue;
        }
        if in_deps && !t.is_empty() && !t.starts_with('#') {
            if let Some((name, _)) = t.split_once('=') {
                let name = name.trim().trim_matches('"');
                if !name.is_empty() {
                    names.push(name.to_string());
                }
            }
        }
    }
    finish(names, root.join("Cargo.lock").is_file(), "Cargo.toml")
}

fn package_json(root: &Path) -> Dependencies {
    let path = root.join("package.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Dependencies::default();
    };
    let mut names = Vec::new();
    // Minimal object scan: collect keys inside `dependencies`/`devDependencies`.
    let mut in_deps = false;
    for line in text.lines() {
        let t = line.trim();
        if t.contains("\"dependencies\"") || t.contains("\"devDependencies\"") {
            in_deps = true;
            continue;
        }
        if in_deps {
            if t.starts_with('}') {
                in_deps = false;
                continue;
            }
            if let Some(rest) = t.strip_prefix('"') {
                if let Some((name, _)) = rest.split_once('"') {
                    names.push(name.to_string());
                }
            }
        }
    }
    let locked = root.join("package-lock.json").is_file() || root.join("yarn.lock").is_file();
    finish(names, locked, "package.json")
}

fn requirements(root: &Path) -> Dependencies {
    let path = root.join("requirements.txt");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Dependencies::default();
    };
    let mut names = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with('-') {
            continue;
        }
        let name = t
            .split(['=', '<', '>', '~', '!', ' ', '['])
            .next()
            .unwrap_or("")
            .trim();
        if !name.is_empty() {
            names.push(name.to_string());
        }
    }
    finish(names, false, "requirements.txt")
}

fn go_mod(root: &Path) -> Dependencies {
    let path = root.join("go.mod");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Dependencies::default();
    };
    let mut names = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("require (") {
            in_block = true;
            continue;
        }
        if in_block {
            if t.starts_with(')') {
                in_block = false;
                continue;
            }
            if let Some(name) = t.split_whitespace().next() {
                names.push(name.to_string());
            }
        } else if let Some(rest) = t.strip_prefix("require ") {
            if let Some(name) = rest.split_whitespace().next() {
                names.push(name.to_string());
            }
        }
    }
    finish(names, root.join("go.sum").is_file(), "go.mod")
}

fn finish(mut names: Vec<String>, locked: bool, source: &str) -> Dependencies {
    names.sort();
    names.dedup();
    Dependencies {
        total: names.len(),
        names,
        locked,
        source: Some(source.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("flux-deps-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_cargo_dependencies() {
        let dir = tmp("cargo");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\n\n[dependencies]\nclap = \"4\"\nsha2 = \"0.10\"\n\n[dev-dependencies]\ntempfile = \"3\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("Cargo.lock"), "").unwrap();
        let d = analyze(&dir, Some("rust"));
        assert_eq!(d.total, 3);
        assert!(d.names.contains(&"clap".to_string()));
        assert!(d.locked);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_requirements() {
        let dir = tmp("py");
        std::fs::write(
            dir.join("requirements.txt"),
            "# comment\nflask==2.0\nrequests>=2\n\n-e .\n",
        )
        .unwrap();
        let d = analyze(&dir, Some("python"));
        assert_eq!(d.names, vec!["flask".to_string(), "requests".to_string()]);
        assert!(!d.locked);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
