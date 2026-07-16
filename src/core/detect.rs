//! Project detection.
//!
//! Flux inspects a directory for well-known marker files and infers the
//! project's language, name, and whether it has tests and a working toolchain.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A supported language marker.
struct Marker {
    /// The file that signals this language, e.g. `Cargo.toml`.
    file: &'static str,
    /// The canonical Flux language id, e.g. `rust`.
    language: &'static str,
    /// The toolchain executable used to check availability, e.g. `cargo`.
    toolchain: &'static str,
}

/// Ordered by precedence: the first marker found wins.
const MARKERS: &[Marker] = &[
    Marker {
        file: "Cargo.toml",
        language: "rust",
        toolchain: "cargo",
    },
    Marker {
        file: "package.json",
        language: "node",
        toolchain: "node",
    },
    Marker {
        file: "requirements.txt",
        language: "python",
        toolchain: "python",
    },
    Marker {
        file: "pyproject.toml",
        language: "python",
        toolchain: "python",
    },
    Marker {
        file: "go.mod",
        language: "go",
        toolchain: "go",
    },
    Marker {
        file: "pom.xml",
        language: "java",
        toolchain: "mvn",
    },
];

/// The outcome of inspecting a project directory.
#[derive(Debug, Clone)]
pub struct Detection {
    /// Detected language id, if any.
    pub language: Option<String>,
    /// Detected project name, if we could read one.
    pub name: Option<String>,
    /// Marker files found, as `(filename, found)` for display.
    pub markers: Vec<(String, bool)>,
    /// Whether the project appears to have tests.
    pub has_tests: bool,
    /// Whether the language toolchain is available on `PATH`.
    pub toolchain_available: bool,
}

impl Detection {
    /// Human label for the detected language (`Rust`, `Node`, ...).
    pub fn language_label(&self) -> String {
        match self.language.as_deref() {
            Some("rust") => "Rust".into(),
            Some("node") => "Node".into(),
            Some("python") => "Python".into(),
            Some("go") => "Go".into(),
            Some("java") => "Java".into(),
            Some(other) => other.to_string(),
            None => "Unknown".into(),
        }
    }
}

/// Inspect `dir` and report what Flux can infer about it.
pub fn detect(dir: &Path) -> Detection {
    let mut language = None;
    let mut toolchain = None;
    let mut markers = Vec::new();

    for m in MARKERS {
        let found = dir.join(m.file).is_file();
        if found && language.is_none() {
            language = Some(m.language.to_string());
            toolchain = Some(m.toolchain);
        }
        // Only surface the marker line for the languages we actually matched
        // or the primary Cargo/package markers, to keep `info` readable.
        if found {
            markers.push((m.file.to_string(), true));
        }
    }

    let name = language
        .as_deref()
        .and_then(|lang| read_project_name(dir, lang));

    let has_tests = language
        .as_deref()
        .map(|lang| detect_tests(dir, lang))
        .unwrap_or(false);

    let toolchain_available = toolchain.map(toolchain_available).unwrap_or(false);

    Detection {
        language,
        name,
        markers,
        has_tests,
        toolchain_available,
    }
}

/// Try to read a project's declared name from its manifest.
fn read_project_name(dir: &Path, language: &str) -> Option<String> {
    match language {
        "rust" => manifest_name_cargo(&dir.join("Cargo.toml")),
        "node" => manifest_name_json(&dir.join("package.json")),
        "python" => manifest_name_pyproject(&dir.join("pyproject.toml")),
        _ => None,
    }
}

/// Extract `name = "..."` from the `[package]` table of a Cargo.toml.
fn manifest_name_cargo(path: &PathBuf) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_package = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package {
            if let Some(rest) = trimmed.strip_prefix("name") {
                if let Some(val) = rest.trim_start().strip_prefix('=') {
                    return Some(unquote(val.trim()));
                }
            }
        }
    }
    None
}

/// Extract the top-level `"name"` field from a package.json.
fn manifest_name_json(path: &PathBuf) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("\"name\"") {
            if let Some(val) = rest.trim_start().strip_prefix(':') {
                let v = val.trim().trim_end_matches(',');
                return Some(unquote(v));
            }
        }
    }
    None
}

/// Extract `name = "..."` from `[project]` or `[tool.poetry]` in pyproject.toml.
fn manifest_name_pyproject(path: &PathBuf) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_name_table = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_name_table = matches!(trimmed, "[project]" | "[tool.poetry]");
            continue;
        }
        if in_name_table {
            if let Some(rest) = trimmed.strip_prefix("name") {
                if let Some(val) = rest.trim_start().strip_prefix('=') {
                    return Some(unquote(val.trim()));
                }
            }
        }
    }
    None
}

/// Strip surrounding single or double quotes.
fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// Heuristic: does this project appear to have tests?
fn detect_tests(dir: &Path, language: &str) -> bool {
    match language {
        "rust" => dir.join("tests").is_dir() || source_contains(&dir.join("src"), "#[test]"),
        "node" => package_json_has_test_script(&dir.join("package.json")),
        "python" => {
            dir.join("tests").is_dir()
                || dir.join("test").is_dir()
                || has_file_prefixed(dir, "test_")
        }
        "go" => has_file_suffixed(dir, "_test.go"),
        _ => false,
    }
}

fn package_json_has_test_script(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .map(|t| t.contains("\"test\""))
        .unwrap_or(false)
}

/// Shallowly scan a directory for a file whose name starts with `prefix`.
fn has_file_prefixed(dir: &Path, prefix: &str) -> bool {
    read_dir_names(dir).iter().any(|n| n.starts_with(prefix))
}

/// Shallowly scan a directory for a file whose name ends with `suffix`.
fn has_file_suffixed(dir: &Path, suffix: &str) -> bool {
    read_dir_names(dir).iter().any(|n| n.ends_with(suffix))
}

fn read_dir_names(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if let Ok(name) = e.file_name().into_string() {
                names.push(name);
            }
        }
    }
    names
}

/// Check whether any file directly under `dir` contains `needle`.
fn source_contains(dir: &Path, needle: &str) -> bool {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let path = e.path();
            if path.is_file() {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if text.contains(needle) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Is `tool` runnable? We probe `tool --version` and check it starts.
fn toolchain_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
