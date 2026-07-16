//! The Flux platform config (`flux.yaml`).
//!
//! Flux's *pipeline* lives in the `.flux` file (the build language). The
//! *platform* layer — repository intelligence, AI agents, GitHub integration —
//! is configured separately in a committed `flux.yaml` at the project root.
//!
//! We can't reuse `.flux` for this (that name is a file, and the spec's
//! `.flux/` directory would collide), and we deliberately avoid a YAML crate to
//! stay `windows-sys`-free (see CLAUDE.md). So this module hand-rolls a tiny
//! parser for the *flat, two-level* subset of YAML we actually emit and read:
//!
//! ```yaml
//! project:
//!   name: my-app
//! agents:
//!   enabled: true
//! ai:
//!   provider: none        # none | external
//!   command: ""           # e.g. "ollama run llama3" — Flux pipes context on stdin
//! github:
//!   enabled: true
//! deployment:
//!   enabled: false
//! integrations:
//!   github: true
//! ```
//!
//! Anything more elaborate (anchors, lists, nested maps) is out of scope on
//! purpose: this file is Flux's, and Flux keeps it simple.

use std::path::{Path, PathBuf};

/// The conventional platform-config filename.
pub const PLATFORM_FILE: &str = "flux.yaml";

/// The committed directory that holds authored platform assets (agent
/// definitions, rules, shared memory). Named `.flux.d/` because `.flux` is the
/// pipeline file and can't also be a directory.
pub const PLATFORM_DIR: &str = ".flux.d";

/// How the AI features source their intelligence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiProvider {
    /// No LLM configured — agents and `ask` fall back to honest heuristics.
    None,
    /// Pipe assembled context to an external CLI (`ai.command`) on stdin.
    External,
}

impl AiProvider {
    fn parse(s: &str) -> AiProvider {
        match s.trim() {
            "external" => AiProvider::External,
            _ => AiProvider::None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            AiProvider::None => "none",
            AiProvider::External => "external",
        }
    }
}

/// The parsed `flux.yaml`.
#[derive(Debug, Clone)]
pub struct PlatformConfig {
    pub project_name: Option<String>,
    pub pipeline_auto_detect: bool,
    pub agents_enabled: bool,
    pub ai_provider: AiProvider,
    /// The external command to pipe context into (only meaningful when
    /// `ai_provider == External`). e.g. `"ollama run llama3"`.
    pub ai_command: Option<String>,
    pub github_enabled: bool,
    pub deployment_enabled: bool,
}

impl Default for PlatformConfig {
    fn default() -> Self {
        PlatformConfig {
            project_name: None,
            pipeline_auto_detect: true,
            agents_enabled: true,
            ai_provider: AiProvider::None,
            ai_command: None,
            github_enabled: true,
            deployment_enabled: false,
        }
    }
}

impl PlatformConfig {
    /// Load `flux.yaml` from `root`, or the defaults if it doesn't exist.
    pub fn load(root: &Path) -> PlatformConfig {
        let path = root.join(PLATFORM_FILE);
        match std::fs::read_to_string(&path) {
            Ok(src) => parse(&src),
            Err(_) => PlatformConfig::default(),
        }
    }

    /// Does a `flux.yaml` exist at `root`?
    pub fn exists(root: &Path) -> bool {
        root.join(PLATFORM_FILE).is_file()
    }

    /// Is an external AI provider configured *and* usable (has a command)?
    pub fn ai_command(&self) -> Option<&str> {
        match self.ai_provider {
            AiProvider::External => self.ai_command.as_deref().filter(|c| !c.is_empty()),
            AiProvider::None => None,
        }
    }

    /// Render back to canonical `flux.yaml` text.
    pub fn render(&self) -> String {
        let name = self.project_name.as_deref().unwrap_or("my-app");
        let cmd = self.ai_command.as_deref().unwrap_or("");
        format!(
            "project:\n  name: {name}\n\
             pipeline:\n  autoDetect: {}\n\
             agents:\n  enabled: {}\n\
             ai:\n  provider: {}\n  command: \"{cmd}\"\n\
             github:\n  enabled: {}\n\
             deployment:\n  enabled: {}\n\
             integrations:\n  github: {}\n",
            self.pipeline_auto_detect,
            self.agents_enabled,
            self.ai_provider.as_str(),
            self.github_enabled,
            self.deployment_enabled,
            self.github_enabled,
        )
    }

    /// The path to the authored platform-assets directory.
    pub fn dir(root: &Path) -> PathBuf {
        root.join(PLATFORM_DIR)
    }
}

/// Parse the flat two-level YAML subset. Unknown keys are ignored; malformed
/// lines are skipped rather than erroring — this config is advisory.
fn parse(src: &str) -> PlatformConfig {
    let mut cfg = PlatformConfig::default();
    let mut section = String::new();

    for raw in src.lines() {
        // Strip comments (we don't support `#` inside quoted values, which is
        // fine for the values we emit).
        let line = strip_comment(raw);
        if line.trim().is_empty() {
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();

        if indent == 0 {
            // A new top-level section: `project:`.
            section = trimmed.trim_end_matches(':').trim().to_string();
            continue;
        }

        // A `key: value` pair within the current section.
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = unquote(value.trim());

        match (section.as_str(), key) {
            ("project", "name") => cfg.project_name = Some(value.to_string()),
            ("pipeline", "autoDetect") => cfg.pipeline_auto_detect = truthy(&value),
            ("agents", "enabled") => cfg.agents_enabled = truthy(&value),
            ("ai", "provider") => cfg.ai_provider = AiProvider::parse(&value),
            ("ai", "command") => {
                cfg.ai_command = if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                }
            }
            ("github", "enabled") | ("integrations", "github") => {
                cfg.github_enabled = truthy(&value)
            }
            ("deployment", "enabled") => cfg.deployment_enabled = truthy(&value),
            _ => {}
        }
    }
    cfg
}

fn strip_comment(line: &str) -> String {
    // Only strip `#` that isn't inside quotes. Our emitted values never contain
    // `#`, so a simple in-quote tracker is enough.
    let mut out = String::new();
    let mut in_quote = false;
    for c in line.chars() {
        match c {
            '"' => {
                in_quote = !in_quote;
                out.push(c);
            }
            '#' if !in_quote => break,
            _ => out.push(c),
        }
    }
    out
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn truthy(s: &str) -> bool {
    matches!(s.trim(), "true" | "yes" | "on" | "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let c = PlatformConfig::default();
        assert!(c.pipeline_auto_detect);
        assert!(c.agents_enabled);
        assert_eq!(c.ai_provider, AiProvider::None);
        assert!(c.ai_command().is_none());
    }

    #[test]
    fn parses_a_full_config() {
        let src = "\
project:\n  name: demo\n\
pipeline:\n  autoDetect: false\n\
agents:\n  enabled: true\n\
ai:\n  provider: external\n  command: \"ollama run llama3\"\n\
github:\n  enabled: true\n\
deployment:\n  enabled: true\n";
        let c = parse(src);
        assert_eq!(c.project_name.as_deref(), Some("demo"));
        assert!(!c.pipeline_auto_detect);
        assert_eq!(c.ai_provider, AiProvider::External);
        assert_eq!(c.ai_command(), Some("ollama run llama3"));
        assert!(c.deployment_enabled);
    }

    #[test]
    fn external_provider_without_command_is_not_usable() {
        let src = "ai:\n  provider: external\n  command: \"\"\n";
        let c = parse(src);
        assert_eq!(c.ai_provider, AiProvider::External);
        assert!(c.ai_command().is_none());
    }

    #[test]
    fn comments_and_blanks_are_ignored() {
        let src = "# top\nproject:\n  name: x   # inline\n\nagents:\n  enabled: no\n";
        let c = parse(src);
        assert_eq!(c.project_name.as_deref(), Some("x"));
        assert!(!c.agents_enabled);
    }

    #[test]
    fn render_round_trips() {
        let c = PlatformConfig {
            project_name: Some("round".into()),
            ai_provider: AiProvider::External,
            ai_command: Some("claude -p".into()),
            ..PlatformConfig::default()
        };
        let reparsed = parse(&c.render());
        assert_eq!(reparsed.project_name.as_deref(), Some("round"));
        assert_eq!(reparsed.ai_command(), Some("claude -p"));
    }
}
