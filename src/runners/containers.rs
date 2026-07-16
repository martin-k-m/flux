//! Flux Containers — lightweight, ephemeral **build environments**.
//!
//! This is not a Docker replacement. When a `.flux` declares an
//! `environment { image "rust:latest" }`, Flux wraps each command so it runs
//! inside that image via whatever OCI engine is available (Docker or Podman),
//! then the container is torn down (`--rm`). If no engine is installed, Flux
//! degrades gracefully and runs the command natively.

use std::path::Path;
use std::process::{Command, Stdio};

/// The OCI engines we know how to drive, in preference order.
const ENGINES: &[&str] = &["docker", "podman"];

/// The first available container engine on this machine, if any.
pub fn engine() -> Option<&'static str> {
    ENGINES.iter().copied().find(|&e| is_available(e))
}

/// Is a container engine available at all?
pub fn available() -> bool {
    engine().is_some()
}

fn is_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Wrap `cmd` so it runs inside `image` with `root` mounted as the working
/// directory. Returns `None` when no engine is available (run natively).
///
/// The container is ephemeral: `--rm` destroys it when the command exits.
pub fn wrap_command(cmd: &str, image: &str, root: &Path) -> Option<String> {
    let engine = engine()?;
    let mount = mount_path(root);
    // Single-quote the inner command for the container's shell; escape any
    // embedded single quotes the POSIX way.
    let inner = cmd.replace('\'', "'\\''");
    Some(format!(
        "{engine} run --rm -v \"{mount}:/workspace\" -w /workspace {image} sh -c '{inner}'"
    ))
}

/// Render a host path for a `-v` bind mount. Docker Desktop accepts Windows
/// paths with forward slashes.
fn mount_path(root: &Path) -> String {
    let s = root.display().to_string();
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s
    }
}
