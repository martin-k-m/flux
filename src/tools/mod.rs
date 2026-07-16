//! Flux first-party tool suite (Phase 4, 4.5).
//!
//! Language-aware developer commands that Flux ships out of the box: `fmt`,
//! `lint`, `changelog`, `version`, `deps`, and `doctor`. Each degrades honestly
//! when the underlying tool isn't installed rather than pretending to succeed.

pub mod changelog;
pub mod deps;
pub mod doctor;
pub mod version;

use std::path::Path;

use crate::runners::shell;

/// The formatter command for a language, if we know one.
fn formatter(language: &str) -> Option<&'static str> {
    match language {
        "rust" => Some("cargo fmt"),
        "node" => Some("npx --yes prettier --write ."),
        "python" => Some("black ."),
        "go" => Some("gofmt -w ."),
        _ => None,
    }
}

/// The linter command for a language, if we know one.
fn linter(language: &str) -> Option<&'static str> {
    match language {
        "rust" => Some("cargo clippy"),
        "node" => Some("npx --yes eslint ."),
        "python" => Some("ruff check ."),
        "go" => Some("go vet ./..."),
        _ => None,
    }
}

/// The outcome of running a first-party tool.
pub enum ToolOutcome {
    Ran { success: bool },
    NoCommand,
}

/// Run the language formatter.
pub fn fmt(root: &Path, language: &str) -> ToolOutcome {
    run_tool(root, formatter(language))
}

/// Run the language linter.
pub fn lint(root: &Path, language: &str) -> ToolOutcome {
    run_tool(root, linter(language))
}

fn run_tool(root: &Path, command: Option<&str>) -> ToolOutcome {
    match command {
        Some(cmd) => match shell::run(cmd, root) {
            Ok(r) => ToolOutcome::Ran { success: r.success },
            Err(_) => ToolOutcome::NoCommand,
        },
        None => ToolOutcome::NoCommand,
    }
}

/// The command a tool would run (for display).
pub fn fmt_command(language: &str) -> Option<&'static str> {
    formatter(language)
}

/// The command a tool would run (for display).
pub fn lint_command(language: &str) -> Option<&'static str> {
    linter(language)
}
