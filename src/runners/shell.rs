//! The shell runner: executes a command string in the project directory.
//!
//! Two modes:
//! * [`run`] streams output straight to the terminal (inherited stdio) — used
//!   by the linear pipeline where interleaving isn't a concern;
//! * [`run_captured`] buffers combined stdout+stderr and returns it — used by
//!   the parallel graph engine so concurrent steps don't interleave.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// The result of running a single command (streamed).
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub success: bool,
}

/// The result of running a single command with its output captured.
#[derive(Debug, Clone)]
pub struct CapturedResult {
    pub success: bool,
    pub duration: Duration,
    /// Combined stdout + stderr.
    pub output: String,
}

/// Build a platform shell invocation for `cmd`.
#[cfg(windows)]
fn shell(cmd: &str) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/C").arg(cmd);
    c
}

#[cfg(not(windows))]
fn shell(cmd: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd);
    c
}

/// Run `cmd` in `dir`, streaming its output. Returns whether it succeeded.
pub fn run(cmd: &str, dir: &Path) -> std::io::Result<CommandResult> {
    let status = shell(cmd).current_dir(dir).status()?;
    Ok(CommandResult {
        success: status.success(),
    })
}

/// Run `cmd` in `dir` with extra environment variables, capturing its output.
pub fn run_captured(
    cmd: &str,
    dir: &Path,
    env: &[(String, String)],
) -> std::io::Result<CapturedResult> {
    let start = Instant::now();
    let mut command = shell(cmd);
    command.current_dir(dir);
    for (k, v) in env {
        command.env(k, v);
    }
    let out = command.output()?;

    let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stderr.is_empty() {
        output.push_str(&stderr);
    }

    Ok(CapturedResult {
        success: out.status.success(),
        duration: start.elapsed(),
        output,
    })
}
