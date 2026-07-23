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

/// The plugin catalog: `(name, category)`. Installing one today records it
/// locally; a hosted marketplace that fetches real plugin code is future work.
const CATALOG: &[(&str, &str)] = &[
    ("docker", "container"),
    ("kubernetes", "infrastructure"),
    ("terraform", "infrastructure"),
    ("helm", "infrastructure"),
    ("aws", "cloud"),
    ("gcp", "cloud"),
    ("azure", "cloud"),
    ("cloudflare", "cloud"),
    ("npm", "language"),
    ("cargo", "language"),
    ("python", "language"),
];

fn known(name: &str) -> bool {
    CATALOG.iter().any(|(n, _)| *n == name)
}

/// The category reported for the built-in pipelines in [`registry`]. They are
/// all language build pipelines, matching how `CATALOG` labels `npm`/`cargo`.
const BUILTIN_CATEGORY: &str = "language";

/// One plugin-search hit.
///
/// `builtin` marks a plugin that ships inside Flux (see [`registry`]): it is
/// already available and needs no installation. Every other hit comes from
/// `CATALOG` and can be recorded with `flux plugin install`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub name: String,
    pub category: String,
    pub builtin: bool,
}

/// Search the built-in registry *and* the catalog by name or category
/// substring.
///
/// The two lists are separate, so searching only the catalog made the built-in
/// pipelines (`rust`, `node`, `python`) look like they did not exist. Built-ins
/// are listed first and flagged; a catalog entry that is also built in is
/// reported once, as a built-in.
pub fn search(query: &str) -> Vec<SearchHit> {
    let q = query.to_lowercase();
    let mut out: Vec<SearchHit> = Vec::new();

    for plugin in registry() {
        let name = plugin.id().to_string();
        if name.to_lowercase().contains(&q) || BUILTIN_CATEGORY.contains(&q) {
            out.push(SearchHit {
                name,
                category: BUILTIN_CATEGORY.to_string(),
                builtin: true,
            });
        }
    }

    for (name, category) in CATALOG.iter().copied() {
        if !(name.contains(&q) || category.contains(&q)) {
            continue;
        }
        if out.iter().any(|hit| hit.name == name) {
            continue;
        }
        out.push(SearchHit {
            name: name.to_string(),
            category: category.to_string(),
            builtin: false,
        });
    }

    out
}

/// The result of verifying one installed plugin.
pub struct PluginCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// Verify that installed plugin manifests are well-formed.
pub fn verify(root: &std::path::Path) -> Vec<PluginCheck> {
    installed(root)
        .into_iter()
        .map(|name| {
            let path = plugins_dir(root).join(format!("{name}.plugin"));
            let ok = std::fs::read_to_string(&path)
                .map(|t| t.contains("name ="))
                .unwrap_or(false);
            PluginCheck {
                name,
                ok,
                detail: if ok {
                    "manifest ok".into()
                } else {
                    "malformed or missing manifest".into()
                },
            }
        })
        .collect()
}

fn plugins_dir(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".flux-cache").join("plugins")
}

/// Record a plugin as installed for this project. Returns an error for a name
/// the marketplace doesn't recognise (so typos fail loudly).
pub fn install(root: &std::path::Path, name: &str) -> anyhow::Result<()> {
    if !known(name) {
        let names: Vec<&str> = CATALOG.iter().map(|(n, _)| *n).collect();
        anyhow::bail!(
            "unknown plugin '{name}'. Known plugins: {}",
            names.join(", ")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn names(query: &str) -> Vec<String> {
        search(query).into_iter().map(|h| h.name).collect()
    }

    /// Every built-in from `registry()` must be findable by `plugin search`.
    /// `rust` and `node` are not in `CATALOG`, so a catalog-only search
    /// reported "no matches" for plugins Flux actually ships.
    #[test]
    fn search_finds_builtin_plugins() {
        for builtin in registry() {
            let id = builtin.id();
            let hits = search(id);
            let hit = hits
                .iter()
                .find(|h| h.name == id)
                .unwrap_or_else(|| panic!("built-in '{id}' not found by search"));
            assert!(hit.builtin, "built-in '{id}' should be flagged as built-in");
        }
    }

    #[test]
    fn search_still_finds_catalog_entries() {
        assert!(names("docker").contains(&"docker".to_string()));
        // Category matches keep working.
        assert!(names("cloud").contains(&"aws".to_string()));
    }

    /// `python` is both a built-in and a catalog entry; report it once.
    #[test]
    fn search_does_not_duplicate_builtin_and_catalog_entries() {
        let hits = search("python");
        let python: Vec<_> = hits.iter().filter(|h| h.name == "python").collect();
        assert_eq!(python.len(), 1, "expected one 'python' hit, got {python:?}");
        assert!(python[0].builtin);
    }

    #[test]
    fn search_excludes_unrelated_plugins() {
        assert!(!names("docker").contains(&"terraform".to_string()));
        assert!(search("definitely-not-a-plugin").is_empty());
    }

    /// Built-ins are listed before installable catalog entries.
    #[test]
    fn builtins_are_listed_first() {
        let hits = search("language");
        let first_catalog = hits.iter().position(|h| !h.builtin);
        let last_builtin = hits.iter().rposition(|h| h.builtin);
        if let (Some(first_catalog), Some(last_builtin)) = (first_catalog, last_builtin) {
            assert!(last_builtin < first_catalog, "built-ins should sort first");
        }
    }
}
