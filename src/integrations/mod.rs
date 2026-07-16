//! Native Blink & Killer integration (Phase 4, 4.18).
//!
//! Flux recognises its sibling tools automatically, with no manual wiring:
//!
//! * **Blink** creates and manages projects. If Blink metadata is present, Flux
//!   notes the project profile.
//! * **Killer** provides security analysis. If a Killer config is present, Flux
//!   runs a security step automatically after the build.
//!
//! ```text
//! Project → Blink → Flux → Killer
//! ```

use std::path::{Path, PathBuf};

use crate::core::config::Step;

/// Detected sibling-tool configuration.
#[derive(Debug, Clone, Default)]
pub struct Siblings {
    /// Blink metadata file, if present.
    pub blink: Option<PathBuf>,
    /// Killer config file, if present.
    pub killer: Option<PathBuf>,
}

impl Siblings {
    pub fn has_blink(&self) -> bool {
        self.blink.is_some()
    }
    pub fn has_killer(&self) -> bool {
        self.killer.is_some()
    }
}

const BLINK_MARKERS: &[&str] = &[".bnk", ".blink", "blink.toml"];
const KILLER_MARKERS: &[&str] = &[".killer", "killer.toml"];

/// Detect sibling-tool config files in `root`.
pub fn detect(root: &Path) -> Siblings {
    Siblings {
        blink: first_present(root, BLINK_MARKERS),
        killer: first_present(root, KILLER_MARKERS),
    }
}

fn first_present(root: &Path, markers: &[&str]) -> Option<PathBuf> {
    markers.iter().map(|m| root.join(m)).find(|p| p.is_file())
}

/// If Killer is configured but the pipeline has no security step, append one so
/// a scan runs automatically. Returns `true` when a step was added.
pub fn inject_killer(steps: &mut Vec<Step>, siblings: &Siblings) -> bool {
    if !siblings.has_killer() {
        return false;
    }
    let has_security = steps
        .iter()
        .any(|s| s.tool.as_deref() == Some("killer") || s.name.contains("security"));
    if has_security {
        return false;
    }

    // Depend on the last step so the scan runs after the build/tests.
    let mut security = Step::new("security");
    security.tool = Some("killer".into());
    if let Some(last) = steps.last() {
        security.needs = vec![last.name.clone()];
    }
    steps.push(security);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_killer_step_when_configured() {
        let siblings = Siblings {
            blink: None,
            killer: Some(PathBuf::from(".killer")),
        };
        let mut steps = vec![
            Step::command("build", "cargo build"),
            Step::command("test", "cargo test"),
        ];
        let added = inject_killer(&mut steps, &siblings);
        assert!(added);
        let sec = steps.last().unwrap();
        assert_eq!(sec.tool.as_deref(), Some("killer"));
        assert_eq!(sec.needs, vec!["test"]);
    }

    #[test]
    fn does_not_inject_when_already_present() {
        let siblings = Siblings {
            blink: None,
            killer: Some(PathBuf::from(".killer")),
        };
        let mut sec = Step::new("security");
        sec.tool = Some("killer".into());
        let mut steps = vec![sec];
        assert!(!inject_killer(&mut steps, &siblings));
        assert_eq!(steps.len(), 1);
    }

    #[test]
    fn no_injection_without_killer_config() {
        let siblings = Siblings::default();
        let mut steps = vec![Step::command("build", "cargo build")];
        assert!(!inject_killer(&mut steps, &siblings));
    }
}
