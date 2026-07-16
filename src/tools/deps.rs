//! `flux deps` — inspect project dependencies (4.5).
//!
//! Reports the dependency count and any duplicates. "Outdated" detection needs
//! registry access (network), so it is reported as unavailable rather than
//! guessed at — see [`DepsReport::outdated_note`].

use std::path::Path;

/// A dependency inspection report.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DepsReport {
    pub total: usize,
    /// Dependencies that appear in more than one place (e.g. deps + devDeps).
    pub duplicates: Vec<String>,
}

impl DepsReport {
    /// Why "outdated" counts aren't shown offline.
    pub fn outdated_note(&self) -> &'static str {
        "outdated/unused analysis needs registry access (offline)"
    }
}

/// Inspect dependencies for `language` under `root`.
pub fn inspect(root: &Path, language: &str) -> anyhow::Result<DepsReport> {
    match language {
        "rust" => inspect_cargo(&root.join("Cargo.toml")),
        "node" => inspect_package_json(&root.join("package.json")),
        "python" => inspect_requirements(&root.join("requirements.txt")),
        other => anyhow::bail!("dependency inspection isn't supported for '{other}' yet"),
    }
}

/// Count entries under `[dependencies]` (and dev/build) in a Cargo.toml.
fn inspect_cargo(path: &Path) -> anyhow::Result<DepsReport> {
    let text = std::fs::read_to_string(path)?;
    let mut in_deps = false;
    let mut seen: Vec<String> = Vec::new();
    let mut duplicates = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_deps = matches!(
                trimmed,
                "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
            );
            continue;
        }
        if in_deps && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some(name) = trimmed.split(['=', ' ']).next() {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    if seen.contains(&name) {
                        duplicates.push(name);
                    } else {
                        seen.push(name);
                    }
                }
            }
        }
    }
    Ok(DepsReport {
        total: seen.len(),
        duplicates,
    })
}

/// Count and compare `dependencies` / `devDependencies` in a package.json.
fn inspect_package_json(path: &Path) -> anyhow::Result<DepsReport> {
    let text = std::fs::read_to_string(path)?;
    let deps = json_object_keys(&text, "dependencies");
    let dev = json_object_keys(&text, "devDependencies");

    let mut duplicates = Vec::new();
    for d in &deps {
        if dev.contains(d) {
            duplicates.push(d.clone());
        }
    }
    let mut all = deps;
    for d in dev {
        if !all.contains(&d) {
            all.push(d);
        }
    }
    Ok(DepsReport {
        total: all.len(),
        duplicates,
    })
}

/// Very small extractor: keys of a top-level JSON object field.
fn json_object_keys(text: &str, field: &str) -> Vec<String> {
    let needle = format!("\"{field}\"");
    let Some(start) = text.find(&needle) else {
        return Vec::new();
    };
    let Some(brace) = text[start..].find('{') else {
        return Vec::new();
    };
    let rest = &text[start + brace + 1..];
    let Some(end) = rest.find('}') else {
        return Vec::new();
    };
    let body = &rest[..end];
    body.lines()
        .filter_map(|l| {
            let inner = l.trim().strip_prefix('"')?;
            inner.find('"').map(|e| inner[..e].to_string())
        })
        .collect()
}

/// Count non-empty, non-comment lines in requirements.txt.
fn inspect_requirements(path: &Path) -> anyhow::Result<DepsReport> {
    let text = std::fs::read_to_string(path)?;
    let mut seen: Vec<String> = Vec::new();
    let mut duplicates = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let name = l
            .split(['=', '>', '<', '~', '!', ' '])
            .next()
            .unwrap_or(l)
            .to_string();
        if seen.contains(&name) {
            duplicates.push(name);
        } else {
            seen.push(name);
        }
    }
    Ok(DepsReport {
        total: seen.len(),
        duplicates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str, name: &str, contents: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("flux-deps-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), contents).unwrap();
        dir
    }

    #[test]
    fn counts_cargo_dependencies() {
        let dir = tmp(
            "cargo",
            "Cargo.toml",
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n[dependencies]\nserde = \"1\"\nclap = \"4\"\n[dev-dependencies]\ntempfile = \"3\"\n",
        );
        let r = inspect(&dir, "rust").unwrap();
        assert_eq!(r.total, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_node_duplicate_across_deps_and_dev() {
        let dir = tmp(
            "node",
            "package.json",
            "{\n \"dependencies\": {\n  \"react\": \"18\",\n  \"lodash\": \"4\"\n },\n \"devDependencies\": {\n  \"lodash\": \"4\"\n }\n}\n",
        );
        let r = inspect(&dir, "node").unwrap();
        assert_eq!(r.total, 2);
        assert_eq!(r.duplicates, vec!["lodash"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
