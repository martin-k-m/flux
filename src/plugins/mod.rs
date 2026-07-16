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

/// Plugin Development Kit (4.19): scaffold a new plugin project.
///
/// Generates `plugins/<name>/` with a manifest, source, tests, and a README so
/// the ecosystem has a stable starting point.
pub fn create(root: &std::path::Path, name: &str) -> anyhow::Result<std::path::PathBuf> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!("invalid plugin name '{name}' (use letters, digits, '-' or '_')");
    }
    let dir = root.join("plugins").join(name);
    if dir.exists() {
        anyhow::bail!("plugins/{name} already exists");
    }
    std::fs::create_dir_all(dir.join("src"))?;
    std::fs::create_dir_all(dir.join("tests"))?;

    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            "name = \"{name}\"\nversion = \"0.1.0\"\nauthor = \"you\"\ncapabilities = []\n\n\
             # Declare what this plugin can do, e.g.:\n\
             # capabilities = [\"container-build\", \"image-push\"]\n"
        ),
    )?;
    std::fs::write(
        dir.join("src").join("plugin.rs"),
        "// Implement the Flux plugin contract.\n\
         //\n\
         // trait FluxPlugin {\n\
         //     fn execute(&self, ctx: &Context) -> Result<()>;\n\
         //     fn validate(&self) -> Result<()>;\n\
         // }\n",
    )?;
    std::fs::write(
        dir.join("tests").join("plugin_test.rs"),
        "// Add plugin tests here.\n",
    )?;
    std::fs::write(
        dir.join("README.md"),
        format!("# {name}\n\nA Flux plugin. Describe what it does and how to configure it.\n"),
    )?;
    Ok(dir)
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
