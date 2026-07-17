//! Repository Intelligence Engine.
//!
//! Flux inspects a repository and reports what it *is* — languages,
//! dependencies, architecture, git activity — and a deterministic, explainable
//! **health score** built from real signals (does it have tests? CI? docs? how
//! many TODOs?). Nothing here is guessed by an LLM; every number is derived by
//! walking the tree and reading manifests, so `flux project` is reproducible.
//!
//! The output feeds two consumers:
//! * humans, via the `flux project` terminal report; and
//! * machines/AI, via [`crate::knowledge`], which serialises this analysis to
//!   JSON under `.flux-cache/knowledge/`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use crate::fsutil;

pub mod deps;
pub mod git;
pub mod health;

pub use deps::Dependencies;
pub use git::GitStats;
pub use health::HealthScore;

/// A source component — a top-level module/area of the codebase.
#[derive(Debug, Clone)]
pub struct Component {
    pub name: String,
    /// Number of source files in the component.
    pub files: usize,
    /// Other components this one references (heuristic; Rust `use crate::x`).
    pub depends_on: Vec<String>,
}

/// The full result of analysing a repository.
#[derive(Debug, Clone)]
pub struct Intelligence {
    pub project: String,
    pub primary_language: Option<String>,
    /// `(language label, file count)`, most files first.
    pub languages: Vec<(String, usize)>,
    pub file_count: usize,
    pub dependencies: Dependencies,
    pub components: Vec<Component>,
    pub git: GitStats,
    pub health: HealthScore,
}

/// Analyse the repository rooted at `root`.
pub fn analyze(root: &Path) -> Intelligence {
    let detection = crate::core::detect::detect(root);
    let files = fsutil::collect_files(root);

    let languages = language_histogram(&files);
    let primary_language = detection
        .language
        .clone()
        .or_else(|| languages.first().map(|(l, _)| l.clone()));

    let dependencies = deps::analyze(root, primary_language.as_deref());
    let components = infer_components(root);
    let git = git::analyze(root);

    let health = health::score(root, &detection, &git, &dependencies);

    let project = detection
        .name
        .or_else(|| dir_name(root))
        .unwrap_or_else(|| "project".into());

    Intelligence {
        project,
        primary_language,
        languages,
        file_count: files.len(),
        dependencies,
        components,
        git,
        health,
    }
}

/// Count source files per language by extension.
fn language_histogram(files: &[std::path::PathBuf]) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for f in files {
        if let Some(ext) = fsutil::extension(f) {
            if let Some(lang) = language_for_ext(&ext) {
                *counts.entry(lang).or_insert(0) += 1;
            }
        }
    }
    let mut v: Vec<(String, usize)> = counts
        .into_iter()
        .map(|(k, n)| (k.to_string(), n))
        .collect();
    // Most files first; stable by name on ties.
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

/// Map a file extension to a human language label, or `None` for non-source.
fn language_for_ext(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "rs" => "Rust",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kt" => "Kotlin",
        "rb" => "Ruby",
        "c" | "h" => "C",
        "cpp" | "cc" | "hpp" => "C++",
        "cs" => "C#",
        "php" => "PHP",
        "swift" => "Swift",
        "sh" | "bash" => "Shell",
        _ => return None,
    })
}

/// Infer components from top-level source modules. For a Rust project we read
/// `src/` subdirectories; for others we use top-level directories. Edges are a
/// heuristic: Rust `use crate::<name>` references between components.
fn infer_components(root: &Path) -> Vec<Component> {
    let src = root.join("src");
    let base = if src.is_dir() {
        src
    } else {
        root.to_path_buf()
    };

    let mut components: Vec<Component> = Vec::new();
    let entries = match std::fs::read_dir(&base) {
        Ok(e) => e,
        Err(_) => return components,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if path.is_dir() {
            if fsutil::IGNORED_DIRS.contains(&name.as_str()) {
                continue;
            }
            let files = fsutil::collect_files(&path);
            let source_files: Vec<_> = files
                .iter()
                .filter(|f| fsutil::extension(f).as_deref().is_some_and(is_source_ext))
                .collect();
            if source_files.is_empty() {
                continue;
            }
            let depends_on = rust_component_edges(&source_files);
            components.push(Component {
                name,
                files: source_files.len(),
                depends_on,
            });
        }
    }

    // Resolve edges to only reference known components; drop self and unknowns.
    let names: std::collections::HashSet<String> =
        components.iter().map(|c| c.name.clone()).collect();
    for c in &mut components {
        c.depends_on.retain(|d| d != &c.name && names.contains(d));
        c.depends_on.sort();
        c.depends_on.dedup();
    }
    components.sort_by(|a, b| b.files.cmp(&a.files).then_with(|| a.name.cmp(&b.name)));
    components
}

fn is_source_ext(ext: &str) -> bool {
    language_for_ext(ext).is_some()
}

/// Scan Rust files for `use crate::<name>` / `crate::<name>` references to build
/// component dependency edges. Best-effort; empty for non-Rust code.
fn rust_component_edges(files: &[&std::path::PathBuf]) -> Vec<String> {
    let mut edges = Vec::new();
    for f in files {
        if fsutil::extension(f).as_deref() != Some("rs") {
            continue;
        }
        let text = match std::fs::read_to_string(f) {
            Ok(t) => t,
            Err(_) => continue,
        };
        for line in text.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("use crate::") {
                if let Some(name) = rest.split([':', ';', ',', ' ', '{']).next() {
                    if !name.is_empty() {
                        edges.push(name.to_string());
                    }
                }
            }
        }
    }
    edges.sort();
    edges.dedup();
    edges
}

/// Human label for a language id (`rust` → `Rust`). Mirrors the CLI's labels so
/// intelligence output reads the same as the rest of Flux.
pub fn language_display(lang: &str) -> String {
    match lang {
        "rust" => "Rust".into(),
        "node" => "Node".into(),
        "python" => "Python".into(),
        "go" => "Go".into(),
        "java" => "Java".into(),
        other => {
            // Capitalise a bare id we don't have a canonical label for.
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => other.to_string(),
            }
        }
    }
}

fn dir_name(root: &Path) -> Option<String> {
    // Resolve `.` to an absolute path so we can read the directory name.
    let abs = if root.as_os_str() == "." {
        std::env::current_dir().ok()?
    } else {
        root.to_path_buf()
    };
    abs.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
}

/// Is `git` available on PATH? Shared by callers that degrade honestly.
pub(crate) fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_counts_by_language() {
        let files = vec![
            std::path::PathBuf::from("a/main.rs"),
            std::path::PathBuf::from("a/lib.rs"),
            std::path::PathBuf::from("web/app.ts"),
            std::path::PathBuf::from("README.md"),
        ];
        let h = language_histogram(&files);
        assert_eq!(h[0], ("Rust".to_string(), 2));
        assert_eq!(h[1], ("TypeScript".to_string(), 1));
        // Markdown is not a source language.
        assert!(!h.iter().any(|(l, _)| l == "Markdown"));
    }

    #[test]
    fn edges_only_reference_known_components() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("flux-intel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/cli")).unwrap();
        std::fs::create_dir_all(dir.join("src/core")).unwrap();
        std::fs::write(
            dir.join("src/cli/mod.rs"),
            "use crate::core::thing;\nuse crate::missing::x;\nfn f() {}",
        )
        .unwrap();
        std::fs::write(dir.join("src/core/mod.rs"), "pub fn thing() {}").unwrap();

        let comps = infer_components(&dir);
        let cli = comps.iter().find(|c| c.name == "cli").unwrap();
        assert_eq!(cli.depends_on, vec!["core".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
