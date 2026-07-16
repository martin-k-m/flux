//! Default pipeline for Rust projects.

use crate::core::config::Step;

/// `cargo fetch` → `cargo build --release` → `cargo test`.
pub fn default_steps() -> Vec<Step> {
    vec![
        describe(
            Step::command("dependencies", "cargo fetch"),
            "Fetch crate dependencies",
        ),
        describe(
            Step::command("build", "cargo build --release"),
            "Compile in release mode",
        ),
        describe(Step::command("test", "cargo test"), "Run the test suite"),
    ]
}

fn describe(mut step: Step, desc: &str) -> Step {
    step.description = Some(desc.to_string());
    step
}
