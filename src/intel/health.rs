//! A deterministic, explainable project health score.
//!
//! The score is a weighted sum of concrete signals — each either earned or not,
//! with a one-line reason. There is no magic and no network: run it twice on the
//! same tree and you get the same number, and every point is attributable to a
//! [`Signal`]. This keeps `flux project` honest and lets the health drop be
//! actionable ("+15 if you add a CI workflow") rather than a black box.

use std::path::Path;

use super::deps::Dependencies;
use super::git::GitStats;
use crate::core::detect::Detection;
use crate::fsutil;

/// One health signal: a weighted, pass/fail check with an explanation.
#[derive(Debug, Clone)]
pub struct Signal {
    pub name: String,
    pub ok: bool,
    pub weight: u32,
    pub detail: String,
}

/// The overall score plus its constituent signals.
#[derive(Debug, Clone)]
pub struct HealthScore {
    /// 0–100.
    pub score: u32,
    pub signals: Vec<Signal>,
}

impl HealthScore {
    /// A one-word grade for display.
    pub fn grade(&self) -> &'static str {
        match self.score {
            90..=100 => "excellent",
            75..=89 => "healthy",
            50..=74 => "fair",
            _ => "needs work",
        }
    }

    /// The signals that were *not* earned, best improvements first (highest
    /// weight). Drives recommendations in `flux project` and the dashboard.
    pub fn gaps(&self) -> Vec<&Signal> {
        let mut gaps: Vec<&Signal> = self.signals.iter().filter(|s| !s.ok).collect();
        gaps.sort_by_key(|s| std::cmp::Reverse(s.weight));
        gaps
    }
}

/// Compute the health score for `root`.
pub fn score(
    root: &Path,
    detection: &Detection,
    git: &GitStats,
    deps: &Dependencies,
) -> HealthScore {
    let todo_count = count_todos(root);
    let signals = vec![
        signal(
            "README",
            root.join("README.md").is_file(),
            10,
            "a top-level README documents the project",
        ),
        signal(
            "Documentation",
            root.join("docs").is_dir(),
            10,
            "a docs/ directory holds reference material",
        ),
        signal(
            "Tests",
            detection.has_tests,
            20,
            "the project has an automated test suite",
        ),
        signal(
            "CI",
            has_ci(root),
            15,
            "a CI workflow runs checks on every change",
        ),
        signal(
            "Flux pipeline",
            root.join(crate::core::config::CONFIG_FILE).is_file(),
            10,
            "a .flux pipeline captures build/test/ship",
        ),
        signal(
            "Locked dependencies",
            deps.locked,
            10,
            "a lockfile pins exact dependency versions",
        ),
        signal(
            "Toolchain",
            detection.toolchain_available,
            10,
            "the language toolchain is installed",
        ),
        signal(
            "Version control",
            git.is_repo,
            5,
            "the project is tracked in git",
        ),
        signal(
            "Low TODO debt",
            todo_count <= 25,
            10,
            &format!("{todo_count} TODO/FIXME markers in source"),
        ),
    ];

    let total: u32 = signals.iter().map(|s| s.weight).sum();
    let earned: u32 = signals.iter().filter(|s| s.ok).map(|s| s.weight).sum();
    let score = (earned * 100).checked_div(total).unwrap_or(0);

    HealthScore { score, signals }
}

fn signal(name: &str, ok: bool, weight: u32, detail: &str) -> Signal {
    Signal {
        name: name.to_string(),
        ok,
        weight,
        detail: detail.to_string(),
    }
}

/// Detect a CI configuration (GitHub Actions, GitLab CI, or a Jenkinsfile).
fn has_ci(root: &Path) -> bool {
    let workflows = root.join(".github").join("workflows");
    if workflows.is_dir() {
        if let Ok(mut entries) = std::fs::read_dir(&workflows) {
            if entries.any(|e| e.is_ok()) {
                return true;
            }
        }
    }
    root.join(".gitlab-ci.yml").is_file() || root.join("Jenkinsfile").is_file()
}

/// Count `TODO`/`FIXME` markers across source files.
pub fn count_todos(root: &Path) -> usize {
    let mut count = 0;
    for file in fsutil::collect_files(root) {
        if !fsutil::extension(&file).as_deref().is_some_and(is_code) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&file) {
            for line in text.lines() {
                if line.contains("TODO") || line.contains("FIXME") {
                    count += 1;
                }
            }
        }
    }
    count
}

fn is_code(ext: &str) -> bool {
    matches!(
        ext,
        "rs" | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "py"
            | "go"
            | "java"
            | "kt"
            | "rb"
            | "c"
            | "h"
            | "cpp"
            | "cc"
            | "hpp"
            | "cs"
            | "php"
            | "swift"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::detect::Detection;

    fn detection(has_tests: bool, toolchain: bool) -> Detection {
        Detection {
            language: Some("rust".into()),
            name: Some("x".into()),
            markers: vec![],
            has_tests,
            toolchain_available: toolchain,
        }
    }

    #[test]
    fn empty_project_scores_low_but_deterministic() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("flux-health-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let d = detection(false, false);
        let a = score(&dir, &d, &GitStats::default(), &Dependencies::default());
        let b = score(&dir, &d, &GitStats::default(), &Dependencies::default());
        assert_eq!(a.score, b.score);
        assert!(a.score < 50);
        assert!(!a.gaps().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn signals_earn_their_weight() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("flux-health-full-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::create_dir_all(dir.join(".github/workflows")).unwrap();
        std::fs::write(dir.join("README.md"), "# x").unwrap();
        std::fs::write(dir.join(".github/workflows/ci.yml"), "on: push").unwrap();
        std::fs::write(dir.join(".flux"), "language rust").unwrap();

        let d = detection(true, true);
        let deps = Dependencies {
            total: 3,
            names: vec![],
            locked: true,
            source: Some("Cargo.toml".into()),
        };
        let git = GitStats {
            is_repo: true,
            ..Default::default()
        };
        let h = score(&dir, &d, &git, &deps);
        // README+docs+tests+CI+flux+locked+toolchain+git+lowtodo = everything.
        assert_eq!(h.score, 100);
        assert_eq!(h.grade(), "excellent");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
