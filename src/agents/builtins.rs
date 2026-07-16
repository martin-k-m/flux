//! The built-in agents.
//!
//! Every agent here is a heuristic analyzer — deterministic, offline, and honest
//! about being a heuristic. None of them call a model; the optional AI provider
//! is layered on by [`super::run`] after the fact.

use std::path::Path;
use std::process::Command;

use super::{Agent, Finding, Report, RunCtx};
use crate::fsutil;

// ---------------------------------------------------------------------------
// Planner
// ---------------------------------------------------------------------------

/// Turns a feature description into a structured implementation-plan skeleton for
/// an AI or human to flesh out.
pub struct Planner;

impl Agent for Planner {
    fn name(&self) -> &'static str {
        "planner"
    }
    fn description(&self) -> &'static str {
        "Break a feature or issue into an implementation plan"
    }
    fn run(&self, ctx: &RunCtx) -> Report {
        let feature = ctx
            .arg
            .clone()
            .unwrap_or_else(|| "the requested change".to_string());
        let mut r = Report::new(self.name(), "Flux Planner").summary(format!(
            "A heuristic implementation plan for: {feature}. Fill in the specifics per step."
        ));
        for (i, step) in [
            "Model the data / schema changes",
            "Implement the core service or logic",
            "Wire it into the CLI / API surface",
            "Add unit and integration tests",
            "Update documentation and examples",
        ]
        .iter()
        .enumerate()
        {
            r.findings
                .push(Finding::action(format!("{}. {step}", i + 1)));
        }
        r.recommend(
            "This is a scaffold, not a decision. Configure `ai.command` in flux.yaml to have an \
             external model expand each step against your codebase.",
        )
    }
}

// ---------------------------------------------------------------------------
// Reviewer
// ---------------------------------------------------------------------------

/// Summarises the working-tree diff and flags changed code that lacks a matching
/// test change.
pub struct Reviewer;

impl Agent for Reviewer {
    fn name(&self) -> &'static str {
        "reviewer"
    }
    fn description(&self) -> &'static str {
        "Summarise the current changes and flag review risks"
    }
    fn run(&self, ctx: &RunCtx) -> Report {
        let mut r = Report::new(self.name(), "Flux Review");

        if !crate::intel::git_available() || !is_repo(ctx.root) {
            r.findings
                .push(Finding::warn("not a git repository — nothing to review"));
            return r.summary("No git history available.");
        }

        let changed = changed_files(ctx.root);
        if changed.is_empty() {
            return r
                .summary("The working tree is clean — no changes to review.")
                .recommend("Make some changes, then run `flux agent run reviewer` again.");
        }

        r = r.summary(format!(
            "{} file(s) changed in the working tree.",
            changed.len()
        ));

        // Group by top-level component for a readable summary.
        let mut code_changed = false;
        let mut test_changed = false;
        for f in &changed {
            let norm = f.replace('\\', "/");
            if is_test_path(&norm) {
                test_changed = true;
            } else if is_source_file(&norm) {
                code_changed = true;
            }
            r.findings.push(Finding::info(norm));
        }

        if code_changed && !test_changed {
            r.findings.push(Finding::action(
                "source changed but no test files changed — consider adding coverage",
            ));
        }

        // Report what verification is available.
        if let Some(lang) = crate::core::detect::detect(ctx.root).language {
            if crate::tools::fmt_command(&lang).is_some() {
                r.findings.push(Finding::info(format!(
                    "run `flux fmt` / `flux lint` before merging ({lang})"
                )));
            }
        }

        let rec = if code_changed && !test_changed {
            "Add tests for the changed code, then run `flux verify`."
        } else {
            "Run `flux verify` to confirm fmt, lint, and tests pass."
        };
        r.recommend(rec)
    }
}

// ---------------------------------------------------------------------------
// Tester
// ---------------------------------------------------------------------------

/// Finds source modules that expose public functions but contain no tests.
pub struct Tester;

impl Agent for Tester {
    fn name(&self) -> &'static str {
        "tester"
    }
    fn description(&self) -> &'static str {
        "Find code that lacks test coverage (heuristic)"
    }
    fn run(&self, ctx: &RunCtx) -> Report {
        let mut r = Report::new(self.name(), "Flux Tester");
        let files = fsutil::collect_files(ctx.root);
        let rust: Vec<_> = files
            .iter()
            .filter(|f| fsutil::extension(f).as_deref() == Some("rs"))
            .collect();

        if rust.is_empty() {
            return r
                .summary("Test-coverage heuristics currently target Rust source.")
                .recommend("For other languages, wire your test command into the .flux pipeline.");
        }

        let mut untested = Vec::new();
        for f in &rust {
            let Ok(text) = std::fs::read_to_string(f) else {
                continue;
            };
            let has_pub_fn = text.contains("pub fn ");
            let has_tests = text.contains("#[test]") || text.contains("#[cfg(test)]");
            if has_pub_fn && !has_tests {
                if let Ok(rel) = f.strip_prefix(ctx.root) {
                    untested.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }

        if untested.is_empty() {
            return r
                .summary("Every Rust file with a public API also carries tests. Nice.")
                .recommend("Keep it up — add tests alongside new public functions.");
        }

        r = r.summary(format!(
            "{} Rust file(s) expose a public API but contain no tests.",
            untested.len()
        ));
        for f in untested.iter().take(20) {
            r.findings
                .push(Finding::action(format!("add tests for {f}")));
        }
        if untested.len() > 20 {
            r.findings
                .push(Finding::info(format!("… and {} more", untested.len() - 20)));
        }
        r.recommend("Add a `#[cfg(test)] mod tests` block to each file above.")
    }
}

// ---------------------------------------------------------------------------
// Documentation
// ---------------------------------------------------------------------------

/// Flags documentation gaps: missing README sections and undocumented docs pages.
pub struct Documentation;

impl Agent for Documentation {
    fn name(&self) -> &'static str {
        "documentation"
    }
    fn description(&self) -> &'static str {
        "Detect documentation gaps"
    }
    fn run(&self, ctx: &RunCtx) -> Report {
        let mut r = Report::new(self.name(), "Flux Documentation");
        let root = ctx.root;

        let readme = root.join("README.md");
        if !readme.is_file() {
            r.findings.push(Finding::action(
                "no README.md — add one describing the project",
            ));
        } else {
            let text = std::fs::read_to_string(&readme).unwrap_or_default();
            for section in ["install", "usage", "license"] {
                if !text.to_lowercase().contains(section) {
                    r.findings
                        .push(Finding::warn(format!("README has no '{section}' section")));
                }
            }
        }

        if !root.join("docs").is_dir() {
            r.findings.push(Finding::action(
                "no docs/ directory — add reference documentation",
            ));
        }
        if !root.join("CHANGELOG.md").is_file() {
            r.findings.push(Finding::warn(
                "no CHANGELOG.md — `flux changelog` can generate one",
            ));
        }

        if r.findings.is_empty() {
            return r
                .summary("Documentation basics are in place.")
                .recommend("Run `flux docs` to keep generated reference sections in sync.");
        }
        let count = r.findings.len();
        r.summary(format!("{count} documentation gap(s) found."))
            .recommend("Address the gaps above, then run `flux docs` to regenerate references.")
    }
}

// ---------------------------------------------------------------------------
// Maintenance
// ---------------------------------------------------------------------------

/// General housekeeping: dependency inventory, TODO debt, and project hygiene.
pub struct Maintenance;

impl Agent for Maintenance {
    fn name(&self) -> &'static str {
        "maintenance"
    }
    fn description(&self) -> &'static str {
        "Housekeeping: dependencies, TODO debt, hygiene"
    }
    fn run(&self, ctx: &RunCtx) -> Report {
        let intel = crate::intel::analyze(ctx.root);
        let mut r = Report::new(self.name(), "Flux Maintenance").summary(format!(
            "Health {}% ({}). Heuristic housekeeping scan.",
            intel.health.score,
            intel.health.grade()
        ));

        // Dependency inventory (no network → we report, we don't fake "outdated").
        let d = &intel.dependencies;
        if let Some(src) = &d.source {
            let lock = if d.locked { "locked" } else { "not locked" };
            r.findings.push(Finding::info(format!(
                "{} dependencies in {src} ({lock})",
                d.total
            )));
            if !d.locked {
                r.findings.push(Finding::action(
                    "commit a lockfile to pin dependency versions",
                ));
            }
        }

        // TODO debt.
        let todos = crate::intel::health::count_todos(ctx.root);
        if todos > 25 {
            r.findings.push(Finding::action(format!(
                "{todos} TODO/FIXME markers — consider triaging into issues"
            )));
        } else {
            r.findings
                .push(Finding::info(format!("{todos} TODO/FIXME markers")));
        }

        // Surface the top health gaps as concrete work.
        for gap in intel.health.gaps().into_iter().take(3) {
            r.findings
                .push(Finding::action(format!("{}: {}", gap.name, gap.detail)));
        }

        let has_gaps = r
            .findings
            .iter()
            .any(|f| f.severity == super::Severity::Action);
        if has_gaps {
            r.recommend("Work through the action items above to raise the health score.")
        } else {
            r.recommend("Nothing urgent — the project is in good shape.")
        }
    }
}

// ---------------------------------------------------------------------------
// Release
// ---------------------------------------------------------------------------

/// Previews release readiness: version, and the changelog since the last tag.
pub struct Release;

impl Agent for Release {
    fn name(&self) -> &'static str {
        "release"
    }
    fn description(&self) -> &'static str {
        "Preview release notes and versioning"
    }
    fn run(&self, ctx: &RunCtx) -> Report {
        let mut r = Report::new(self.name(), "Flux Release");

        match crate::tools::changelog::generate(ctx.root) {
            Ok(md) if !md.trim().is_empty() => {
                let lines = md
                    .lines()
                    .filter(|l| l.trim_start().starts_with('-'))
                    .count();
                r = r.summary(format!("{lines} change(s) since the last release tag."));
                for line in md
                    .lines()
                    .filter(|l| l.trim_start().starts_with('-'))
                    .take(15)
                {
                    r.findings.push(Finding::info(line.trim().to_string()));
                }
            }
            _ => {
                r = r.summary("No git history or no changes since the last tag.");
            }
        }

        r.recommend(
            "When ready: `flux version <part>` to bump, then `flux release create v<x.y.z>`.",
        )
    }
}

// ---------------------------------------------------------------------------
// Shared git helpers (working-tree diff)
// ---------------------------------------------------------------------------

fn is_repo(root: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Changed + untracked files in the working tree, via `git status --porcelain`.
fn changed_files(root: &Path) -> Vec<String> {
    let out = match Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain"])
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|line| {
            // Format: "XY <path>" (or "XY <old> -> <new>" for renames).
            let path = line.get(3..)?.trim();
            let path = path.split(" -> ").last().unwrap_or(path);
            if path.is_empty() {
                None
            } else {
                Some(path.trim_matches('"').to_string())
            }
        })
        .collect()
}

fn is_test_path(path: &str) -> bool {
    path.contains("/tests/")
        || path.starts_with("tests/")
        || path.contains("test_")
        || path.contains("_test.")
        || path.contains(".test.")
        || path.contains(".spec.")
}

fn is_source_file(path: &str) -> bool {
    matches!(
        path.rsplit('.').next(),
        Some(
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
                | "cpp"
                | "cs"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::PlatformConfig;

    fn ctx<'a>(root: &'a Path, platform: &'a PlatformConfig, arg: Option<String>) -> RunCtx<'a> {
        RunCtx {
            root,
            arg,
            platform,
        }
    }

    #[test]
    fn planner_produces_ordered_steps() {
        let platform = PlatformConfig::default();
        let root = std::env::temp_dir();
        let report = Planner.run(&ctx(&root, &platform, Some("notifications".into())));
        assert!(report.summary.contains("notifications"));
        assert_eq!(report.findings.len(), 5);
    }

    #[test]
    fn tester_flags_untested_public_api() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("flux-tester-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "pub fn a() {}").unwrap();
        std::fs::write(
            dir.join("src/b.rs"),
            "pub fn b() {}\n#[cfg(test)] mod t { #[test] fn x() {} }",
        )
        .unwrap();

        let platform = PlatformConfig::default();
        let report = Tester.run(&ctx(&dir, &platform, None));
        assert!(report
            .findings
            .iter()
            .any(|f| f.message.contains("src/a.rs")));
        assert!(!report
            .findings
            .iter()
            .any(|f| f.message.contains("src/b.rs")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_path_detection() {
        assert!(is_test_path("tests/integration.rs"));
        assert!(is_test_path("src/foo.test.ts"));
        assert!(!is_test_path("src/foo.rs"));
    }
}
