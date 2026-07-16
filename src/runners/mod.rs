//! Runners.
//!
//! The **shell runner** ([`shell`]) is the low-level command executor every
//! step ultimately uses. The language runners ([`rust`], [`node`], [`python`])
//! describe the *default* pipeline for a project when no explicit step list is
//! given in `.flux`. The plugin layer ([`crate::plugins`]) registers these so
//! new languages can be added without touching the engine.

pub mod containers;
pub mod node;
pub mod python;
pub mod rust;
pub mod shell;

use crate::core::config::Step;

/// The set of default steps a language runner contributes.
pub fn default_steps(language: &str) -> Option<Vec<Step>> {
    match language {
        "rust" => Some(rust::default_steps()),
        "node" => Some(node::default_steps()),
        "python" => Some(python::default_steps()),
        _ => None,
    }
}
