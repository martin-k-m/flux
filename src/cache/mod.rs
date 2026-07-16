//! A simple content-hash build cache.
//!
//! This is deliberately *not* a Bazel-style graph. Flux hashes the project's
//! source files (via SHA-256) and, per step, remembers the hash of the last
//! successful run. If the sources are byte-for-byte unchanged, the step is
//! skipped:
//!
//! ```text
//! No changes detected. Using cached build.
//! ```
//!
//! Cache state lives under `.flux-cache/builds/`. (The `.flux` name itself is
//! taken by the config *file*, so Flux keeps its internal state in a sibling
//! `.flux-cache/` directory.)

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Files we never hash (Flux's own metadata, not project sources).
const IGNORED_FILES: &[&str] = &[".flux.lock"];

/// Directories we never hash or descend into.
const IGNORED_DIRS: &[&str] = &[
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
];

/// The build cache rooted at a project directory.
pub struct Cache {
    project_root: PathBuf,
    builds_dir: PathBuf,
}

impl Cache {
    /// Open (but do not yet create) the cache for `project_root`.
    pub fn new(project_root: &Path) -> Self {
        let builds_dir = project_root.join(".flux-cache").join("builds");
        Cache {
            project_root: project_root.to_path_buf(),
            builds_dir,
        }
    }

    /// Compute a stable SHA-256 over all source files in the project.
    ///
    /// Both file paths and contents feed the hash, so adding, removing, or
    /// renaming a file changes the digest.
    pub fn source_hash(&self) -> String {
        let mut entries: Vec<PathBuf> = Vec::new();
        collect_files(&self.project_root, &mut entries);
        self.hash_paths(entries)
    }

    /// Compute a hash over only the files matching `patterns` (globs relative to
    /// the project root). This is the intelligent-cache core (Phase 3, 3.2): a
    /// step that declares `inputs [ "frontend/**" ]` is only invalidated when a
    /// matching file changes, so unrelated edits don't rebuild it.
    ///
    /// With no patterns, this falls back to the whole-project hash.
    pub fn source_hash_scoped(&self, patterns: &[String]) -> String {
        if patterns.is_empty() {
            return self.source_hash();
        }
        let mut all: Vec<PathBuf> = Vec::new();
        collect_files(&self.project_root, &mut all);
        let matched: Vec<PathBuf> = all
            .into_iter()
            .filter(|p| {
                p.strip_prefix(&self.project_root).is_ok_and(|rel| {
                    let rel = rel.to_string_lossy().replace('\\', "/");
                    patterns.iter().any(|pat| glob_match(pat, &rel))
                })
            })
            .collect();
        self.hash_paths(matched)
    }

    /// Hash a set of files (paths + contents) into a stable digest.
    fn hash_paths(&self, mut entries: Vec<PathBuf>) -> String {
        // Deterministic ordering regardless of filesystem enumeration order.
        entries.sort();

        let mut hasher = Sha256::new();
        for path in entries {
            if let Ok(rel) = path.strip_prefix(&self.project_root) {
                hasher.update(rel.to_string_lossy().as_bytes());
            }
            hasher.update([0u8]); // separator
            if let Ok(bytes) = std::fs::read(&path) {
                hasher.update(&bytes);
            }
            hasher.update([0u8]);
        }
        hex(&hasher.finalize())
    }

    /// Is the stored hash for `step` equal to `hash`?
    pub fn is_fresh(&self, step: &str, hash: &str) -> bool {
        self.stored_hash(step).as_deref() == Some(hash)
    }

    /// Record `hash` as the successful build hash for `step`.
    pub fn store(&self, step: &str, hash: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.builds_dir)?;
        std::fs::write(self.step_file(step), hash)
    }

    /// Remove the per-step build hashes (forces a full rebuild). This does NOT
    /// touch secrets, artifacts, runners, or analytics — only the build cache.
    pub fn clear_builds(&self) -> std::io::Result<()> {
        if self.builds_dir.exists() {
            std::fs::remove_dir_all(&self.builds_dir)?;
        }
        Ok(())
    }

    fn stored_hash(&self, step: &str) -> Option<String> {
        std::fs::read_to_string(self.step_file(step))
            .ok()
            .map(|s| s.trim().to_string())
    }

    fn step_file(&self, step: &str) -> PathBuf {
        // Sanitise the step name so it is always a safe filename.
        let safe: String = step
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.builds_dir.join(format!("{safe}.hash"))
    }
}

/// Recursively collect regular files under `dir`, skipping ignored directories.
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
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
            if !is_ignored(&path) {
                collect_files(&path, out);
            }
        } else if file_type.is_file() && !is_ignored_file(&path) {
            out.push(path);
        }
        // Symlinks are ignored to avoid cycles.
    }
}

/// Should this directory be skipped by the walker?
fn is_ignored(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| IGNORED_DIRS.contains(&name))
        .unwrap_or(false)
}

/// Should this file be excluded from the source hash?
fn is_ignored_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| IGNORED_FILES.contains(&name))
        .unwrap_or(false)
}

/// Lowercase hex encoding of a byte slice (avoids pulling in the `hex` crate).
fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Match a forward-slash `path` against a glob `pattern`.
///
/// Supports `**` (any number of path segments), `*` (any characters within a
/// segment), and `?` (one character within a segment). A pattern with no `/`
/// (e.g. `*.rs`) matches against the file's basename too, so `inputs [ "*.rs" ]`
/// works intuitively.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pat_segs: Vec<&str> = pattern.split('/').collect();
    let path_segs: Vec<&str> = path.split('/').collect();
    if match_segments(&pat_segs, &path_segs) {
        return true;
    }
    // A single-segment pattern also matches the basename anywhere in the tree.
    if pat_segs.len() == 1 {
        if let Some(base) = path_segs.last() {
            return segment_match(pat_segs[0], base);
        }
    }
    false
}

fn match_segments(pat: &[&str], seg: &[&str]) -> bool {
    if pat.is_empty() {
        return seg.is_empty();
    }
    if pat[0] == "**" {
        // `**` matches zero or more segments.
        for i in 0..=seg.len() {
            if match_segments(&pat[1..], &seg[i..]) {
                return true;
            }
        }
        return false;
    }
    if seg.is_empty() {
        return false;
    }
    if segment_match(pat[0], seg[0]) {
        return match_segments(&pat[1..], &seg[1..]);
    }
    false
}

/// Wildcard match within a single path segment (`*` and `?`, no `/`).
fn segment_match(pat: &str, seg: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let s: Vec<char> = seg.chars().collect();
    // Classic dynamic wildcard match.
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star, mut mark) = (None, 0usize);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_matches_double_star() {
        assert!(glob_match("frontend/**", "frontend/src/button.tsx"));
        assert!(glob_match("frontend/**", "frontend/index.ts"));
        assert!(!glob_match("frontend/**", "backend/main.rs"));
    }

    #[test]
    fn glob_matches_single_star_within_segment() {
        assert!(glob_match("src/*.rs", "src/main.rs"));
        assert!(!glob_match("src/*.rs", "src/nested/main.rs"));
    }

    #[test]
    fn bare_pattern_matches_basename() {
        assert!(glob_match("*.rs", "src/deep/main.rs"));
        assert!(glob_match("Cargo.toml", "Cargo.toml"));
        assert!(!glob_match("*.rs", "src/main.py"));
    }

    #[test]
    fn scoped_hash_ignores_unrelated_files() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("flux-cache-scope-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("frontend")).unwrap();
        std::fs::create_dir_all(dir.join("backend")).unwrap();
        std::fs::write(dir.join("frontend/a.ts"), "one").unwrap();
        std::fs::write(dir.join("backend/b.rs"), "two").unwrap();

        let cache = Cache::new(&dir);
        let patterns = vec!["frontend/**".to_string()];
        let before = cache.source_hash_scoped(&patterns);

        // Change an unrelated (backend) file: scoped hash must not move.
        std::fs::write(dir.join("backend/b.rs"), "changed").unwrap();
        assert_eq!(before, cache.source_hash_scoped(&patterns));

        // Change a matching (frontend) file: scoped hash must change.
        std::fs::write(dir.join("frontend/a.ts"), "changed").unwrap();
        assert_ne!(before, cache.source_hash_scoped(&patterns));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
