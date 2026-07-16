//! The Flux AI agent framework.
//!
//! An "agent" here is an honest, deterministic analyzer that inspects the
//! repository and produces a **structured report** — findings plus a
//! recommendation — written to `.flux-cache/reports/`. Flux embeds no language
//! model: the built-in agents reason with heuristics and clearly say so.
//!
//! Where a real model *is* wanted, the design stays "for AI and users to use":
//! if `flux.yaml` configures `ai.command`, an agent pipes its assembled context
//! to that external CLI (Ollama, `claude`, etc.) and appends the reply as an
//! *AI analysis* section. Without it, you still get the heuristic report — never
//! a fake "success". This mirrors Flux's honest-degradation convention for
//! docker/kubectl/formatters.

mod builtins;

// Re-exported so callers (ask, github) can construct specific agents directly.
// The rest are reached via `registry()`.
pub use builtins::{Maintenance, Planner, Reviewer, Tester};

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::platform::PlatformConfig;

/// How urgent a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warn,
    Action,
}

impl Severity {
    fn glyph(self) -> &'static str {
        match self {
            Severity::Info => crate::core::logging::CHECK,
            Severity::Warn => "\u{26a0}", // ⚠
            Severity::Action => crate::core::logging::ARROW,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Action => "action",
        }
    }
}

/// A single observation an agent made.
#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
}

impl Finding {
    pub fn info(msg: impl Into<String>) -> Finding {
        Finding {
            severity: Severity::Info,
            message: msg.into(),
        }
    }
    pub fn warn(msg: impl Into<String>) -> Finding {
        Finding {
            severity: Severity::Warn,
            message: msg.into(),
        }
    }
    pub fn action(msg: impl Into<String>) -> Finding {
        Finding {
            severity: Severity::Action,
            message: msg.into(),
        }
    }
}

/// A structured agent report.
#[derive(Debug, Clone)]
pub struct Report {
    pub agent: String,
    pub title: String,
    pub summary: String,
    pub findings: Vec<Finding>,
    pub recommendation: Option<String>,
    /// Optional AI-provider output, appended when `ai.command` is configured.
    pub ai_analysis: Option<String>,
}

impl Report {
    /// Create an empty report for `agent` with a display `title`.
    pub fn new(agent: &str, title: &str) -> Report {
        Report {
            agent: agent.to_string(),
            title: title.to_string(),
            summary: String::new(),
            findings: Vec::new(),
            recommendation: None,
            ai_analysis: None,
        }
    }

    /// Render the report as Markdown (what lands in `.flux-cache/reports/*.md`).
    pub fn to_markdown(&self) -> String {
        let mut md = format!("# {}\n\n", self.title);
        if !self.summary.is_empty() {
            md.push_str(&format!("{}\n\n", self.summary));
        }
        if self.findings.is_empty() {
            md.push_str("_No findings._\n");
        } else {
            md.push_str("## Findings\n\n");
            for f in &self.findings {
                md.push_str(&format!("- **[{}]** {}\n", f.severity.label(), f.message));
            }
        }
        if let Some(rec) = &self.recommendation {
            md.push_str(&format!("\n## Recommendation\n\n{rec}\n"));
        }
        if let Some(ai) = &self.ai_analysis {
            md.push_str(&format!("\n## AI analysis\n\n{ai}\n"));
        }
        md
    }

    /// A plain-text context bundle handed to an external AI provider.
    fn to_context(&self) -> String {
        let mut c = format!(
            "Flux agent report: {}\n\n{}\n\nFindings:\n",
            self.title, self.summary
        );
        for f in &self.findings {
            c.push_str(&format!("- [{}] {}\n", f.severity.label(), f.message));
        }
        c
    }

    /// Render the report to the terminal in Flux's house style.
    pub fn print(&self) {
        use crate::core::logging as log;
        log::heading(&self.title);
        if !self.summary.is_empty() {
            log::info_line(&format!("  {}", log::dim(&self.summary)));
        }
        println!();
        for f in &self.findings {
            let glyph = f.severity.glyph();
            let painted = match f.severity {
                Severity::Info => log::green(glyph),
                Severity::Warn => log::yellow(glyph),
                Severity::Action => log::cyan(glyph),
            };
            println!("  {} {}", painted, f.message);
        }
        if let Some(rec) = &self.recommendation {
            log::heading("Recommendation:");
            log::info_line(&format!("  {rec}"));
        }
        if let Some(ai) = &self.ai_analysis {
            log::heading("AI analysis:");
            for line in ai.lines() {
                log::info_line(&format!("  {line}"));
            }
        }
    }

    /// Persist the report as Markdown under `.flux-cache/reports/`.
    pub fn write(&self, root: &Path) -> std::io::Result<PathBuf> {
        let dir = reports_dir(root);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.md", self.agent));
        std::fs::write(&path, self.to_markdown())?;
        Ok(path)
    }

    fn summary(mut self, s: impl Into<String>) -> Report {
        self.summary = s.into();
        self
    }

    fn recommend(mut self, s: impl Into<String>) -> Report {
        self.recommendation = Some(s.into());
        self
    }
}

/// The directory Flux writes agent reports into.
pub fn reports_dir(root: &Path) -> PathBuf {
    root.join(".flux-cache").join("reports")
}

/// Context passed to an agent's `run`.
pub struct RunCtx<'a> {
    pub root: &'a Path,
    /// A free-text argument (e.g. the feature to plan). `None` for most agents.
    pub arg: Option<String>,
    pub platform: &'a PlatformConfig,
}

/// An agent: a named, described analyzer.
pub trait Agent {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn run(&self, ctx: &RunCtx) -> Report;
}

/// All built-in agents, in catalogue order.
pub fn registry() -> Vec<Box<dyn Agent>> {
    vec![
        Box::new(builtins::Planner),
        Box::new(builtins::Reviewer),
        Box::new(builtins::Tester),
        Box::new(builtins::Documentation),
        Box::new(builtins::Maintenance),
        Box::new(builtins::Release),
    ]
}

/// Find a built-in agent by name.
pub fn find(name: &str) -> Option<Box<dyn Agent>> {
    registry().into_iter().find(|a| a.name() == name)
}

/// Run an agent and, if an AI provider is configured, enrich the report by
/// piping its context to that external command. The heuristic report is always
/// produced first, so this only ever *adds* signal.
pub fn run(agent: &dyn Agent, ctx: &RunCtx) -> Report {
    let mut report = agent.run(ctx);
    if let Some(cmd) = ctx.platform.ai_command() {
        match invoke_provider(cmd, &report.to_context(), ctx.arg.as_deref()) {
            Ok(reply) if !reply.trim().is_empty() => report.ai_analysis = Some(reply),
            Ok(_) => {}
            Err(e) => {
                report.ai_analysis =
                    Some(format!("(AI provider `{cmd}` could not be reached: {e})"));
            }
        }
    }
    report
}

/// Pipe `context` (and an optional question) to an external AI CLI on stdin and
/// return its stdout. The command string is split on whitespace into argv.
fn invoke_provider(command: &str, context: &str, arg: Option<&str>) -> std::io::Result<String> {
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

    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        if let Some(q) = arg {
            writeln!(stdin, "Question: {q}\n")?;
        }
        stdin.write_all(context.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_names_are_unique_and_findable() {
        let names: Vec<&str> = registry().iter().map(|a| a.name()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(names.len(), sorted.len());
        assert!(find("maintenance").is_some());
        assert!(find("nope").is_none());
    }

    #[test]
    fn report_markdown_has_sections() {
        let mut r = Report::new("demo", "Demo Report").summary("hi");
        r.findings.push(Finding::warn("watch out"));
        r = r.recommend("do the thing");
        let md = r.to_markdown();
        assert!(md.contains("# Demo Report"));
        assert!(md.contains("**[warn]** watch out"));
        assert!(md.contains("## Recommendation"));
    }
}
