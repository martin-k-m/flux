//! Default pipeline for Python projects.

use crate::core::config::Step;

/// `pip install -r requirements.txt` → byte-compile → `pytest`.
pub fn default_steps() -> Vec<Step> {
    vec![
        describe(
            Step::command("dependencies", "pip install -r requirements.txt"),
            "Install dependencies",
        ),
        describe(
            Step::command("build", "python -m compileall ."),
            "Byte-compile sources",
        ),
        describe(Step::command("test", "pytest"), "Run pytest"),
    ]
}

fn describe(mut step: Step, desc: &str) -> Step {
    step.description = Some(desc.to_string());
    step
}
