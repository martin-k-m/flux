//! A small, dependency-free directory walker shared by the intelligence,
//! knowledge, and agent modules.
//!
//! We hand-roll this (rather than pull in `walkdir`, which drags in
//! `winapi-util` → `windows-sys`; see CLAUDE.md) and skip the same noise
//! directories the build cache skips, so repository analysis never wanders into
//! `target/`, `.git/`, or `node_modules/`.

use std::path::{Path, PathBuf};

/// Directories we never descend into during analysis.
pub const IGNORED_DIRS: &[&str] = &[
    ".flux-cache",
    ".git",
    "target",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "dist",
    "build",
    ".mypy_cache",
    ".pytest_cache",
    ".idea",
    ".vscode",
];

/// Recursively collect regular files under `root`, skipping ignored directories
/// and symlinks. Paths are returned absolute (rooted at `root`).
pub fn collect_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            if !is_ignored_dir(&path) {
                walk(&path, out);
            }
        } else if file_type.is_file() {
            out.push(path);
        }
    }
}

fn is_ignored_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| IGNORED_DIRS.contains(&name))
        .unwrap_or(false)
}

/// The lowercase extension of a path, without the dot (e.g. `rs`), if any.
pub fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_ignored_dirs() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("flux-fsutil-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.join("target/junk.rs"), "// build output").unwrap();

        let files = collect_files(&dir);
        assert!(files.iter().any(|p| p.ends_with("main.rs")));
        assert!(!files.iter().any(|p| p.ends_with("junk.rs")));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
