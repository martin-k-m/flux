//! `flux ask` — a natural-language front door to the repository's own data.
//!
//! Flux embeds no model. What it *does* have is a structured understanding of
//! the project (intelligence, knowledge graph, run history). `ask` assembles
//! that into a **context bundle** and either:
//!
//! * pipes it to a configured external model (`ai.command` in flux.yaml), so an
//!   LLM answers grounded in real project data; or
//! * answers offline by routing common questions to the data Flux already has —
//!   clearly labelled as a heuristic answer, never a fabricated one.
//!
//! `flux ask --context` prints the bundle itself: the honest "for AI to use"
//! surface, so you can pipe it into any tool you like.

use std::path::Path;

use crate::agents::{self, Agent, RunCtx};
use crate::platform::PlatformConfig;

/// Build the plain-text context bundle describing the project.
pub fn context_bundle(root: &Path) -> String {
    let intel = crate::intel::analyze(root);
    let mut b = String::new();
    b.push_str(&format!("# Project: {}\n", intel.project));
    if let Some(lang) = &intel.primary_language {
        b.push_str(&format!(
            "Primary language: {}\n",
            crate::intel::language_display(lang)
        ));
    }
    b.push_str(&format!(
        "Health: {}% ({})\n",
        intel.health.score,
        intel.health.grade()
    ));
    b.push_str(&format!("Source files: {}\n", intel.file_count));

    b.push_str("\n## Languages\n");
    for (lang, n) in &intel.languages {
        b.push_str(&format!("- {lang}: {n} files\n"));
    }

    b.push_str("\n## Components\n");
    for c in &intel.components {
        let deps = if c.depends_on.is_empty() {
            String::new()
        } else {
            format!(" → {}", c.depends_on.join(", "))
        };
        b.push_str(&format!("- {} ({} files){deps}\n", c.name, c.files));
    }

    b.push_str(&format!(
        "\n## Dependencies\n{} declared{}\n",
        intel.dependencies.total,
        intel
            .dependencies
            .source
            .as_ref()
            .map(|s| format!(" in {s}"))
            .unwrap_or_default()
    ));

    b.push_str("\n## Health signals\n");
    for s in &intel.health.signals {
        let mark = if s.ok { "ok" } else { "gap" };
        b.push_str(&format!("- [{mark}] {} ({})\n", s.name, s.detail));
    }

    if intel.git.is_repo {
        b.push_str(&format!(
            "\n## Git\n{} commits, {} contributor(s){}\n",
            intel.git.commits,
            intel.git.contributors,
            intel
                .git
                .last_commit
                .as_ref()
                .map(|d| format!(", last commit {d}"))
                .unwrap_or_default()
        ));
    }

    b
}

/// The outcome of an `ask`: the answer text and whether an external model
/// produced it (vs. an offline heuristic).
pub struct Answer {
    pub text: String,
    pub ai_used: bool,
}

/// Answer `question` about the project at `root`.
pub fn answer(root: &Path, question: &str, platform: &PlatformConfig) -> Answer {
    let bundle = context_bundle(root);

    // Prefer a configured external model, grounded in the bundle.
    if let Some(cmd) = platform.ai_command() {
        if let Ok(reply) = pipe_to_model(cmd, &bundle, question) {
            if !reply.trim().is_empty() {
                return Answer {
                    text: reply,
                    ai_used: true,
                };
            }
        }
    }

    Answer {
        text: offline_answer(root, question, platform),
        ai_used: false,
    }
}

/// Route a question to the project data Flux already has. Honest and bounded:
/// it never invents facts, and says when it can only point you at a command.
fn offline_answer(root: &Path, question: &str, platform: &PlatformConfig) -> String {
    let q = question.to_lowercase();

    let mentions = |words: &[&str]| words.iter().any(|w| q.contains(w));

    if mentions(&["fail", "failed", "error", "broke", "broken"]) {
        let a = crate::analytics::analyze(root).ok();
        let failures = a.map(|a| a.failures).unwrap_or(0);
        return format!(
            "Flux doesn't store raw build output offline, so it can't read the exact failure. \
             Recorded run history shows {failures} past failure(s).\n\n\
             To see the real error, run `flux verify` (or the failing step via `flux run <step>`); \
             Flux's failure assistant will diagnose the output. Configure `ai.command` in flux.yaml \
             for a model-assisted explanation."
        );
    }

    if mentions(&["next", "work on", "todo", "should i", "priorit"]) {
        let ctx = RunCtx {
            root,
            arg: None,
            platform,
        };
        let report = agents::Maintenance.run(&ctx);
        let mut out =
            String::from("Based on a heuristic maintenance scan, the highest-value work is:\n\n");
        let mut any = false;
        for f in report
            .findings
            .iter()
            .filter(|f| matches!(f.severity, crate::agents::Severity::Action))
        {
            out.push_str(&format!("- {}\n", f.message));
            any = true;
        }
        if !any {
            out.push_str("- Nothing urgent — the project is in good shape.\n");
        }
        return out;
    }

    if mentions(&["test", "coverage"]) {
        let ctx = RunCtx {
            root,
            arg: None,
            platform,
        };
        return agents::Tester.run(&ctx).summary;
    }

    if mentions(&[
        "explain",
        "overview",
        "what is",
        "what does",
        "describe",
        "understand",
    ]) {
        return context_bundle(root);
    }

    // Default: hand back the bundle so the user (or their own tools) can use it.
    format!(
        "No offline heuristic matched that question, so here is what Flux knows about the \
         project — pipe it into any model, or set `ai.command` in flux.yaml:\n\n{}",
        context_bundle(root)
    )
}

fn pipe_to_model(command: &str, bundle: &str, question: &str) -> std::io::Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty ai.command"))?;
    let args: Vec<&str> = parts.collect();

    let mut child = Command::new(program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        writeln!(stdin, "Question: {question}\n")?;
        writeln!(stdin, "Answer using only this project context:\n")?;
        stdin.write_all(bundle.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_describes_project() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("flux-ask-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"askdemo\"\n").unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();

        let bundle = context_bundle(&dir);
        assert!(bundle.contains("askdemo"));
        assert!(bundle.contains("Health:"));
        assert!(bundle.contains("## Components"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn offline_answer_is_labelled_and_grounded() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("flux-ask-off-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();

        let platform = PlatformConfig::default();
        let a = answer(&dir, "explain this repository", &platform);
        assert!(!a.ai_used);
        assert!(a.text.contains("Project: x"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
