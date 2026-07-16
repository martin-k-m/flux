//! `flux doctor` — environment health checks (4.5).

use std::path::Path;
use std::process::Command;

use crate::core::config;
use crate::core::detect::Detection;
use crate::runners::containers;

/// A single health-check result.
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// Repository-wide health checks (`flux doctor --all`): the things that make a
/// project releasable — CI, examples, docs, packaging, and community files.
pub fn repository_checks(root: &Path) -> Vec<Check> {
    let mut checks = Vec::new();

    let present = |rel: &str| root.join(rel).exists();
    let mut file_check = |name: &str, rel: &str| {
        checks.push(Check {
            name: name.to_string(),
            ok: present(rel),
            detail: if present(rel) {
                rel.to_string()
            } else {
                format!("missing {rel}")
            },
        });
    };

    file_check("CI workflow", ".github/workflows/ci.yml");
    file_check("Release workflow", ".github/workflows/release.yml");
    file_check("Changelog", "CHANGELOG.md");
    file_check("Contributing guide", "CONTRIBUTING.md");
    file_check("Security policy", "SECURITY.md");
    file_check("Code of conduct", "CODE_OF_CONDUCT.md");
    file_check("Issue templates", ".github/ISSUE_TEMPLATE");
    file_check("PR template", ".github/PULL_REQUEST_TEMPLATE.md");
    file_check("Documentation", "docs");

    // Examples: every example directory must carry a `.flux`.
    let examples_dir = root.join("examples");
    if examples_dir.is_dir() {
        let mut ok = true;
        let mut missing = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&examples_dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() && !p.join(".flux").is_file() && !p.join("flux.workspace").is_file() {
                    ok = false;
                    if let Some(n) = p.file_name().and_then(|n| n.to_str()) {
                        missing.push(n.to_string());
                    }
                }
            }
        }
        checks.push(Check {
            name: "Examples".to_string(),
            ok,
            detail: if ok {
                "all carry a .flux".to_string()
            } else {
                format!("missing .flux: {}", missing.join(", "))
            },
        });
    } else {
        checks.push(Check {
            name: "Examples".to_string(),
            ok: false,
            detail: "no examples/ directory".to_string(),
        });
    }

    checks
}

/// Run all checks for the project at `root`.
pub fn run(root: &Path, detection: &Detection) -> Vec<Check> {
    let mut checks = Vec::new();

    // Git.
    checks.push(binary_check("git", "git"));

    // Language toolchain.
    if let Some(lang) = &detection.language {
        let tool = match lang.as_str() {
            "rust" => "cargo",
            "node" => "node",
            "python" => "python",
            "go" => "go",
            other => other,
        };
        checks.push(binary_check(&format!("{lang} toolchain"), tool));
    } else {
        checks.push(Check {
            name: "language".into(),
            ok: false,
            detail: "no supported project detected".into(),
        });
    }

    // Container engine (optional).
    checks.push(match containers::engine() {
        Some(e) => Check {
            name: "container engine".into(),
            ok: true,
            detail: e.to_string(),
        },
        None => Check {
            name: "container engine".into(),
            ok: true, // optional, not a failure
            detail: "none (optional)".into(),
        },
    });

    // .flux config presence and validity.
    let flux_path = root.join(config::CONFIG_FILE);
    if flux_path.is_file() {
        match config::load(&flux_path) {
            Ok(_) => checks.push(Check {
                name: ".flux config".into(),
                ok: true,
                detail: "present and valid".into(),
            }),
            Err(e) => checks.push(Check {
                name: ".flux config".into(),
                ok: false,
                detail: format!("parse error: {e}"),
            }),
        }
    } else {
        checks.push(Check {
            name: ".flux config".into(),
            ok: false,
            detail: "missing — run `flux init`".into(),
        });
    }

    // Cache directory is writable.
    let cache_dir = root.join(".flux-cache");
    let writable = std::fs::create_dir_all(&cache_dir)
        .and_then(|_| {
            let probe = cache_dir.join(".doctor-probe");
            std::fs::write(&probe, b"ok")?;
            std::fs::remove_file(&probe)
        })
        .is_ok();
    checks.push(Check {
        name: "cache writable".into(),
        ok: writable,
        detail: if writable {
            ".flux-cache/".into()
        } else {
            "cannot write to .flux-cache/".into()
        },
    });

    // Installed plugins.
    let installed = crate::plugins::installed(root);
    checks.push(Check {
        name: "plugins".into(),
        ok: true,
        detail: if installed.is_empty() {
            "none installed".into()
        } else {
            installed.join(", ")
        },
    });

    checks
}

fn binary_check(name: &str, binary: &str) -> Check {
    let version = binary_version(binary);
    Check {
        name: name.to_string(),
        ok: version.is_some(),
        detail: version.unwrap_or_else(|| format!("'{binary}' not found on PATH")),
    }
}

fn binary_version(binary: &str) -> Option<String> {
    let out = Command::new(binary).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string(),
    )
}
