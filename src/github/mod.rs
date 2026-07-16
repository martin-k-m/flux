//! GitHub integration — local-first, with optional `gh` CLI enrichment.
//!
//! Flux is not a hosted GitHub App (that's a documented non-goal — it needs a
//! server Flux deliberately doesn't run). Instead it does the parts that are
//! honest offline:
//!
//! * `flux github init` writes a ready-to-use Actions workflow and PR template;
//! * `flux github review` builds a review from the working-tree diff (or a PR
//!   diff when the `gh` CLI is installed and authenticated);
//! * `flux github plan` turns an issue into an implementation plan.
//!
//! Anything that would *post* to GitHub is left as an explicit `gh` command the
//! user runs — Flux never publishes on your behalf.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::agents::{self, RunCtx};
use crate::platform::PlatformConfig;

/// Is the `gh` CLI available on PATH?
pub fn gh_available() -> bool {
    Command::new("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Files written by [`init`].
pub struct InitResult {
    pub written: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

/// Scaffold GitHub CI + PR template. Existing files are skipped unless `force`.
pub fn init(root: &Path, force: bool) -> std::io::Result<InitResult> {
    let mut written = Vec::new();
    let mut skipped = Vec::new();

    let workflow_dir = root.join(".github").join("workflows");
    std::fs::create_dir_all(&workflow_dir)?;
    let workflow = workflow_dir.join("flux.yml");
    if workflow.exists() && !force {
        skipped.push(workflow);
    } else {
        std::fs::write(&workflow, WORKFLOW)?;
        written.push(workflow);
    }

    let template = root.join(".github").join("pull_request_template.md");
    if template.exists() && !force {
        skipped.push(template);
    } else {
        std::fs::write(&template, PR_TEMPLATE)?;
        written.push(template);
    }

    Ok(InitResult { written, skipped })
}

/// Build a review report for the current changes, or a named PR when `gh` is
/// available. Writes the report and returns it.
pub fn review(
    root: &Path,
    pr: Option<u32>,
    platform: &PlatformConfig,
) -> anyhow::Result<agents::Report> {
    // A specific PR needs `gh`; degrade honestly to the working tree otherwise.
    if let Some(number) = pr {
        if gh_available() {
            return Ok(pr_review(root, number));
        }
        anyhow::bail!(
            "reviewing PR #{number} needs the `gh` CLI (install from https://cli.github.com). \
             Without it, run `flux github review` to review your working tree."
        );
    }

    let ctx = RunCtx {
        root,
        arg: None,
        platform,
    };
    let reviewer = agents::Reviewer;
    Ok(agents::run(&reviewer, &ctx))
}

/// Review a PR by reading its diff via `gh pr diff`.
fn pr_review(root: &Path, number: u32) -> agents::Report {
    let mut report = agents::Report::new("reviewer", &format!("Flux Review — PR #{number}"));
    let out = Command::new("gh")
        .arg("-R")
        .arg(".")
        .current_dir(root)
        .args(["pr", "diff", &number.to_string(), "--name-only"])
        .output();

    match out {
        Ok(o) if o.status.success() => {
            let files: Vec<String> = String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect();
            report.summary = format!("{} file(s) changed in PR #{number}.", files.len());
            for f in &files {
                report.findings.push(agents::Finding::info(f.clone()));
            }
            report.recommendation =
                Some("Run `flux verify` on the branch, then merge when green.".into());
        }
        _ => {
            report.summary = format!("Could not read PR #{number} via `gh`.");
            report.findings.push(agents::Finding::warn(
                "check that `gh` is authenticated (`gh auth status`) and the PR number is correct",
            ));
        }
    }
    report
}

/// Turn an issue (or free-text description) into an implementation plan.
pub fn plan(
    root: &Path,
    issue: Option<u32>,
    description: Option<String>,
    platform: &PlatformConfig,
) -> agents::Report {
    // Prefer an explicit description; otherwise fetch the issue title via `gh`.
    let arg = description.or_else(|| issue.and_then(|n| gh_issue_title(root, n)));

    let ctx = RunCtx {
        root,
        arg,
        platform,
    };
    let planner = agents::Planner;
    let mut report = agents::run(&planner, &ctx);
    if let Some(n) = issue {
        report.title = format!("Flux Plan — issue #{n}");
    }
    report
}

fn gh_issue_title(root: &Path, number: u32) -> Option<String> {
    if !gh_available() {
        return None;
    }
    let out = Command::new("gh")
        .current_dir(root)
        .args([
            "issue",
            "view",
            &number.to_string(),
            "--json",
            "title",
            "-q",
            ".title",
        ])
        .output()
        .ok()?;
    if out.status.success() {
        let title = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if title.is_empty() {
            None
        } else {
            Some(title)
        }
    } else {
        None
    }
}

const WORKFLOW: &str = r#"# Generated by `flux github init`.
name: Flux CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  flux:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Format check
        run: flux fmt || true
      - name: Lint
        run: flux lint || true
      - name: Verify (fmt, clippy, tests)
        run: flux verify
      - name: Build pipeline
        run: flux build
"#;

const PR_TEMPLATE: &str = r#"## Summary

<!-- What does this change do? -->

## Flux checklist

- [ ] `flux verify` passes
- [ ] Tests added/updated for the change
- [ ] Docs updated (`flux docs`)

<!-- Tip: run `flux agent run reviewer` for a heuristic review before requesting one. -->
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_writes_workflow_and_template() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("flux-gh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let r = init(&dir, false).unwrap();
        assert_eq!(r.written.len(), 2);
        assert!(dir.join(".github/workflows/flux.yml").is_file());
        assert!(dir.join(".github/pull_request_template.md").is_file());

        // Second run skips existing files.
        let r2 = init(&dir, false).unwrap();
        assert_eq!(r2.written.len(), 0);
        assert_eq!(r2.skipped.len(), 2);

        // Force overwrites.
        let r3 = init(&dir, true).unwrap();
        assert_eq!(r3.written.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
