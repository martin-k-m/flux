//! Flux — a local-first software automation engine.
//!
//! Give Flux a project and it knows how to build, test, and package it
//! consistently. This binary wires the CLI to the core engine.

mod agent;
mod analytics;
mod artifacts;
mod assist;
mod cache;
mod cli;
mod core;
mod deploy;
mod plugins;
mod repro;
mod runners;
mod secrets;

/// Human-facing version label (`flux --version` still reports the full semver).
pub const VERSION_LABEL: &str = "v0.1";

fn main() {
    // The CLI layer owns all user-facing error reporting and returns the
    // process exit code so `main` stays a thin shell.
    std::process::exit(cli::run());
}
