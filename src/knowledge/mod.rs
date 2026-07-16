//! The project knowledge graph.
//!
//! Flux serialises its [repository intelligence][crate::intel] into a small set
//! of JSON files under `.flux-cache/knowledge/`. This is the "AI-legible" layer:
//! an external agent (or a human) can read a stable, structured description of
//! the project without re-deriving it. Flux itself hosts no model — it just
//! writes the ground truth down.
//!
//! ```text
//! .flux-cache/knowledge/
//!   architecture.json   components + dependency edges
//!   dependencies.json   declared direct dependencies
//!   patterns.json       detected conventions (tests, CI, layout)
//!   history.json        git activity
//!   decisions.json      an append-only decision log (seeded empty)
//! ```

pub mod json;

use std::path::{Path, PathBuf};

use crate::intel::Intelligence;
use json::Json;

/// The directory that holds the knowledge graph for `root`.
pub fn dir(root: &Path) -> PathBuf {
    root.join(".flux-cache").join("knowledge")
}

/// Build the knowledge graph from an analysis and write it to disk. Returns the
/// files written. `decisions.json` is only *seeded* (never overwritten) so an
/// AI or human can append to it.
pub fn build(root: &Path, intel: &Intelligence) -> std::io::Result<Vec<PathBuf>> {
    let dir = dir(root);
    std::fs::create_dir_all(&dir)?;

    let mut written = Vec::new();
    let mut write = |name: &str, value: Json| -> std::io::Result<()> {
        let path = dir.join(name);
        std::fs::write(&path, value.pretty())?;
        written.push(path);
        Ok(())
    };

    write("architecture.json", architecture(intel))?;
    write("dependencies.json", dependencies(intel))?;
    write("patterns.json", patterns(intel))?;
    write("history.json", history(intel))?;

    // Seed the decisions log once; never clobber an existing one.
    let decisions_path = dir.join("decisions.json");
    if !decisions_path.exists() {
        let seed = Json::Object(vec![
            (
                "note".into(),
                Json::s(
                    "Append architecture decisions here as { title, date, context, decision }.",
                ),
            ),
            ("decisions".into(), Json::Array(vec![])),
        ]);
        std::fs::write(&decisions_path, seed.pretty())?;
        written.push(decisions_path);
    }

    Ok(written)
}

fn architecture(intel: &Intelligence) -> Json {
    let components = intel.components.iter().map(|c| {
        Json::Object(vec![
            ("name".into(), Json::s(&c.name)),
            ("files".into(), Json::Num(c.files as i64)),
            (
                "dependsOn".into(),
                Json::array(c.depends_on.iter().map(Json::s)),
            ),
        ])
    });

    Json::Object(vec![
        ("project".into(), Json::s(&intel.project)),
        (
            "primaryLanguage".into(),
            match &intel.primary_language {
                Some(l) => Json::s(crate::intel::language_display(l)),
                None => Json::Null,
            },
        ),
        ("components".into(), Json::array(components)),
    ])
}

fn dependencies(intel: &Intelligence) -> Json {
    Json::Object(vec![
        ("total".into(), Json::Num(intel.dependencies.total as i64)),
        (
            "source".into(),
            match &intel.dependencies.source {
                Some(s) => Json::s(s),
                None => Json::Null,
            },
        ),
        ("locked".into(), Json::Bool(intel.dependencies.locked)),
        (
            "names".into(),
            Json::array(intel.dependencies.names.iter().map(Json::s)),
        ),
    ])
}

fn patterns(intel: &Intelligence) -> Json {
    let languages = intel.languages.iter().map(|(lang, n)| {
        Json::Object(vec![
            ("language".into(), Json::s(lang)),
            ("files".into(), Json::Num(*n as i64)),
        ])
    });

    let signals = intel.health.signals.iter().map(|s| {
        Json::Object(vec![
            ("name".into(), Json::s(&s.name)),
            ("present".into(), Json::Bool(s.ok)),
        ])
    });

    Json::Object(vec![
        ("fileCount".into(), Json::Num(intel.file_count as i64)),
        ("languages".into(), Json::array(languages)),
        ("signals".into(), Json::array(signals)),
    ])
}

fn history(intel: &Intelligence) -> Json {
    let g = &intel.git;
    Json::Object(vec![
        ("isRepo".into(), Json::Bool(g.is_repo)),
        ("commits".into(), Json::Num(g.commits as i64)),
        ("contributors".into(), Json::Num(g.contributors as i64)),
        (
            "branch".into(),
            match &g.branch {
                Some(b) => Json::s(b),
                None => Json::Null,
            },
        ),
        (
            "lastCommit".into(),
            match &g.last_commit {
                Some(d) => Json::s(d),
                None => Json::Null,
            },
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_writes_graph_files() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("flux-knowledge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"kg\"\n").unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();

        let intel = crate::intel::analyze(&dir);
        let written = build(&dir, &intel).unwrap();
        assert_eq!(written.len(), 5);
        let arch = std::fs::read_to_string(super::dir(&dir).join("architecture.json")).unwrap();
        assert!(arch.contains("\"project\""));
        assert!(arch.contains("\"components\""));

        // decisions.json is not clobbered on a second run.
        std::fs::write(super::dir(&dir).join("decisions.json"), "CUSTOM").unwrap();
        let intel2 = crate::intel::analyze(&dir);
        build(&dir, &intel2).unwrap();
        let decisions = std::fs::read_to_string(super::dir(&dir).join("decisions.json")).unwrap();
        assert_eq!(decisions, "CUSTOM");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
