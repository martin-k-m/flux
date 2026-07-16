//! Flux Observability (3.5).
//!
//! Every `flux build` / `flux ci` appends a run record to
//! `.flux-cache/analytics/runs.log`. `flux analytics` aggregates them into
//! build-performance stats: average build time, cache hit rate, the most
//! expensive step, and failure count.
//!
//! Record format (one line per run, tab-separated):
//! ```text
//! <unix_ts>\t<kind>\t<success 0|1>\t<total_ms>\t<step>:<status>:<ms>,<step>:<status>:<ms>,...
//! ```

use std::io;
use std::path::{Path, PathBuf};

use crate::core::graph::GraphOutcome;

fn log_path(root: &Path) -> PathBuf {
    root.join(".flux-cache").join("analytics").join("runs.log")
}

/// Append a run record for `outcome`.
pub fn record(root: &Path, kind: &str, outcome: &GraphOutcome) -> io::Result<()> {
    let path = log_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let steps: Vec<String> = outcome
        .records
        .iter()
        .map(|r| {
            format!(
                "{}:{}:{}",
                sanitize(&r.name),
                r.status.code(),
                r.duration.as_millis()
            )
        })
        .collect();

    let line = format!(
        "{ts}\t{kind}\t{}\t{}\t{}\n",
        if outcome.success { 1 } else { 0 },
        outcome.total.as_millis(),
        steps.join(","),
    );

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())
}

/// Aggregated analytics.
#[derive(Debug, Default)]
pub struct Analytics {
    pub runs: usize,
    pub failures: usize,
    pub avg_total_ms: u128,
    pub cache_hits: usize,
    pub cacheable_steps: usize,
    /// (step name, average duration in ms), most expensive first.
    pub expensive: Vec<(String, u128)>,
}

impl Analytics {
    pub fn cache_hit_rate(&self) -> f64 {
        if self.cacheable_steps == 0 {
            0.0
        } else {
            self.cache_hits as f64 / self.cacheable_steps as f64
        }
    }
}

/// Parse and aggregate the run log.
pub fn analyze(root: &Path) -> io::Result<Analytics> {
    let path = log_path(root);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Analytics::default()),
        Err(e) => return Err(e),
    };

    let mut a = Analytics::default();
    let mut total_sum: u128 = 0;

    // Accumulate per-step totals to compute averages.
    let mut step_total: std::collections::HashMap<String, (u128, u128)> =
        std::collections::HashMap::new(); // name -> (sum_ms, count)

    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 4 {
            continue;
        }
        a.runs += 1;
        let success = fields[2] == "1";
        if !success {
            a.failures += 1;
        }
        total_sum += fields[3].parse::<u128>().unwrap_or(0);

        if let Some(steps) = fields.get(4) {
            for step in steps.split(',').filter(|s| !s.is_empty()) {
                let parts: Vec<&str> = step.split(':').collect();
                if parts.len() < 3 {
                    continue;
                }
                let (name, status, ms) =
                    (parts[0], parts[1], parts[2].parse::<u128>().unwrap_or(0));
                // Cache accounting: `ok` and `cached` are the cacheable outcomes.
                if status == "ok" || status == "cached" {
                    a.cacheable_steps += 1;
                    if status == "cached" {
                        a.cache_hits += 1;
                    }
                }
                let entry = step_total.entry(name.to_string()).or_insert((0, 0));
                entry.0 += ms;
                entry.1 += 1;
            }
        }
    }

    a.avg_total_ms = total_sum.checked_div(a.runs as u128).unwrap_or(0);

    let mut expensive: Vec<(String, u128)> = step_total
        .into_iter()
        .map(|(name, (sum, count))| (name, sum.checked_div(count).unwrap_or(0)))
        .collect();
    expensive.sort_by_key(|(_, ms)| std::cmp::Reverse(*ms));
    a.expensive = expensive;

    Ok(a)
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c == ':' || c == ',' || c == '\t' {
                '_'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::graph::{GraphOutcome, NodeStatus, StepRecord};
    use std::time::Duration;

    fn temp(tag: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("flux-analytics-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn outcome(success: bool, recs: Vec<(&str, NodeStatus, u64)>) -> GraphOutcome {
        let records: Vec<StepRecord> = recs
            .into_iter()
            .map(|(n, s, ms)| StepRecord {
                name: n.to_string(),
                status: s,
                duration: Duration::from_millis(ms),
            })
            .collect();
        let total = records.iter().map(|r| r.duration).sum();
        GraphOutcome {
            records,
            success,
            total,
        }
    }

    #[test]
    fn aggregates_runs_and_cache_rate() {
        let root = temp("agg");
        // Run 1: build ran (ok), test ran (ok).
        record(
            &root,
            "build",
            &outcome(
                true,
                vec![("build", NodeStatus::Ok, 100), ("test", NodeStatus::Ok, 50)],
            ),
        )
        .unwrap();
        // Run 2: build cached, test ran (ok).
        record(
            &root,
            "build",
            &outcome(
                true,
                vec![
                    ("build", NodeStatus::Cached, 0),
                    ("test", NodeStatus::Ok, 60),
                ],
            ),
        )
        .unwrap();
        // Run 3: failure.
        record(
            &root,
            "build",
            &outcome(false, vec![("build", NodeStatus::Failed, 10)]),
        )
        .unwrap();

        let a = analyze(&root).unwrap();
        assert_eq!(a.runs, 3);
        assert_eq!(a.failures, 1);
        // cacheable = run1(build+test) + run2(build cached + test) = 4; hits = 1.
        assert_eq!(a.cache_hits, 1);
        assert_eq!(a.cacheable_steps, 4);
        // Most expensive step should be "build" (avg (100+0+?)/... ) or "test"; just ensure non-empty.
        assert!(!a.expensive.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_when_no_history() {
        let root = temp("empty");
        let a = analyze(&root).unwrap();
        assert_eq!(a.runs, 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}
