//! Flux — a local-first software automation engine.
//!
//! Give Flux a project and it knows how to build, test, and package it
//! consistently. This binary wires the CLI to the core engine.

mod agent;
mod agents;
mod analytics;
mod artifacts;
mod ask;
mod assist;
mod cache;
mod cli;
mod core;
mod dashboard;
mod deploy;
mod docs_engine;
mod fsutil;
mod github;
mod intel;
mod knowledge;
mod platform;
mod plugins;
mod policy;
mod repro;
mod runners;
mod secrets;
mod tools;
mod workspace;

/// Human-facing version label (`flux --version` still reports the full semver).
pub const VERSION_LABEL: &str = "v0.2";

fn main() {
    // The CLI layer owns all user-facing error reporting and returns the
    // process exit code so `main` stays a thin shell.
    std::process::exit(cli::run());
}
