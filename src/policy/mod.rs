//! Flux Policy Engine (Phase 4, 4.15).
//!
//! Policies declare organization-wide rules a pipeline must satisfy before it
//! ships — e.g. tests must exist, a security scan must run, and a number of
//! approvals must be present. Pipelines that violate policy are stopped.
//!
//! ```text
//! policy production {
//!     require tests
//!     require security
//!     require approvals 2
//! }
//! ```
//!
//! Approvals are supplied out-of-band (the `FLUX_APPROVALS` environment
//! variable), since Flux itself has no identity system — that's the honest
//! boundary of what a local tool can enforce.

use crate::core::config::FluxConfig;

/// A single policy violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub policy: String,
    pub message: String,
}

/// Evaluate all policies in `config` against its pipeline and the supplied
/// `approvals` count. Returns every violation (empty when compliant).
pub fn evaluate(config: &FluxConfig, approvals: u32) -> Vec<Violation> {
    let has_tests = config.steps.iter().any(|s| s.name.contains("test"));
    let has_security = config
        .steps
        .iter()
        .any(|s| s.tool.as_deref() == Some("killer") || s.name.contains("security"));

    let mut violations = Vec::new();
    for policy in &config.policies {
        if policy.require_tests && !has_tests {
            violations.push(Violation {
                policy: policy.name.clone(),
                message: "requires a test step, but the pipeline has none".into(),
            });
        }
        if policy.require_security && !has_security {
            violations.push(Violation {
                policy: policy.name.clone(),
                message: "requires a security step (e.g. `tool killer`), but none is present"
                    .into(),
            });
        }
        if approvals < policy.require_approvals {
            violations.push(Violation {
                policy: policy.name.clone(),
                message: format!(
                    "requires {} approval(s), but {approvals} provided (set FLUX_APPROVALS)",
                    policy.require_approvals
                ),
            });
        }
    }
    violations
}

/// Read the approvals count from the environment (`FLUX_APPROVALS`).
pub fn approvals_from_env() -> u32 {
    std::env::var("FLUX_APPROVALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::parse;

    const POLICY: &str = "policy prod { require tests require security require approvals 2 }\n";

    #[test]
    fn passes_when_requirements_met() {
        let src = format!(
            "{POLICY}pipeline {{ step test {{ command \"cargo test\" }} step security {{ tool killer }} }}"
        );
        let config = parse(&src).unwrap();
        assert!(evaluate(&config, 2).is_empty());
    }

    #[test]
    fn flags_missing_tests_security_and_approvals() {
        let src = format!("{POLICY}pipeline {{ step build {{ command \"cargo build\" }} }}");
        let config = parse(&src).unwrap();
        let violations = evaluate(&config, 0);
        assert_eq!(violations.len(), 3);
    }
}
