//! The pipeline **graph** engine.
//!
//! Phase 1 executed steps in a straight line. Phase 2 turns the pipeline into a
//! real dependency graph:
//!
//! ```text
//!   frontend        backend
//!        \            /
//!         \          /
//!          +-> tests <-+
//!               |
//!            package
//! ```
//!
//! The engine resolves dependencies (`needs`), runs independent steps in
//! parallel, propagates failure (dependents of a failed step are skipped),
//! retries failed commands, and honours `only_if` conditions.
//!
//! ## Backward compatibility
//!
//! If **no** step declares `needs`, the pipeline is treated as a linear chain
//! in declared order — exactly the Phase 1 behaviour. The moment any step uses
//! `needs`, the whole pipeline becomes an explicit DAG (steps without `needs`
//! are roots and may run in parallel).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::cache::Cache;
use crate::core::config::Step;
use crate::core::logging as log;
use crate::core::runner::fmt_duration;
use crate::runners::{containers, shell};

/// A node in the pipeline graph.
#[derive(Debug)]
struct Node {
    step: Step,
    /// Indices of steps this one depends on.
    deps: Vec<usize>,
    /// Indices of steps that depend on this one.
    dependents: Vec<usize>,
}

/// A validated pipeline graph.
#[derive(Debug)]
pub struct Graph {
    nodes: Vec<Node>,
    /// Whether the graph came from explicit `needs` (vs. an implicit chain).
    explicit: bool,
}

/// A graph construction error (unknown dependency or a cycle).
#[derive(Debug)]
pub struct GraphError(pub String);

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for GraphError {}

/// How a node finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    Ok,
    Cached,
    Hook,
    /// Command failed after all retries.
    Failed,
    /// Skipped because a dependency failed.
    Skipped,
    /// Skipped because its `only_if` condition was false.
    Conditional,
    /// Could not launch the command.
    Errored,
}

impl NodeStatus {
    /// Does this status block dependents (cascade-skip them)?
    fn is_blocking(self) -> bool {
        matches!(
            self,
            NodeStatus::Failed | NodeStatus::Skipped | NodeStatus::Errored
        )
    }

    /// A short, stable code for logs and analytics.
    pub fn code(self) -> &'static str {
        match self {
            NodeStatus::Ok => "ok",
            NodeStatus::Cached => "cached",
            NodeStatus::Hook => "hook",
            NodeStatus::Failed => "failed",
            NodeStatus::Skipped => "skipped",
            NodeStatus::Conditional => "conditional",
            NodeStatus::Errored => "errored",
        }
    }
}

/// The result of running a single node.
struct NodeResult {
    idx: usize,
    status: NodeStatus,
    duration: Duration,
    /// Fully-formatted, ready-to-print output block (printed by the coordinator
    /// so nothing interleaves).
    block: String,
}

/// A per-step record in the outcome (drives the summary and analytics).
#[derive(Debug, Clone)]
pub struct StepRecord {
    pub name: String,
    pub status: NodeStatus,
    pub duration: Duration,
}

/// The aggregate result of executing a graph.
pub struct GraphOutcome {
    pub records: Vec<StepRecord>,
    pub success: bool,
    pub total: Duration,
}

impl GraphOutcome {
    pub fn ran(&self) -> usize {
        self.records
            .iter()
            .filter(|r| matches!(r.status, NodeStatus::Ok | NodeStatus::Failed))
            .count()
    }
}

/// Shared, read-only execution context handed to every worker.
pub struct ExecCtx {
    pub project_root: PathBuf,
    pub use_cache: bool,
    /// Variables for `only_if` evaluation (e.g. `branch`).
    pub vars: HashMap<String, String>,
    /// Resolved secret values, injected into steps that list them in `env`.
    pub secrets: HashMap<String, String>,
    /// Max steps to run concurrently.
    pub max_parallel: usize,
    /// If set, commands run inside this container image (when an engine exists).
    pub container_image: Option<String>,
}

impl ExecCtx {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        ExecCtx {
            project_root: project_root.into(),
            use_cache: true,
            vars: HashMap::new(),
            secrets: HashMap::new(),
            max_parallel: default_parallelism(),
            container_image: None,
        }
    }
}

/// A sensible default worker count.
pub fn default_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 16)
}

/// Select `targets` together with all their transitive dependencies (`needs`),
/// returning the closed sub-list of steps in their original order.
///
/// Used by `flux run <step>` and `flux test`: running a target also runs the
/// steps it depends on. For a linear pipeline (no `needs`), this is just the
/// targets themselves — matching the Phase 1 behaviour of running one step.
pub fn select_with_deps(steps: &[Step], targets: &[&str]) -> Vec<Step> {
    let index: HashMap<&str, usize> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.as_str(), i))
        .collect();

    let mut keep: HashSet<usize> = HashSet::new();
    let mut stack: Vec<usize> = targets
        .iter()
        .filter_map(|t| index.get(t).copied())
        .collect();
    while let Some(i) = stack.pop() {
        if !keep.insert(i) {
            continue;
        }
        for need in &steps[i].needs {
            if let Some(&d) = index.get(need.as_str()) {
                stack.push(d);
            }
        }
    }

    steps
        .iter()
        .enumerate()
        .filter(|(i, _)| keep.contains(i))
        .map(|(_, s)| s.clone())
        .collect()
}

/// Collect `branch` and similar variables for `only_if` evaluation.
pub fn build_vars(root: &Path) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    if let Some(branch) = git_branch(root) {
        vars.insert("branch".to_string(), branch);
    }
    vars
}

fn git_branch(root: &Path) -> Option<String> {
    // `--show-current` reports the branch name even on an unborn branch (no
    // commits yet), unlike `rev-parse --abbrev-ref HEAD` which needs a commit.
    let out = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

impl Graph {
    /// Build and validate a graph from the pipeline steps.
    pub fn build(steps: &[Step]) -> Result<Graph, GraphError> {
        let index: HashMap<&str, usize> = steps
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.as_str(), i))
            .collect();

        // Detect duplicate step names early — they make `needs` ambiguous.
        if index.len() != steps.len() {
            return Err(GraphError("duplicate step names in pipeline".into()));
        }

        let uses_needs = steps.iter().any(|s| !s.needs.is_empty());

        let mut nodes: Vec<Node> = steps
            .iter()
            .map(|s| Node {
                step: s.clone(),
                deps: Vec::new(),
                dependents: Vec::new(),
            })
            .collect();

        if uses_needs {
            // Explicit DAG from `needs`.
            for (i, step) in steps.iter().enumerate() {
                for need in &step.needs {
                    let dep = *index.get(need.as_str()).ok_or_else(|| {
                        GraphError(format!(
                            "step '{}' needs unknown step '{}'",
                            step.name, need
                        ))
                    })?;
                    if dep == i {
                        return Err(GraphError(format!("step '{}' needs itself", step.name)));
                    }
                    nodes[i].deps.push(dep);
                    nodes[dep].dependents.push(i);
                }
            }
        } else {
            // Implicit linear chain: each step depends on the previous one.
            for i in 1..nodes.len() {
                nodes[i].deps.push(i - 1);
                nodes[i - 1].dependents.push(i);
            }
        }

        let graph = Graph {
            nodes,
            explicit: uses_needs,
        };
        graph.check_acyclic()?;
        Ok(graph)
    }

    /// Whether the graph came from explicit `needs`.
    pub fn is_explicit(&self) -> bool {
        self.explicit
    }

    /// Kahn's algorithm — detects cycles by checking all nodes can be ordered.
    fn check_acyclic(&self) -> Result<(), GraphError> {
        let mut indeg: Vec<usize> = self.nodes.iter().map(|n| n.deps.len()).collect();
        let mut queue: VecDeque<usize> = (0..self.nodes.len()).filter(|&i| indeg[i] == 0).collect();
        let mut visited = 0;
        while let Some(i) = queue.pop_front() {
            visited += 1;
            for &d in &self.nodes[i].dependents {
                indeg[d] -= 1;
                if indeg[d] == 0 {
                    queue.push_back(d);
                }
            }
        }
        if visited != self.nodes.len() {
            let in_cycle: Vec<&str> = self
                .nodes
                .iter()
                .zip(indeg.iter())
                .filter(|(_, &d)| d > 0)
                .map(|(n, _)| n.step.name.as_str())
                .collect();
            return Err(GraphError(format!(
                "pipeline has a dependency cycle involving: {}",
                in_cycle.join(", ")
            )));
        }
        Ok(())
    }

    /// A topological presentation of steps for display (roots first).
    pub fn topo_order(&self) -> Vec<String> {
        let mut indeg: Vec<usize> = self.nodes.iter().map(|n| n.deps.len()).collect();
        let mut queue: VecDeque<usize> = (0..self.nodes.len()).filter(|&i| indeg[i] == 0).collect();
        let mut order = Vec::new();
        while let Some(i) = queue.pop_front() {
            order.push(self.nodes[i].step.name.clone());
            for &d in &self.nodes[i].dependents {
                indeg[d] -= 1;
                if indeg[d] == 0 {
                    queue.push_back(d);
                }
            }
        }
        order
    }

    /// Execute the graph. Prints progress; returns the aggregate outcome.
    pub fn execute(&self, ctx: &ExecCtx) -> GraphOutcome {
        let n = self.nodes.len();
        let mut status: Vec<Option<NodeStatus>> = vec![None; n];
        let mut indeg: Vec<usize> = self.nodes.iter().map(|node| node.deps.len()).collect();
        let mut durations: Vec<Duration> = vec![Duration::ZERO; n];

        // `rebuilt[i]` records whether node i actually rebuilt (vs. cache hit).
        // A node is force-rebuilt when any of its dependencies rebuilt — this is
        // the graph-aware invalidation half of the intelligent cache (3.2).
        let mut rebuilt: Vec<bool> = vec![false; n];
        let mut force: Vec<bool> = vec![false; n];

        // Work channel (coordinator -> workers) carries (node, force-rebuild);
        // the result channel goes back the other way. The work receiver is
        // shared behind a mutex so any idle worker can claim the next node.
        let (work_tx, work_rx) = channel::<(usize, bool)>();
        let work_rx = Arc::new(Mutex::new(work_rx));
        let (res_tx, res_rx) = channel::<NodeResult>();

        let workers = ctx.max_parallel.clamp(1, n.max(1));

        let outcome = std::thread::scope(|scope| {
            // Spawn a fixed pool of workers.
            for _ in 0..workers {
                let work_rx: Arc<Mutex<Receiver<(usize, bool)>>> = Arc::clone(&work_rx);
                let res_tx = res_tx.clone();
                scope.spawn(move || loop {
                    let job = {
                        let rx = work_rx.lock().unwrap();
                        rx.recv()
                    };
                    match job {
                        Ok((i, force)) => {
                            let result = self.run_node(i, ctx, force);
                            if res_tx.send(result).is_err() {
                                break;
                            }
                        }
                        Err(_) => break, // work channel closed → shut down
                    }
                });
            }
            // Drop our own result sender so the channel closes once all workers do.
            drop(res_tx);

            // Seed with nodes that have no dependencies.
            let mut ready: VecDeque<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
            let mut inflight = 0usize;
            let mut finished = 0usize;

            loop {
                // Dispatch everything currently ready.
                while let Some(i) = ready.pop_front() {
                    if status[i].is_some() {
                        continue; // already finalized (e.g. cascade-skipped)
                    }
                    work_tx.send((i, force[i])).expect("workers alive");
                    inflight += 1;
                    log::info_line(&format!(
                        "  {} {}",
                        log::dim("queued"),
                        self.nodes[i].step.name
                    ));
                }

                if finished == n {
                    break;
                }
                if inflight == 0 {
                    // Nothing running and nothing ready but not all finished:
                    // remaining nodes were cascade-skipped. Finalize loop.
                    // Safety: avoid deadlock — mark any unfinalized as skipped.
                    for s in status.iter_mut() {
                        if s.is_none() {
                            *s = Some(NodeStatus::Skipped);
                        }
                    }
                    break;
                }

                // Wait for a completion.
                let result = match res_rx.recv() {
                    Ok(r) => r,
                    Err(_) => break,
                };
                inflight -= 1;
                finished += 1;
                print!("{}", result.block);
                status[result.idx] = Some(result.status);
                durations[result.idx] = result.duration;
                rebuilt[result.idx] = result.status == NodeStatus::Ok;

                if result.status.is_blocking() {
                    // Cascade-skip all transitive dependents.
                    finished += self.cascade_skip(result.idx, &mut status);
                } else {
                    // Release dependents whose deps are now all satisfied.
                    for &dep in &self.nodes[result.idx].dependents {
                        if status[dep].is_some() {
                            continue;
                        }
                        indeg[dep] = indeg[dep].saturating_sub(1);
                        if indeg[dep] == 0 {
                            // Force a rebuild if any dependency rebuilt.
                            force[dep] = self.nodes[dep].deps.iter().any(|&d| rebuilt[d]);
                            ready.push_back(dep);
                        }
                    }
                }
            }

            // Close the work channel so workers exit; scope joins them.
            drop(work_tx);

            let records: Vec<StepRecord> = self
                .nodes
                .iter()
                .enumerate()
                .map(|(i, node)| StepRecord {
                    name: node.step.name.clone(),
                    status: status[i].unwrap_or(NodeStatus::Skipped),
                    duration: durations[i],
                })
                .collect();
            let success = records.iter().all(|r| !r.status.is_blocking());
            let total = durations.iter().copied().sum();
            GraphOutcome {
                records,
                success,
                total,
            }
        });

        outcome
    }

    /// Mark all transitive dependents of `idx` as skipped. Returns how many
    /// nodes were newly finalized.
    fn cascade_skip(&self, idx: usize, status: &mut [Option<NodeStatus>]) -> usize {
        let mut newly = 0;
        let mut stack: Vec<usize> = self.nodes[idx].dependents.clone();
        let mut seen: HashSet<usize> = HashSet::new();
        while let Some(d) = stack.pop() {
            if !seen.insert(d) {
                continue;
            }
            if status[d].is_none() {
                status[d] = Some(NodeStatus::Skipped);
                newly += 1;
                for &dd in &self.nodes[d].dependents {
                    stack.push(dd);
                }
            }
        }
        newly
    }

    /// Run one node, returning a fully-formatted output block. `force` skips the
    /// cache (set when a dependency rebuilt).
    fn run_node(&self, idx: usize, ctx: &ExecCtx, force: bool) -> NodeResult {
        let step = &self.nodes[idx].step;
        let mut block = String::new();

        // `only_if` guard.
        if let Some(cond) = &step.only_if {
            if !cond.evaluate(&ctx.vars) {
                block.push_str(&format!(
                    "  {} {}  {}\n",
                    log::yellow(log::DOT),
                    step.name,
                    log::dim(&format!("skipped (only_if {} is false)", cond.describe()))
                ));
                return NodeResult {
                    idx,
                    status: NodeStatus::Conditional,
                    duration: Duration::ZERO,
                    block,
                };
            }
        }

        // Tool hooks.
        if step.is_hook() {
            let tool = step.tool.as_deref().unwrap_or_default();
            block.push_str(&hook_line(&step.name, tool));
            return NodeResult {
                idx,
                status: NodeStatus::Hook,
                duration: Duration::ZERO,
                block,
            };
        }

        let command = match &step.command {
            Some(c) => c.clone(),
            None => {
                block.push_str(&format!(
                    "  {} {}  no command\n",
                    log::red(log::CROSS),
                    step.name
                ));
                return NodeResult {
                    idx,
                    status: NodeStatus::Errored,
                    duration: Duration::ZERO,
                    block,
                };
            }
        };

        let cache = Cache::new(&ctx.project_root);

        // Cache short-circuit — scoped to this step's declared `inputs`, and
        // skipped entirely when a dependency rebuilt (`force`).
        if ctx.use_cache && step.cache && !force {
            let hash = cache.source_hash_scoped(&step.inputs);
            if cache.is_fresh(&step.name, &hash) {
                let note = if step.inputs.is_empty() {
                    "(cached — no changes detected)".to_string()
                } else {
                    format!("(cached — {} unchanged)", step.inputs.join(", "))
                };
                block.push_str(&format!(
                    "  {} {}  {}\n",
                    log::green(log::CHECK),
                    step.name,
                    log::dim(&note)
                ));
                return NodeResult {
                    idx,
                    status: NodeStatus::Cached,
                    duration: Duration::ZERO,
                    block,
                };
            }
        }

        // Resolve secret env vars.
        let mut env: Vec<(String, String)> = Vec::new();
        for name in &step.env {
            match ctx.secrets.get(name) {
                Some(v) => env.push((name.clone(), v.clone())),
                None => block.push_str(&format!(
                    "  {} {}  {}\n",
                    log::yellow(log::DOT),
                    step.name,
                    log::dim(&format!("secret '{name}' not set — injected as empty"))
                )),
            }
        }

        // Container wrapping (if requested and an engine exists).
        let effective = match &ctx.container_image {
            Some(image) => containers::wrap_command(&command, image, &ctx.project_root)
                .unwrap_or_else(|| command.clone()),
            None => command.clone(),
        };

        // Header first, so retry notices and output appear beneath it.
        block.push_str(&format!(
            "  {} {}  {}\n",
            log::cyan(log::ARROW),
            step.name,
            log::dim(&command)
        ));

        // Run with retries.
        let max_attempts = step.retries + 1;
        let mut attempt = 0u32;
        let mut last_output = String::new();
        let mut last_status = NodeStatus::Failed;
        let mut total = Duration::ZERO;

        while attempt < max_attempts {
            attempt += 1;
            match shell::run_captured(&effective, &ctx.project_root, &env) {
                Ok(res) => {
                    total += res.duration;
                    last_output = res.output;
                    if res.success {
                        last_status = NodeStatus::Ok;
                        break;
                    } else {
                        last_status = NodeStatus::Failed;
                        if attempt < max_attempts {
                            block.push_str(&format!(
                                "  {} {}  {}\n",
                                log::yellow(log::DOT),
                                step.name,
                                log::dim(&format!(
                                    "attempt {attempt}/{max_attempts} failed, retrying"
                                ))
                            ));
                        }
                    }
                }
                Err(e) => {
                    last_status = NodeStatus::Errored;
                    last_output = format!("could not launch command: {e}");
                    break;
                }
            }
        }

        if last_status == NodeStatus::Ok && ctx.use_cache && step.cache {
            let hash = cache.source_hash_scoped(&step.inputs);
            let _ = cache.store(&step.name, &hash);
        }

        // Indented command output, then the result line.
        for line in last_output.lines() {
            block.push_str(&format!("      {line}\n"));
        }
        let result_line = match last_status {
            NodeStatus::Ok => format!(
                "  {} {}  {}\n",
                log::green(log::CHECK),
                step.name,
                log::dim(&format!("({})", fmt_duration(total)))
            ),
            NodeStatus::Errored => format!("  {} {}  errored\n", log::red(log::CROSS), step.name),
            _ => format!(
                "  {} {}  {}\n",
                log::red(log::CROSS),
                step.name,
                log::dim(&format!("failed after {attempt} attempt(s)"))
            ),
        };
        block.push_str(&result_line);

        // On failure, offer heuristic suggestions (Flux Assist, 2.12).
        if matches!(last_status, NodeStatus::Failed | NodeStatus::Errored) {
            let suggestions = crate::assist::diagnose(&command, &last_output);
            if !suggestions.is_empty() {
                block.push_str(&format!(
                    "      {}\n",
                    log::yellow("Flux assist — possible fixes:")
                ));
                for s in suggestions {
                    block.push_str(&format!("        {} {}\n", log::dim("•"), s.cause));
                    block.push_str(&format!("          {}\n", log::dim(&s.fix)));
                }
            }
        }

        NodeResult {
            idx,
            status: last_status,
            duration: total,
            block,
        }
    }
}

fn hook_line(step_name: &str, tool: &str) -> String {
    format!(
        "  {} {}  {}\n",
        log::yellow(log::DOT),
        step_name,
        log::dim(&format!(
            "'{tool}' tool hook (install the {tool} plugin to run)"
        ))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::Step;

    fn cmd(name: &str, needs: &[&str]) -> Step {
        let mut s = Step::command(name, "echo hi");
        s.needs = needs.iter().map(|s| s.to_string()).collect();
        s
    }

    #[test]
    fn linear_when_no_needs() {
        let steps = vec![cmd("a", &[]), cmd("b", &[]), cmd("c", &[])];
        let g = Graph::build(&steps).unwrap();
        assert!(!g.is_explicit());
        // Implicit chain a -> b -> c.
        assert_eq!(g.topo_order(), vec!["a", "b", "c"]);
    }

    #[test]
    fn diamond_dependencies_resolve() {
        let steps = vec![
            cmd("frontend", &[]),
            cmd("backend", &[]),
            cmd("tests", &["frontend", "backend"]),
            cmd("package", &["tests"]),
        ];
        let g = Graph::build(&steps).unwrap();
        assert!(g.is_explicit());
        let order = g.topo_order();
        // tests after both roots; package last.
        assert!(
            order.iter().position(|s| s == "tests") > order.iter().position(|s| s == "frontend")
        );
        assert!(
            order.iter().position(|s| s == "tests") > order.iter().position(|s| s == "backend")
        );
        assert_eq!(order.last().unwrap(), "package");
    }

    #[test]
    fn detects_cycles() {
        let steps = vec![cmd("a", &["b"]), cmd("b", &["a"])];
        let err = Graph::build(&steps).unwrap_err();
        assert!(err.0.contains("cycle"), "{}", err.0);
    }

    #[test]
    fn rejects_unknown_dependency() {
        let steps = vec![cmd("a", &["ghost"])];
        let err = Graph::build(&steps).unwrap_err();
        assert!(err.0.contains("unknown step"), "{}", err.0);
    }
}
