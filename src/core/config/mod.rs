//! The `.flux` configuration language.
//!
//! Flux ships its own small declarative language so a project doesn't need
//! `package.json` scripts, a `Makefile`, and a pile of shell scripts to
//! describe how it builds. A `.flux` file is the single source of truth.
//!
//! Phase 3 adds **modules**: a pipeline can `use` a reusable module from a
//! sibling `modules/` directory, and those steps are spliced in at load time.

mod ast;
mod parser;

pub use ast::{Deployment, FluxConfig, Step};
pub use parser::parse;

use std::collections::HashSet;
use std::path::Path;

/// The conventional config filename.
pub const CONFIG_FILE: &str = ".flux";

/// Read and parse a `.flux` file from disk, resolving any `use` modules.
pub fn load(path: &Path) -> anyhow::Result<FluxConfig> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))?;
    let mut cfg = parse(&src).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;

    let base = path.parent().unwrap_or_else(|| Path::new("."));
    resolve_modules(&mut cfg, base)?;
    Ok(cfg)
}

/// Expand `use <module>` directives by splicing module steps ahead of the
/// pipeline's own steps. Explicit steps win on name collisions.
fn resolve_modules(cfg: &mut FluxConfig, base: &Path) -> anyhow::Result<()> {
    if cfg.uses.is_empty() {
        return Ok(());
    }

    let mut module_steps: Vec<Step> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    for name in cfg.uses.clone() {
        expand_module(&name, base, &mut module_steps, &mut visited)?;
    }

    let explicit: HashSet<String> = cfg.steps.iter().map(|s| s.name.clone()).collect();
    let mut merged: Vec<Step> = Vec::new();
    for s in module_steps {
        if !explicit.contains(&s.name) && !merged.iter().any(|m| m.name == s.name) {
            merged.push(s);
        }
    }
    merged.append(&mut cfg.steps);
    cfg.steps = merged;
    Ok(())
}

/// Load `modules/<name>.flux` and collect its steps (recursively expanding any
/// modules it itself uses). `visited` guards against cycles and duplicates.
fn expand_module(
    name: &str,
    base: &Path,
    out: &mut Vec<Step>,
    visited: &mut HashSet<String>,
) -> anyhow::Result<()> {
    if !visited.insert(name.to_string()) {
        return Ok(());
    }
    let path = base.join("modules").join(format!("{name}.flux"));
    let src = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("could not load module '{name}' ({}): {e}", path.display()))?;
    let modcfg = parse(&src).map_err(|e| anyhow::anyhow!("module '{name}': {e}"))?;

    for nested in &modcfg.uses {
        expand_module(nested, base, out, visited)?;
    }
    out.extend(modcfg.steps);
    Ok(())
}
