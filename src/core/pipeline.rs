//! Pipeline resolution.
//!
//! A [`Pipeline`] is the resolved project metadata plus the ordered step list,
//! built from an explicit `.flux` file or, when steps are omitted, from the
//! language plugin's defaults. *Execution* is the job of the graph engine
//! ([`crate::core::graph`]), which turns these steps into a dependency graph.

use crate::core::config::{FluxConfig, Step};
use crate::core::detect::Detection;

/// A resolved build pipeline (metadata + steps, not yet a graph).
#[derive(Debug, Clone)]
pub struct Pipeline {
    pub project: String,
    pub language: String,
    pub steps: Vec<Step>,
}

impl Pipeline {
    /// Resolve a pipeline from a parsed config, falling back to detected
    /// defaults for anything the config leaves unspecified.
    pub fn resolve(config: &FluxConfig, detection: &Detection) -> Pipeline {
        let language = config
            .language
            .clone()
            .or_else(|| detection.language.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let project = config
            .project
            .clone()
            .or_else(|| detection.name.clone())
            .unwrap_or_else(|| "project".to_string());

        // When the config omits steps, fall back to the language plugin's
        // default pipeline. Routing through the plugin registry keeps the
        // plugin layer the single source of truth for a language's build.
        let steps = if config.steps.is_empty() {
            crate::plugins::for_language(&language)
                .map(|p| p.default_steps())
                .unwrap_or_default()
        } else {
            config.steps.clone()
        };

        Pipeline {
            project,
            language,
            steps,
        }
    }
}
