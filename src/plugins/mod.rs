//! Plugin system foundation.
//!
//! Phase 1 does not ship a dynamic plugin loader — it establishes the *shape*
//! so future work (`flux plugin install docker`, `... kubernetes`) can slot in
//! without reworking the engine. A [`Plugin`] describes a language or tool
//! integration: how to detect it and what its default pipeline looks like.
//!
//! Today the built-in plugins (`rust`, `node`, `python`) simply wrap the
//! language runners. Tomorrow, third-party plugins living under `plugins/`
//! (e.g. `rust.plugin`, `docker.plugin`) would register through this same
//! interface.

use crate::core::config::Step;
use crate::runners;

/// A registered Flux plugin.
pub trait Plugin {
    /// The plugin id, e.g. `rust`.
    fn id(&self) -> &str;
    /// A one-line human description.
    fn description(&self) -> &str;
    /// Marker files that indicate this plugin applies (for detection/display).
    fn markers(&self) -> &[&str];
    /// The default pipeline steps this plugin contributes.
    fn default_steps(&self) -> Vec<Step>;
    /// Whether the plugin is built in (vs. installed by the user).
    fn builtin(&self) -> bool {
        true
    }
}

/// A built-in language plugin backed by a runner's default steps.
struct LanguagePlugin {
    id: &'static str,
    description: &'static str,
    markers: &'static [&'static str],
}

impl Plugin for LanguagePlugin {
    fn id(&self) -> &str {
        self.id
    }
    fn description(&self) -> &str {
        self.description
    }
    fn markers(&self) -> &[&str] {
        self.markers
    }
    fn default_steps(&self) -> Vec<Step> {
        runners::default_steps(self.id).unwrap_or_default()
    }
}

/// All plugins currently known to Flux.
pub fn registry() -> Vec<Box<dyn Plugin>> {
    vec![
        Box::new(LanguagePlugin {
            id: "rust",
            description: "Rust / Cargo build pipeline",
            markers: &["Cargo.toml"],
        }),
        Box::new(LanguagePlugin {
            id: "node",
            description: "Node / npm build pipeline",
            markers: &["package.json"],
        }),
        Box::new(LanguagePlugin {
            id: "python",
            description: "Python build pipeline",
            markers: &["requirements.txt", "pyproject.toml"],
        }),
    ]
}

/// Look up the plugin responsible for `language`.
pub fn for_language(language: &str) -> Option<Box<dyn Plugin>> {
    registry().into_iter().find(|p| p.id() == language)
}

/// Plugin names known to the (future) plugin marketplace. Installing one today
/// records it locally; a later phase will fetch and load real behaviour.
const KNOWN: &[&str] = &[
    "aws",
    "docker",
    "kubernetes",
    "terraform",
    "npm",
    "cargo",
    "python",
    "gcp",
    "azure",
];

fn plugins_dir(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".flux-cache").join("plugins")
}

/// Record a plugin as installed for this project. Returns an error for a name
/// the marketplace doesn't recognise (so typos fail loudly).
pub fn install(root: &std::path::Path, name: &str) -> anyhow::Result<()> {
    if !KNOWN.contains(&name) {
        anyhow::bail!(
            "unknown plugin '{name}'. Known plugins: {}",
            KNOWN.join(", ")
        );
    }
    let dir = plugins_dir(root);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join(format!("{name}.plugin")),
        format!("name = {name}\nstatus = installed\n"),
    )?;
    Ok(())
}

/// List locally-installed plugin names.
pub fn installed(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(plugins_dir(root)) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("plugin") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_string());
                }
            }
        }
    }
    out.sort();
    out
}
