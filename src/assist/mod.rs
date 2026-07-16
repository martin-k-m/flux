//! Flux Assist (2.12) — heuristic failure diagnosis.
//!
//! Flux is **not** an AI wrapper. When a step fails, Flux matches the command
//! and its output against a table of known failure signatures and offers
//! plain, deterministic suggestions. No model call, no network — just useful
//! pattern matching that a developer would otherwise do in their head.

/// A diagnosis: a likely cause and a suggested fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub cause: String,
    pub fix: String,
}

struct Rule {
    /// Substrings that must all appear in the combined command+output (case-insensitive).
    needles: &'static [&'static str],
    cause: &'static str,
    fix: &'static str,
}

const RULES: &[Rule] = &[
    Rule {
        needles: &["cargo", "no such command"],
        cause: "Cargo subcommand missing",
        fix: "Install it, e.g. `cargo install cargo-<name>`, or check the command spelling.",
    },
    Rule {
        needles: &["error: linker", "link.exe"],
        cause: "Linker not found or misconfigured",
        fix: "Install the platform toolchain (MSVC build tools or MinGW), or avoid crates that need it.",
    },
    Rule {
        needles: &["openssl"],
        cause: "OpenSSL headers/libraries are missing",
        fix: "Install OpenSSL dev files (`libssl-dev` on Debian/Ubuntu, `openssl-devel` on Fedora), or use a vendored/rustls feature.",
    },
    Rule {
        needles: &["pkg-config", "not found"],
        cause: "A native library isn't discoverable via pkg-config",
        fix: "Install pkg-config and the library's -dev package, or set PKG_CONFIG_PATH.",
    },
    Rule {
        needles: &["undefined reference"],
        cause: "A native symbol failed to link",
        fix: "Install the missing system library (its -dev/-devel package) and rebuild.",
    },
    Rule {
        needles: &["rustc", "requires rustc"],
        cause: "Rust version too old for a dependency",
        fix: "Run `rustup update` to get the latest stable toolchain.",
    },
    Rule {
        needles: &["edition2024"],
        cause: "A dependency needs a newer Rust edition",
        fix: "Run `rustup update`, or pin the dependency to an older compatible version.",
    },
    Rule {
        needles: &["e404", "npm err"],
        cause: "npm package not found in the registry",
        fix: "Check the package name/version in package.json, or your registry configuration.",
    },
    Rule {
        needles: &["eresolve"],
        cause: "npm dependency conflict",
        fix: "Reconcile peer dependencies, or retry with `npm install --legacy-peer-deps`.",
    },
    Rule {
        needles: &["modulenotfounderror"],
        cause: "A Python module is not installed",
        fix: "Install it with `pip install <module>` (or add it to requirements.txt).",
    },
    Rule {
        needles: &["could not find a version that satisfies"],
        cause: "pip cannot satisfy a version constraint",
        fix: "Loosen the version pin in requirements.txt, or upgrade pip: `pip install -U pip`.",
    },
    Rule {
        needles: &["permission denied"],
        cause: "The command lacked permission",
        fix: "Check file permissions/ownership, or whether a file is locked by another process.",
    },
    Rule {
        needles: &["command not found"],
        cause: "A required executable is not on PATH",
        fix: "Install the tool or add it to PATH; verify with `<tool> --version`.",
    },
    Rule {
        needles: &["connection refused"],
        cause: "A network service was unreachable",
        fix: "Confirm the service is running and the host/port are correct.",
    },
    Rule {
        needles: &["no space left on device"],
        cause: "The disk is full",
        fix: "Free up space (e.g. `flux clean`, remove old artifacts) and retry.",
    },
];

/// Diagnose a failed command. Returns any matching suggestions (may be empty).
pub fn diagnose(command: &str, output: &str) -> Vec<Suggestion> {
    let haystack = format!("{command}\n{output}").to_lowercase();
    let mut out = Vec::new();
    for rule in RULES {
        if rule
            .needles
            .iter()
            .all(|n| haystack.contains(&n.to_lowercase()))
        {
            out.push(Suggestion {
                cause: rule.cause.to_string(),
                fix: rule.fix.to_string(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnoses_missing_python_module() {
        let s = diagnose("pytest", "ModuleNotFoundError: No module named 'requests'");
        assert!(s.iter().any(|x| x.cause.contains("Python module")));
    }

    #[test]
    fn diagnoses_linker_error() {
        let s = diagnose("cargo build", "error: linker `link.exe` not found");
        assert!(s.iter().any(|x| x.cause.contains("Linker")));
    }

    #[test]
    fn no_false_positive_on_clean_output() {
        assert!(diagnose("echo hi", "hi").is_empty());
    }
}
