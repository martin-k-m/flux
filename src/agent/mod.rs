//! Flux Runner System — local agents.
//!
//! Flux runs pipeline steps on the local machine's cores. True cross-machine
//! distribution (a controller assigning jobs to workers over gRPC, with
//! heartbeats, a job queue, and auth) is out of scope; this module implements
//! the **local** runner model honestly:
//!
//! * `flux runners start` registers *this* machine as an available runner and
//!   reports its capacity;
//! * the graph engine schedules pipeline steps across this runner's cores (that
//!   is the local worker);
//! * `flux runners list` shows the registered runners.
//!
//! Registrations are recorded under `.flux-cache/runners/`.

use std::io;
use std::path::{Path, PathBuf};

/// A machine that can run Flux jobs.
#[derive(Debug, Clone)]
pub struct Runner {
    pub name: String,
    pub cpu_cores: usize,
    /// Total RAM in MB, if we could determine it.
    pub ram_mb: Option<u64>,
    pub os: String,
    pub arch: String,
}

impl Runner {
    /// Describe the machine Flux is running on right now.
    pub fn this_machine() -> Runner {
        Runner {
            name: hostname(),
            cpu_cores: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            ram_mb: total_ram_mb(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }

    fn to_record(&self) -> String {
        format!(
            "name = {}\ncpu_cores = {}\nram_mb = {}\nos = {}\narch = {}\n",
            self.name,
            self.cpu_cores,
            self.ram_mb
                .map(|m| m.to_string())
                .unwrap_or_else(|| "unknown".into()),
            self.os,
            self.arch,
        )
    }

    fn from_record(text: &str) -> Option<Runner> {
        let mut name = None;
        let mut cores = None;
        let mut ram = None;
        let mut os = String::new();
        let mut arch = String::new();
        for line in text.lines() {
            let (k, v) = line.split_once('=').map(|(k, v)| (k.trim(), v.trim()))?;
            match k {
                "name" => name = Some(v.to_string()),
                "cpu_cores" => cores = v.parse().ok(),
                "ram_mb" => ram = v.parse().ok(),
                "os" => os = v.to_string(),
                "arch" => arch = v.to_string(),
                _ => {}
            }
        }
        Some(Runner {
            name: name?,
            cpu_cores: cores?,
            ram_mb: ram,
            os,
            arch,
        })
    }
}

fn runners_dir(root: &Path) -> PathBuf {
    root.join(".flux-cache").join("runners")
}

/// Register this machine as an available runner.
pub fn register_self(root: &Path) -> io::Result<Runner> {
    let dir = runners_dir(root);
    std::fs::create_dir_all(&dir)?;
    let runner = Runner::this_machine();
    let safe: String = runner
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    std::fs::write(dir.join(format!("{safe}.runner")), runner.to_record())?;
    Ok(runner)
}

/// List all registered runners.
pub fn list(root: &Path) -> io::Result<Vec<Runner>> {
    let dir = runners_dir(root);
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|e| e.to_str()) == Some("runner") {
            if let Ok(text) = std::fs::read_to_string(entry.path()) {
                if let Some(r) = Runner::from_record(&text) {
                    out.push(r);
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string())
}

/// Best-effort total RAM. Reads `/proc/meminfo` on Linux; unknown elsewhere
/// (querying it on Windows needs a syscall crate this toolchain can't link).
fn total_ram_mb() -> Option<u64> {
    if cfg!(target_os = "linux") {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                let kb: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
                return Some(kb / 1024);
            }
        }
    }
    None
}
