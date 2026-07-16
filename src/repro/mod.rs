//! Build Reproducibility System (3.6).
//!
//! "It worked on my machine" happens because environments drift. Flux captures
//! the environment into a `.flux.lock` file: toolchain versions, the container
//! image, and a hash of the sources. `flux reproduce` compares the current
//! environment against the lock and reports any drift, so a build can be
//! recreated (or the difference explained) anywhere.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cache::Cache;
use crate::core::config::FluxConfig;

/// The conventional lock filename (sits next to `.flux`).
pub const LOCK_FILE: &str = ".flux.lock";

/// A captured, reproducible environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lock {
    pub language: Option<String>,
    /// Tool → version line, e.g. `rustc` → `1.97.0`.
    pub tools: Vec<(String, String)>,
    pub environment_image: Option<String>,
    pub source_hash: String,
}

impl Lock {
    /// Capture the current environment for `config` under `root`.
    pub fn capture(root: &Path, config: &FluxConfig) -> Lock {
        let language = config.language.clone();
        let tools = capture_tools(language.as_deref());
        let environment_image = config.environment.as_ref().and_then(|e| e.image.clone());
        let source_hash = Cache::new(root).source_hash();
        Lock {
            language,
            tools,
            environment_image,
            source_hash,
        }
    }

    /// Serialize to the lock-file text format.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        if let Some(lang) = &self.language {
            out.push_str(&format!("language = {lang}\n"));
        }
        for (tool, version) in &self.tools {
            out.push_str(&format!("tool.{tool} = {version}\n"));
        }
        if let Some(img) = &self.environment_image {
            out.push_str(&format!("environment_image = {img}\n"));
        }
        out.push_str(&format!("source_hash = {}\n", self.source_hash));
        out
    }

    /// Parse a lock file's text.
    pub fn from_text(text: &str) -> Lock {
        let mut lock = Lock::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "language" => lock.language = Some(v.to_string()),
                "environment_image" => lock.environment_image = Some(v.to_string()),
                "source_hash" => lock.source_hash = v.to_string(),
                _ if k.starts_with("tool.") => {
                    lock.tools.push((k[5..].to_string(), v.to_string()));
                }
                _ => {}
            }
        }
        lock
    }

    /// Compare `self` (the lock) against `current`, returning human-readable
    /// drift descriptions (empty when identical).
    pub fn diff(&self, current: &Lock) -> Vec<String> {
        let mut drift = Vec::new();
        for (tool, locked_ver) in &self.tools {
            match current.tools.iter().find(|(t, _)| t == tool) {
                Some((_, cur_ver)) if cur_ver != locked_ver => {
                    drift.push(format!("{tool}: locked {locked_ver}, current {cur_ver}"))
                }
                None => drift.push(format!("{tool}: locked {locked_ver}, now missing")),
                _ => {}
            }
        }
        if self.environment_image != current.environment_image {
            drift.push(format!(
                "environment image: locked {:?}, current {:?}",
                self.environment_image, current.environment_image
            ));
        }
        if self.source_hash != current.source_hash {
            drift.push("sources have changed since the lock was written".to_string());
        }
        drift
    }
}

/// Write a lock file to `root/.flux.lock`.
pub fn write(root: &Path, lock: &Lock) -> io::Result<PathBuf> {
    let path = root.join(LOCK_FILE);
    std::fs::write(&path, lock.to_text())?;
    Ok(path)
}

/// Read a lock file if present.
pub fn read(root: &Path) -> io::Result<Option<Lock>> {
    let path = root.join(LOCK_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(Lock::from_text(&text))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Capture the versions of the tools relevant to `language`.
fn capture_tools(language: Option<&str>) -> Vec<(String, String)> {
    let tools: &[&str] = match language {
        Some("rust") => &["rustc", "cargo"],
        Some("node") => &["node", "npm"],
        Some("python") => &["python", "pip"],
        Some("go") => &["go"],
        _ => &[],
    };
    tools
        .iter()
        .filter_map(|t| tool_version(t).map(|v| (t.to_string(), v)))
        .collect()
}

/// Best-effort version string from `<tool> --version` (first line, trimmed).
fn tool_version(tool: &str) -> Option<String> {
    let out = Command::new(tool).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next().unwrap_or("").trim();
    // Extract the version-looking token if present, else keep the whole line.
    let version = line
        .split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .unwrap_or(line);
    Some(version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_roundtrips_through_text() {
        let lock = Lock {
            language: Some("rust".into()),
            tools: vec![
                ("rustc".into(), "1.97.0".into()),
                ("cargo".into(), "1.97.0".into()),
            ],
            environment_image: Some("rust:latest".into()),
            source_hash: "abc123".into(),
        };
        let parsed = Lock::from_text(&lock.to_text());
        assert_eq!(lock, parsed);
    }

    #[test]
    fn diff_detects_version_and_source_drift() {
        let locked = Lock {
            language: Some("rust".into()),
            tools: vec![("rustc".into(), "1.97.0".into())],
            environment_image: None,
            source_hash: "aaa".into(),
        };
        let current = Lock {
            language: Some("rust".into()),
            tools: vec![("rustc".into(), "1.98.0".into())],
            environment_image: None,
            source_hash: "bbb".into(),
        };
        let drift = locked.diff(&current);
        assert!(drift.iter().any(|d| d.contains("rustc")));
        assert!(drift.iter().any(|d| d.contains("sources have changed")));
    }
}
