//! Default pipeline for Node projects.

use crate::core::config::Step;

/// `npm install` → `npm run build` → `npm test`.
pub fn default_steps() -> Vec<Step> {
    vec![
        describe(
            Step::command("dependencies", "npm install"),
            "Install npm dependencies",
        ),
        describe(
            Step::command("build", "npm run build"),
            "Run the build script",
        ),
        describe(Step::command("test", "npm test"), "Run the test script"),
    ]
}

fn describe(mut step: Step, desc: &str) -> Step {
    step.description = Some(desc.to_string());
    step
}
