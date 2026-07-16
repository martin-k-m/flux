//! `flux changelog` — generate a changelog from git commits (4.5).
//!
//! Commits are grouped by Conventional-Commit prefix (`feat:`, `fix:`, …).
//! Non-conforming subjects land under "Other".

use std::path::Path;
use std::process::Command;

/// Generate a Markdown changelog from commits since the last tag (or all
/// commits when there are no tags).
pub fn generate(root: &Path) -> anyhow::Result<String> {
    let range = last_tag(root).map(|t| format!("{t}..HEAD"));
    let subjects = commit_subjects(root, range.as_deref())?;
    Ok(group(&subjects))
}

/// Group commit subjects into a Markdown changelog.
pub fn group(subjects: &[String]) -> String {
    let mut features = Vec::new();
    let mut fixes = Vec::new();
    let mut docs = Vec::new();
    let mut other = Vec::new();

    for s in subjects {
        let lower = s.to_lowercase();
        let bucket = if starts_with_type(&lower, "feat") {
            &mut features
        } else if starts_with_type(&lower, "fix") {
            &mut fixes
        } else if starts_with_type(&lower, "docs") {
            &mut docs
        } else {
            &mut other
        };
        bucket.push(strip_type(s));
    }

    let mut out = String::from("# Changelog\n");
    section(&mut out, "Features", &features);
    section(&mut out, "Fixes", &fixes);
    section(&mut out, "Documentation", &docs);
    section(&mut out, "Other", &other);
    if features.is_empty() && fixes.is_empty() && docs.is_empty() && other.is_empty() {
        out.push_str("\n_No commits found._\n");
    }
    out
}

fn section(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("\n## {title}\n"));
    for item in items {
        out.push_str(&format!("- {item}\n"));
    }
}

/// Does `subject` start with a conventional type, e.g. `feat:` or `feat(x):`?
fn starts_with_type(subject: &str, ty: &str) -> bool {
    if let Some(rest) = subject.strip_prefix(ty) {
        rest.starts_with(':') || rest.starts_with('(')
    } else {
        false
    }
}

/// Strip a leading `type:` or `type(scope):` prefix from a subject.
fn strip_type(subject: &str) -> String {
    if let Some(idx) = subject.find(':') {
        // Only strip when the part before ':' looks like a type/scope (no space).
        let prefix = &subject[..idx];
        if !prefix.contains(' ') {
            return subject[idx + 1..].trim().to_string();
        }
    }
    subject.trim().to_string()
}

fn last_tag(root: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let tag = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if tag.is_empty() {
        None
    } else {
        Some(tag)
    }
}

fn commit_subjects(root: &Path, range: Option<&str>) -> anyhow::Result<Vec<String>> {
    let mut args = vec!["log".to_string(), "--pretty=%s".to_string()];
    if let Some(r) = range {
        args.push(r.to_string());
    }
    let out = Command::new("git").args(&args).current_dir(root).output()?;
    if !out.status.success() {
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_by_conventional_type() {
        let subjects = vec![
            "feat: add graph engine".to_string(),
            "feat(cache): scoped inputs".to_string(),
            "fix: correct only_if".to_string(),
            "docs: update readme".to_string(),
            "random cleanup".to_string(),
        ];
        let md = group(&subjects);
        assert!(md.contains("## Features"));
        assert!(md.contains("- add graph engine"));
        assert!(md.contains("- scoped inputs"));
        assert!(md.contains("## Fixes"));
        assert!(md.contains("- correct only_if"));
        assert!(md.contains("## Documentation"));
        assert!(md.contains("## Other"));
        assert!(md.contains("- random cleanup"));
    }

    #[test]
    fn empty_when_no_commits() {
        assert!(group(&[]).contains("No commits found"));
    }
}
