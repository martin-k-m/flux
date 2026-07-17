//! The abstract syntax tree produced by parsing a `.flux` file.

use std::collections::HashMap;

/// A fully parsed `.flux` configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FluxConfig {
    /// The declared project name, if any (`project "my-app"`).
    pub project: Option<String>,
    /// The declared language, if any (`language rust`).
    pub language: Option<String>,
    /// A build environment / container image, if declared.
    pub environment: Option<Environment>,
    /// Declared secret names (`secret DATABASE_URL`).
    pub secrets: Vec<String>,
    /// A deployment target, if declared.
    pub deployment: Option<Deployment>,
    /// Modules imported at the top level (`import rust-library`).
    pub imports: Vec<String>,
    /// Modules spliced into the pipeline (`use rust-library`), in order.
    pub uses: Vec<String>,
    /// Runner pools declared for scheduling (`runners { pool "gpu" { ... } }`).
    pub runner_pools: Vec<RunnerPool>,
    /// Organization policies (`policy production { require tests ... }`).
    pub policies: Vec<Policy>,
    /// The pipeline steps (order as written; execution order comes from the graph).
    pub steps: Vec<Step>,
}

/// An organization-wide policy that a pipeline must satisfy (Phase 4, 4.15).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Policy {
    /// Policy name, e.g. `production`.
    pub name: String,
    /// Require a test step in the pipeline.
    pub require_tests: bool,
    /// Require a security step (a `security`-named step or a `tool` hook).
    pub require_security: bool,
    /// Require at least this many approvals.
    pub require_approvals: u32,
}

/// A pool of runners with shared requirements (Phase 3, 3.1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunnerPool {
    /// Pool name, e.g. `gpu-builders`.
    pub name: String,
    /// Requires a GPU.
    pub gpu: Option<bool>,
    /// Minimum memory, as written (e.g. `32gb`).
    pub memory: Option<String>,
    /// Required OS (e.g. `linux`).
    pub os: Option<String>,
}

/// A build environment — a container image the pipeline runs inside.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Environment {
    /// The OCI image, e.g. `rust:latest`.
    pub image: Option<String>,
}

/// A deployment declaration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Deployment {
    /// Target: `local`, `docker`, `kubernetes`, `vm`, …
    pub target: Option<String>,
    /// Desired replica count (where meaningful).
    pub replicas: Option<u32>,
    /// Optional image to deploy.
    pub image: Option<String>,
}

/// A single pipeline step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// Step identifier, e.g. `build` or `test`.
    pub name: String,
    /// The shell command to execute, if this is a command step.
    pub command: Option<String>,
    /// An external tool hook (e.g. `tool scanner`) instead of a raw command.
    pub tool: Option<String>,
    /// Optional human description.
    pub description: Option<String>,
    /// Whether this step participates in the build cache. Defaults to `true`.
    pub cache: bool,
    /// Names of steps that must succeed before this one runs (`needs [...]`).
    pub needs: Vec<String>,
    /// A guard: the step only runs when this condition holds (`only_if`).
    pub only_if: Option<Condition>,
    /// How many times to retry the command on failure (default 0).
    pub retries: u32,
    /// Secret names to inject into the command's environment (`env: [...]`).
    pub env: Vec<String>,
    /// Glob patterns scoping this step's cache inputs (`inputs [ "src/**" ]`).
    /// When empty, the whole project is hashed (Phase 2 behaviour).
    pub inputs: Vec<String>,
    /// The runner pool this step prefers (`pool "gpu-builders"`).
    pub pool: Option<String>,
}

impl Step {
    /// Create a command-less step with cache enabled by default.
    pub fn new(name: impl Into<String>) -> Self {
        Step {
            name: name.into(),
            command: None,
            tool: None,
            description: None,
            cache: true,
            needs: Vec::new(),
            only_if: None,
            retries: 0,
            env: Vec::new(),
            inputs: Vec::new(),
            pool: None,
        }
    }

    /// Convenience constructor for a shell-command step.
    pub fn command(name: impl Into<String>, command: impl Into<String>) -> Self {
        let mut s = Step::new(name);
        s.command = Some(command.into());
        s
    }

    /// `true` when this step delegates to an external tool rather than a shell
    /// command (an external scanner, linter, or other plugin hook).
    pub fn is_hook(&self) -> bool {
        self.tool.is_some()
    }
}

/// A comparison operator used in `only_if` conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CondOp {
    Eq,
    Ne,
}

/// A single `only_if` condition, e.g. `branch == "main"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    /// The left-hand variable name (e.g. `branch`).
    pub var: String,
    /// The comparison operator.
    pub op: CondOp,
    /// The right-hand literal value.
    pub value: String,
}

impl Condition {
    /// Evaluate this condition against a set of variable bindings. An unknown
    /// variable compares as the empty string.
    pub fn evaluate(&self, vars: &HashMap<String, String>) -> bool {
        let lhs = vars.get(&self.var).map(String::as_str).unwrap_or("");
        match self.op {
            CondOp::Eq => lhs == self.value,
            CondOp::Ne => lhs != self.value,
        }
    }

    /// A human rendering, e.g. `branch == "main"`.
    pub fn describe(&self) -> String {
        let op = match self.op {
            CondOp::Eq => "==",
            CondOp::Ne => "!=",
        };
        format!("{} {} \"{}\"", self.var, op, self.value)
    }
}
