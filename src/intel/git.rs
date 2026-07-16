//! Git activity statistics, gathered by shelling out to `git`.
//!
//! Honest degradation (a Flux convention): if `git` isn't installed or the
//! directory isn't a repository, every field is empty/zero and `is_repo` is
//! false — we never invent history.

use std::path::Path;
use std::process::Command;

/// A snapshot of a repository's git activity.
#[derive(Debug, Clone, Default)]
pub struct GitStats {
    pub is_repo: bool,
    pub commits: usize,
    pub contributors: usize,
    pub branch: Option<String>,
    /// Relative date of the most recent commit (e.g. "3 days ago").
    pub last_commit: Option<String>,
}

/// Gather git stats for `root`. Returns defaults when git is unavailable.
pub fn analyze(root: &Path) -> GitStats {
    if !super::git_available() {
        return GitStats::default();
    }
    if !is_repo(root) {
        return GitStats::default();
    }

    GitStats {
        is_repo: true,
        commits: count_commits(root),
        contributors: count_contributors(root),
        branch: current_branch(root),
        last_commit: last_commit_relative(root),
    }
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn is_repo(root: &Path) -> bool {
    git(root, &["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true")
}

fn count_commits(root: &Path) -> usize {
    git(root, &["rev-list", "--count", "HEAD"])
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn count_contributors(root: &Path) -> usize {
    match git(root, &["shortlog", "-sn", "HEAD"]) {
        Some(s) if !s.is_empty() => s.lines().count(),
        _ => 0,
    }
}

fn current_branch(root: &Path) -> Option<String> {
    let b = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if b.is_empty() {
        None
    } else {
        Some(b)
    }
}

fn last_commit_relative(root: &Path) -> Option<String> {
    let d = git(root, &["log", "-1", "--format=%cr"])?;
    if d.is_empty() {
        None
    } else {
        Some(d)
    }
}
