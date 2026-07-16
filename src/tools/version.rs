//! `flux version bump <part>` — semantic version management (4.5).

use std::path::Path;

/// Which part of a semver to bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Major,
    Minor,
    Patch,
}

impl Part {
    pub fn parse(s: &str) -> Option<Part> {
        match s.to_lowercase().as_str() {
            "major" => Some(Part::Major),
            "minor" => Some(Part::Minor),
            "patch" => Some(Part::Patch),
            _ => None,
        }
    }
}

/// Bump a `MAJOR.MINOR.PATCH` string. Preserves nothing after patch (drops any
/// pre-release/build metadata, which is the conventional behaviour on a bump).
pub fn bump_semver(version: &str, part: Part) -> Option<String> {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let mut nums = core.split('.');
    let major: u64 = nums.next()?.trim().parse().ok()?;
    let minor: u64 = nums.next().unwrap_or("0").trim().parse().ok()?;
    let patch: u64 = nums.next().unwrap_or("0").trim().parse().ok()?;
    let (major, minor, patch) = match part {
        Part::Major => (major + 1, 0, 0),
        Part::Minor => (major, minor + 1, 0),
        Part::Patch => (major, minor, patch + 1),
    };
    Some(format!("{major}.{minor}.{patch}"))
}

/// Read, bump, and write the project version. Returns `(old, new)`.
pub fn bump_project(root: &Path, language: &str, part: Part) -> anyhow::Result<(String, String)> {
    match language {
        "rust" => bump_toml(&root.join("Cargo.toml"), part),
        "node" => bump_json(&root.join("package.json"), part),
        other => anyhow::bail!("version bumping isn't supported for '{other}' yet"),
    }
}

/// Bump `version = "x.y.z"` in a Cargo.toml (only within `[package]`).
fn bump_toml(path: &Path, part: Part) -> anyhow::Result<(String, String)> {
    let text = std::fs::read_to_string(path)?;
    let mut in_package = false;
    let mut old = None;
    let mut new_version = String::new();
    let mut out = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
        }
        if in_package && old.is_none() && trimmed.starts_with("version") {
            if let Some(v) = extract_quoted(trimmed) {
                let bumped = bump_semver(&v, part)
                    .ok_or_else(|| anyhow::anyhow!("'{v}' is not a valid semver"))?;
                old = Some(v);
                new_version = bumped.clone();
                out.push_str(&format!("version = \"{bumped}\"\n"));
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }

    let old = old.ok_or_else(|| anyhow::anyhow!("no version found in {}", path.display()))?;
    std::fs::write(path, out)?;
    Ok((old, new_version))
}

/// Bump the top-level `"version": "x.y.z"` in a package.json.
fn bump_json(path: &Path, part: Part) -> anyhow::Result<(String, String)> {
    let text = std::fs::read_to_string(path)?;
    let mut old = None;
    let mut new_version = String::new();
    let mut out = String::new();

    for line in text.lines() {
        let trimmed = line.trim_start();
        if old.is_none() && trimmed.starts_with("\"version\"") {
            if let Some(v) = extract_quoted_after_colon(trimmed) {
                let bumped = bump_semver(&v, part)
                    .ok_or_else(|| anyhow::anyhow!("'{v}' is not a valid semver"))?;
                old = Some(v);
                new_version = bumped.clone();
                let indent = &line[..line.len() - trimmed.len()];
                let comma = if trimmed.trim_end().ends_with(',') {
                    ","
                } else {
                    ""
                };
                out.push_str(&format!("{indent}\"version\": \"{bumped}\"{comma}\n"));
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }

    let old = old.ok_or_else(|| anyhow::anyhow!("no version found in {}", path.display()))?;
    std::fs::write(path, out)?;
    Ok((old, new_version))
}

/// Extract the value from `key = "value"`.
fn extract_quoted(line: &str) -> Option<String> {
    let start = line.find('"')? + 1;
    let end = line[start..].find('"')? + start;
    Some(line[start..end].to_string())
}

/// Extract the value from `"key": "value"`.
fn extract_quoted_after_colon(line: &str) -> Option<String> {
    let after = line.split_once(':')?.1;
    let start = after.find('"')? + 1;
    let end = after[start..].find('"')? + start;
    Some(after[start..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bumps_each_part() {
        assert_eq!(bump_semver("1.2.3", Part::Major).unwrap(), "2.0.0");
        assert_eq!(bump_semver("1.2.3", Part::Minor).unwrap(), "1.3.0");
        assert_eq!(bump_semver("1.2.3", Part::Patch).unwrap(), "1.2.4");
    }

    #[test]
    fn drops_prerelease_on_bump() {
        assert_eq!(bump_semver("1.2.3-beta.1", Part::Patch).unwrap(), "1.2.4");
    }

    #[test]
    fn rejects_non_semver() {
        assert!(bump_semver("not-a-version", Part::Patch).is_none());
    }

    #[test]
    fn bumps_cargo_toml_only_in_package() {
        let dir = std::env::temp_dir().join(format!("flux-ver-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let toml =
            "[package]\nname = \"x\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0.0\"\n";
        std::fs::write(dir.join("Cargo.toml"), toml).unwrap();

        let (old, new) = bump_toml(&dir.join("Cargo.toml"), Part::Minor).unwrap();
        assert_eq!(old, "0.1.0");
        assert_eq!(new, "0.2.0");
        let written = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(written.contains("version = \"0.2.0\""));
        // The dependency version must be untouched.
        assert!(written.contains("serde = \"1.0.0\""));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
