//! The `flux` command-line interface.
//!
//! This layer owns argument parsing (via clap), user-facing output, and the
//! translation of engine results into process exit codes. All heavy lifting
//! lives in [`crate::core`] and the platform modules.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::agent;
use crate::analytics;
use crate::artifacts::{self, PushSpec, Registry};
use crate::cache::Cache;
use crate::core::config::{self, FluxConfig};
use crate::core::detect::{self, Detection};
use crate::core::graph::{self, ExecCtx, Graph, GraphOutcome, NodeStatus};
use crate::core::logging as log;
use crate::core::pipeline::Pipeline;
use crate::core::runner::fmt_duration;
use crate::deploy;
use crate::integrations;
use crate::policy;
use crate::repro::{self, Lock};
use crate::runners::containers;
use crate::secrets::SecretStore;
use crate::tools;
use crate::workspace::Workspace;
use crate::VERSION_LABEL;

/// Flux — a local-first developer automation platform.
///
/// Point Flux at a project and it builds, tests, packages, and ships it from a
/// single `.flux` file. See <https://github.com/martin-k-m/flux>.
#[derive(Parser, Debug)]
#[command(
    name = "flux",
    version,
    about,
    long_about = None,
    after_help = "\
EXAMPLES:
  flux init rust-api        Scaffold a .flux from a template
  flux build                Run the pipeline (parallel dependency graph)
  flux test                 Run the test step(s)
  flux validate             Check the .flux for errors
  flux ci                   Clean, cache-free build; enforces policy
  flux workspace build      Build affected members of a workspace
  flux doctor               Diagnose the environment

Run `flux <command> --help` for details on any command."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Detect the project and write a starter `.flux` file.
    Init {
        /// Optional template: react, rust-api, library, cli, node-service.
        template: Option<String>,
        /// Overwrite an existing `.flux` file.
        #[arg(long)]
        force: bool,
    },
    /// Run the full build pipeline (dependency graph, parallel).
    Build,
    /// Run the pipeline's test step(s) and their dependencies.
    Test,
    /// Run a single named step (and its dependencies).
    Run {
        /// The step name to run (e.g. `build`).
        step: String,
    },
    /// Remove Flux's cache and artifacts.
    Clean,
    /// Show what Flux detects about this project.
    Info,
    /// Run a clean, cache-free CI pipeline and record an artifact.
    Ci,
    /// Deploy according to the `.flux` deployment block.
    Deploy {
        /// Override the deploy target (local, docker, kubernetes, vm).
        #[arg(long)]
        target: Option<String>,
    },
    /// Manage local build runners (agents).
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// List build runners and declared runner pools.
    Runners {
        #[command(subcommand)]
        action: RunnersAction,
    },
    /// Show build-performance analytics from run history.
    Analytics,
    /// Capture the current environment into `.flux.lock`.
    Lock,
    /// Compare the current environment against `.flux.lock` (reproducibility).
    Reproduce,
    /// Manage the artifact registry.
    Artifact {
        #[command(subcommand)]
        action: ArtifactAction,
    },
    /// Create a release from stored artifacts.
    Release {
        #[command(subcommand)]
        action: ReleaseAction,
    },
    /// Manage encrypted secrets.
    Secret {
        #[command(subcommand)]
        action: SecretAction,
    },
    /// Inspect, install, or scaffold plugins.
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    // ---- Phase 4: platform ----
    /// Manage a multi-project workspace.
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },
    /// Check the pipeline against declared policies.
    Policy,
    /// A one-screen overview of the project's Flux state.
    Status,
    /// Print the pipeline dependency graph.
    Graph,
    /// Format the project with its language formatter.
    Fmt,
    /// Lint the project with its language linter.
    Lint,
    /// Generate a changelog from git commits.
    Changelog,
    /// Bump the project version (major | minor | patch).
    Version { part: String },
    /// Inspect project dependencies.
    Deps,
    /// Diagnose the environment, toolchains, and Flux setup.
    Doctor {
        /// Run repository-wide health checks (CI, examples, docs, release config).
        #[arg(long)]
        all: bool,
    },
    /// Validate the `.flux` file (syntax, pipeline graph, references).
    Validate,
    /// Run the project's full check suite (fmt, clippy, tests).
    Verify {
        /// Also build in release mode.
        #[arg(long)]
        release: bool,
        /// Also validate example projects and build release.
        #[arg(long)]
        full: bool,
    },
    /// Explain the pipeline in plain language.
    Explain,
    /// Canonically format the `.flux` file in place.
    Format {
        /// Only check formatting; don't write. Exits non-zero if unformatted.
        #[arg(long)]
        check: bool,
    },
}

#[derive(Subcommand, Debug)]
enum WorkspaceAction {
    /// Show workspace members and which are affected by changes.
    Status,
    /// Build affected members in dependency order.
    Build,
}

#[derive(Subcommand, Debug)]
enum AgentAction {
    /// Register this machine as an available runner.
    Start,
    /// List registered runners.
    List,
}

#[derive(Subcommand, Debug)]
enum RunnersAction {
    /// List registered runners and declared pools.
    List,
}

#[derive(Subcommand, Debug)]
enum ArtifactAction {
    /// Push a file or directory into the registry.
    Push {
        /// Path to the artifact (file or directory).
        source: PathBuf,
        /// Artifact name (defaults to the project name).
        #[arg(long)]
        name: Option<String>,
        /// Version label (defaults to `dev`).
        #[arg(long)]
        version: Option<String>,
        /// Platform label (defaults to the host platform).
        #[arg(long)]
        platform: Option<String>,
    },
    /// List stored artifacts.
    List,
}

#[derive(Subcommand, Debug)]
enum ReleaseAction {
    /// Create a release bundling artifacts for a version.
    Create {
        /// The release version, e.g. `v1.0`.
        version: String,
    },
}

#[derive(Subcommand, Debug)]
enum SecretAction {
    /// Set (or overwrite) a secret value.
    Set {
        name: String,
        value: String,
        /// Target environment (e.g. development, production).
        #[arg(long, default_value = "default")]
        env: String,
    },
    /// List stored secret names (never values).
    List {
        /// Target environment.
        #[arg(long, default_value = "default")]
        env: String,
    },
}

#[derive(Subcommand, Debug)]
enum PluginAction {
    /// List the plugins Flux knows about.
    List,
    /// Install a plugin (registers it locally).
    Install { name: String },
    /// Scaffold a new plugin with the PDK.
    Create { name: String },
    /// Search the plugin catalog.
    Search { query: String },
    /// Verify installed plugin manifests.
    Verify,
}

/// Entry point invoked by `main`. Returns the process exit code.
pub fn run() -> i32 {
    log::init();
    let cli = Cli::parse();

    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{} {e}", log::red("error:"));
            return 2;
        }
    };

    let result = match cli.command {
        Command::Init { template, force } => cmd_init(&cwd, template, force),
        Command::Build => cmd_build(&cwd),
        Command::Test => cmd_test(&cwd),
        Command::Run { step } => cmd_run(&cwd, &step),
        Command::Clean => cmd_clean(&cwd),
        Command::Info => cmd_info(&cwd),
        Command::Ci => cmd_ci(&cwd),
        Command::Deploy { target } => cmd_deploy(&cwd, target),
        Command::Agent { action } => cmd_agent(&cwd, action),
        Command::Runners { action } => cmd_runners(&cwd, action),
        Command::Analytics => cmd_analytics(&cwd),
        Command::Lock => cmd_lock(&cwd),
        Command::Reproduce => cmd_reproduce(&cwd),
        Command::Artifact { action } => cmd_artifact(&cwd, action),
        Command::Release { action } => cmd_release(&cwd, action),
        Command::Secret { action } => cmd_secret(&cwd, action),
        Command::Plugin { action } => cmd_plugin(&cwd, action),
        Command::Workspace { action } => cmd_workspace(&cwd, action),
        Command::Policy => cmd_policy(&cwd),
        Command::Status => cmd_status(&cwd),
        Command::Graph => cmd_graph(&cwd),
        Command::Fmt => cmd_fmt(&cwd),
        Command::Lint => cmd_lint(&cwd),
        Command::Changelog => cmd_changelog(&cwd),
        Command::Version { part } => cmd_version(&cwd, &part),
        Command::Deps => cmd_deps(&cwd),
        Command::Doctor { all } => cmd_doctor(&cwd, all),
        Command::Validate => cmd_validate(&cwd),
        Command::Verify { release, full } => cmd_verify(&cwd, release, full),
        Command::Explain => cmd_explain(&cwd),
        Command::Format { check } => cmd_format(&cwd, check),
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{} {e}", log::red("error:"));
            2
        }
    }
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Load `.flux` if present, otherwise an empty config.
fn load_config(root: &Path) -> anyhow::Result<FluxConfig> {
    let path = root.join(config::CONFIG_FILE);
    if path.is_file() {
        config::load(&path)
    } else {
        Ok(FluxConfig::default())
    }
}

/// Resolve config, detection, and the pipeline in one shot.
fn load_context(root: &Path) -> anyhow::Result<(FluxConfig, Detection, Pipeline)> {
    let config = load_config(root)?;
    let detection = detect::detect(root);
    let pipeline = Pipeline::resolve(&config, &detection);
    Ok((config, detection, pipeline))
}

fn print_header(pipeline: &Pipeline) {
    log::banner(VERSION_LABEL);
    log::info_line(&log::dim("Detecting project..."));
    log::field("Language", &language_label(&pipeline.language));
    log::field("Project", &pipeline.project);
}

fn language_label(lang: &str) -> String {
    match lang {
        "rust" => "Rust".into(),
        "node" => "Node".into(),
        "python" => "Python".into(),
        "go" => "Go".into(),
        "java" => "Java".into(),
        other => other.to_string(),
    }
}

fn ensure_runnable(pipeline: &Pipeline) -> anyhow::Result<()> {
    if pipeline.steps.is_empty() {
        anyhow::bail!(
            "no pipeline found — this doesn't look like a supported project.\n       \
             Run `flux init` after adding a Cargo.toml, package.json, or requirements.txt."
        );
    }
    Ok(())
}

/// Build an execution context from config + the steps being run.
fn build_ctx(root: &Path, config: &FluxConfig, steps: &[config::Step], use_cache: bool) -> ExecCtx {
    let mut ctx = ExecCtx::new(root);
    ctx.use_cache = use_cache;
    ctx.vars = graph::build_vars(root);

    // Resolve any secrets referenced by the steps being run, from the active
    // environment (FLUX_ENV, defaulting to `default`).
    let names: Vec<String> = steps.iter().flat_map(|s| s.env.clone()).collect();
    if !names.is_empty() {
        let env = std::env::var("FLUX_ENV").unwrap_or_else(|_| "default".into());
        if let Ok(store) = SecretStore::open_env(root, &env) {
            ctx.secrets = store.resolve(&names);
        }
    }

    ctx.container_image = config.environment.as_ref().and_then(|e| e.image.clone());
    ctx
}

/// Build a graph from `steps` and execute it, printing engine banners.
fn execute_steps(
    root: &Path,
    config: &FluxConfig,
    steps: &[config::Step],
    use_cache: bool,
) -> anyhow::Result<GraphOutcome> {
    let graph = Graph::build(steps).map_err(|e| anyhow::anyhow!("invalid pipeline: {e}"))?;
    let ctx = build_ctx(root, config, steps, use_cache);

    if let Some(img) = &ctx.container_image {
        if containers::available() {
            log::field(
                "Environment",
                &format!("{img} (container: {})", containers::engine().unwrap_or("?")),
            );
        } else {
            log::field(
                "Environment",
                &format!("{img} — no engine found, running natively"),
            );
        }
    }
    if graph.is_explicit() {
        log::info_line(&log::dim(&format!(
            "  dependency graph · up to {} steps in parallel",
            ctx.max_parallel
        )));
        log::info_line(&log::dim(&format!(
            "  plan: {}",
            graph.topo_order().join(" \u{2192} ")
        )));
    }

    Ok(graph.execute(&ctx))
}

/// Print a compact per-step status recap.
fn print_summary(outcome: &GraphOutcome) {
    log::heading("Summary:");
    for record in &outcome.records {
        let (name, status) = (&record.name, &record.status);
        let (glyph, note) = match status {
            NodeStatus::Ok => (log::green(log::CHECK), ""),
            NodeStatus::Cached => (log::green(log::CHECK), "cached"),
            NodeStatus::Hook => (log::yellow(log::DOT), "hook"),
            NodeStatus::Conditional => (log::yellow(log::DOT), "skipped (condition)"),
            NodeStatus::Skipped => (log::yellow(log::DOT), "skipped (dependency failed)"),
            NodeStatus::Failed => (log::red(log::CROSS), "failed"),
            NodeStatus::Errored => (log::red(log::CROSS), "errored"),
        };
        if note.is_empty() {
            println!("  {glyph} {name}");
        } else {
            println!("  {glyph} {name}  {}", log::dim(note));
        }
    }
}

fn graph_exit(outcome: &GraphOutcome) -> i32 {
    if outcome.success {
        0
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// Build / test / run / ci
// ---------------------------------------------------------------------------

/// Detect sibling tools (Blink/Killer) and adjust the pipeline accordingly.
fn apply_integrations(root: &Path, steps: &mut Vec<config::Step>) {
    let siblings = integrations::detect(root);
    if siblings.has_blink() {
        log::ok_line("Blink project profile loaded — optimized pipeline selected");
    }
    if integrations::inject_killer(steps, &siblings) {
        log::ok_line("Killer configured — added an automatic security scan");
    }
}

fn cmd_build(root: &Path) -> anyhow::Result<i32> {
    let (config, _, mut pipeline) = load_context(root)?;
    ensure_runnable(&pipeline)?;

    print_header(&pipeline);
    apply_integrations(root, &mut pipeline.steps);
    log::heading("Pipeline:");

    let outcome = execute_steps(root, &config, &pipeline.steps, true)?;
    let _ = analytics::record(root, "build", &outcome);
    print_summary(&outcome);
    if outcome.success {
        println!(
            "\n{}",
            log::green(&format!(
                "Build completed in {}",
                fmt_duration(outcome.total)
            ))
        );
    } else {
        println!("\n{}", log::red("Build failed"));
    }
    Ok(graph_exit(&outcome))
}

fn cmd_test(root: &Path) -> anyhow::Result<i32> {
    let (config, _, pipeline) = load_context(root)?;
    let test_names: Vec<&str> = pipeline
        .steps
        .iter()
        .filter(|s| s.name.contains("test"))
        .map(|s| s.name.as_str())
        .collect();
    if test_names.is_empty() {
        anyhow::bail!("no test step found in this project's pipeline");
    }
    let steps = graph::select_with_deps(&pipeline.steps, &test_names);

    print_header(&pipeline);
    log::heading("Tests:");
    let outcome = execute_steps(root, &config, &steps, true)?;
    print_summary(&outcome);
    if outcome.success {
        println!("\n{}", log::green(&format!("{} Tests passed", log::CHECK)));
    } else {
        println!("\n{}", log::red(&format!("{} Tests failed", log::CROSS)));
    }
    Ok(graph_exit(&outcome))
}

fn cmd_run(root: &Path, step_name: &str) -> anyhow::Result<i32> {
    let (config, _, pipeline) = load_context(root)?;
    if !pipeline.steps.iter().any(|s| s.name == step_name) {
        let available: Vec<&str> = pipeline.steps.iter().map(|s| s.name.as_str()).collect();
        anyhow::bail!(
            "no step named '{step_name}'. Available steps: {}",
            if available.is_empty() {
                "(none)".to_string()
            } else {
                available.join(", ")
            }
        );
    }
    let steps = graph::select_with_deps(&pipeline.steps, &[step_name]);

    print_header(&pipeline);
    log::heading(&format!("Running '{step_name}':"));
    let outcome = execute_steps(root, &config, &steps, true)?;
    print_summary(&outcome);
    Ok(graph_exit(&outcome))
}

fn cmd_ci(root: &Path) -> anyhow::Result<i32> {
    let (config, detection, mut pipeline) = load_context(root)?;
    ensure_runnable(&pipeline)?;

    log::banner(&format!("{VERSION_LABEL}  ·  CI mode"));
    log::info_line(&log::dim("Clean environment · cache disabled"));
    log::field("Project", &pipeline.project);
    log::field("Language", &language_label(&pipeline.language));

    apply_integrations(root, &mut pipeline.steps);

    // Enforce declared policies before running anything (4.15).
    let violations = policy::evaluate(&config, policy::approvals_from_env());
    if !violations.is_empty() {
        log::heading("Policy violations:");
        for v in &violations {
            log::fail_line(&format!("[{}] {}", v.policy, v.message));
        }
        anyhow::bail!(
            "pipeline blocked by policy — fix the violations above or set FLUX_APPROVALS"
        );
    }

    // CI always starts from a clean build cache and never short-circuits.
    Cache::new(root).clear_builds()?;

    log::heading("Pipeline:");
    let outcome = execute_steps(root, &config, &pipeline.steps, false)?;
    let _ = analytics::record(root, "ci", &outcome);

    // Record a real artifact on success.
    let artifact = if outcome.success {
        record_ci_artifact(root, &pipeline).ok()
    } else {
        None
    };

    log::heading("CI Result:");
    log::field("Build", &pass_fail(outcome.success));
    let has_tests = pipeline.steps.iter().any(|s| s.name.contains("test"));
    let test_summary = if !has_tests {
        log::dim("no test step").to_string()
    } else if outcome.success {
        log::green("PASS").to_string()
    } else {
        log::red("did not pass").to_string()
    };
    log::field("Tests", &test_summary);
    log::field("Steps run", &outcome.ran().to_string());
    match &artifact {
        Some(a) => log::field(
            "Artifact",
            &log::cyan(&format!("{}/{} [{}]", a.name, a.platform, a.version)),
        ),
        None => log::field("Artifact", &log::dim("not produced")),
    }
    log::field("Toolchain", &pass_fail(detection.toolchain_available));

    Ok(graph_exit(&outcome))
}

/// Synthesize a build-info artifact and push it to the registry.
fn record_ci_artifact(root: &Path, pipeline: &Pipeline) -> anyhow::Result<artifacts::Artifact> {
    let tmp = root.join(".flux-cache").join("tmp");
    std::fs::create_dir_all(&tmp)?;
    let info = format!(
        "project = {}\nlanguage = {}\nsteps = {}\n",
        pipeline.project,
        pipeline.language,
        pipeline
            .steps
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );
    let info_path = tmp.join("build-info.txt");
    std::fs::write(&info_path, info)?;

    let reg = Registry::new(root);
    let art = reg.push(&PushSpec {
        name: pipeline.project.clone(),
        version: "ci".into(),
        platform: artifacts::host_platform(),
        source: info_path,
    })?;
    Ok(art)
}

// ---------------------------------------------------------------------------
// Deploy
// ---------------------------------------------------------------------------

fn cmd_deploy(root: &Path, target_override: Option<String>) -> anyhow::Result<i32> {
    let (config, _, pipeline) = load_context(root)?;
    let mut deployment = config.deployment.clone().unwrap_or_default();
    if let Some(t) = target_override {
        deployment.target = Some(t);
    }
    if deployment.target.is_none() {
        anyhow::bail!(
            "no deployment configured. Add a `deployment {{ target ... }}` block to .flux, \
             or pass --target."
        );
    }

    log::banner(&format!("{VERSION_LABEL}  ·  deploy"));
    log::field("Project", &pipeline.project);

    let result = deploy::deploy(root, &pipeline.project, &deployment);
    if result.success {
        println!(
            "\n{}",
            log::green(&format!("{} Deploy succeeded", log::CHECK))
        );
        Ok(0)
    } else if result.tool_missing {
        println!(
            "\n{}",
            log::yellow("Deploy could not complete — required tooling is not installed.")
        );
        Ok(1)
    } else {
        println!("\n{}", log::red("Deploy failed"));
        Ok(1)
    }
}

// ---------------------------------------------------------------------------
// Agent / runners
// ---------------------------------------------------------------------------

fn cmd_agent(root: &Path, action: AgentAction) -> anyhow::Result<i32> {
    match action {
        AgentAction::Start => {
            let runner = agent::register_self(root)?;
            log::banner(VERSION_LABEL);
            log::ok_line(&format!(
                "{} registered as a Flux runner",
                log::bold(&runner.name)
            ));
            print_runner(&runner);
            log::info_line(&format!(
                "\n  {}",
                log::dim("This machine now runs pipeline steps across its cores. Cross-machine")
            ));
            log::info_line(&format!(
                "  {}",
                log::dim("distribution (gRPC controller/agents) is planned for a later phase.")
            ));
            Ok(0)
        }
        AgentAction::List => {
            let runners = agent::list(root)?;
            log::banner(VERSION_LABEL);
            log::heading("Available Runners:");
            if runners.is_empty() {
                log::info_line(&format!(
                    "  {} none registered — run `flux agent start`",
                    log::dim(log::DOT)
                ));
            }
            for r in runners {
                println!("\n  {}", log::bold(&r.name));
                print_runner(&r);
            }
            Ok(0)
        }
    }
}

fn print_runner(r: &agent::Runner) {
    log::field("  CPU", &format!("{} cores", r.cpu_cores));
    match r.ram_mb {
        Some(mb) => log::field("  RAM", &format!("{} MB", mb)),
        None => log::field("  RAM", &log::dim("unknown").to_string()),
    }
    log::field("  Platform", &format!("{}-{}", r.os, r.arch));
}

fn cmd_runners(root: &Path, action: RunnersAction) -> anyhow::Result<i32> {
    match action {
        RunnersAction::List => {
            let runners = agent::list(root)?;
            let config = load_config(root).unwrap_or_default();

            log::banner(VERSION_LABEL);
            log::heading("Active Runners:");
            if runners.is_empty() {
                log::info_line(&format!(
                    "  {} none registered — run `flux agent start`",
                    log::dim(log::DOT)
                ));
            }
            for r in runners {
                println!("\n  {}  {}", log::bold(&r.name), log::green("online"));
                print_runner(&r);
            }

            if !config.runner_pools.is_empty() {
                log::heading("Declared Pools:");
                for pool in &config.runner_pools {
                    println!("  {} {}", log::cyan(log::DOT), log::bold(&pool.name));
                    if let Some(os) = &pool.os {
                        log::field("    os", os);
                    }
                    if let Some(gpu) = pool.gpu {
                        log::field("    gpu", if gpu { "required" } else { "no" });
                    }
                    if let Some(mem) = &pool.memory {
                        log::field("    memory", mem);
                    }
                }
                log::info_line(&format!(
                    "\n  {}",
                    log::dim("Pool-based scheduling across machines lands with the distributed runner network.")
                ));
            }
            Ok(0)
        }
    }
}

fn cmd_analytics(root: &Path) -> anyhow::Result<i32> {
    let a = analytics::analyze(root)?;
    log::banner(VERSION_LABEL);
    log::heading("Build Performance:");
    if a.runs == 0 {
        log::info_line(&format!(
            "  {} no history yet — run `flux build` a few times",
            log::dim(log::DOT)
        ));
        return Ok(0);
    }
    log::field("Runs recorded", &a.runs.to_string());
    log::field("Average build", &fmt_ms(a.avg_total_ms));
    log::field(
        "Cache hit rate",
        &format!("{:.0}%", a.cache_hit_rate() * 100.0),
    );
    if let Some((name, ms)) = a.expensive.first() {
        log::field("Most expensive step", &format!("{name} ({})", fmt_ms(*ms)));
    }
    log::field("Failures", &a.failures.to_string());
    Ok(0)
}

fn cmd_lock(root: &Path) -> anyhow::Result<i32> {
    let config = load_config(root)?;
    let lock = Lock::capture(root, &config);
    let path = repro::write(root, &lock)?;
    log::banner(VERSION_LABEL);
    log::ok_line(&format!("Environment locked → {}", path.display()));
    for (tool, ver) in &lock.tools {
        log::field(tool, ver);
    }
    if let Some(img) = &lock.environment_image {
        log::field("environment", img);
    }
    Ok(0)
}

fn cmd_reproduce(root: &Path) -> anyhow::Result<i32> {
    let locked = match repro::read(root)? {
        Some(l) => l,
        None => anyhow::bail!("no .flux.lock found — run `flux lock` first"),
    };
    let config = load_config(root)?;
    let current = Lock::capture(root, &config);

    log::banner(&format!("{VERSION_LABEL}  ·  reproduce"));
    let drift = locked.diff(&current);
    if drift.is_empty() {
        println!(
            "{}",
            log::green(&format!(
                "{} Environment matches .flux.lock — this build is reproducible",
                log::CHECK
            ))
        );
        Ok(0)
    } else {
        log::heading("Environment drift:");
        for d in &drift {
            log::fail_line(d);
        }
        println!(
            "\n{}",
            log::yellow("The current environment differs from the lock; results may not match.")
        );
        Ok(1)
    }
}

fn fmt_ms(ms: u128) -> String {
    if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}

// ---------------------------------------------------------------------------
// Artifacts / releases
// ---------------------------------------------------------------------------

fn cmd_artifact(root: &Path, action: ArtifactAction) -> anyhow::Result<i32> {
    let reg = Registry::new(root);
    match action {
        ArtifactAction::Push {
            source,
            name,
            version,
            platform,
        } => {
            if !source.exists() {
                anyhow::bail!("artifact source not found: {}", source.display());
            }
            let (_, _, pipeline) = load_context(root)?;
            let spec = PushSpec {
                name: name.unwrap_or(pipeline.project),
                version: version.unwrap_or_else(|| "dev".into()),
                platform: platform.unwrap_or_else(artifacts::host_platform),
                source,
            };
            let art = reg.push(&spec)?;
            log::banner(VERSION_LABEL);
            log::ok_line(&format!(
                "Pushed {} {} [{}] ({} bytes)",
                log::bold(&art.name),
                art.version,
                art.platform,
                art.bytes
            ));
            Ok(0)
        }
        ArtifactAction::List => {
            let list = reg.list()?;
            log::banner(VERSION_LABEL);
            log::heading("Artifacts:");
            if list.is_empty() {
                log::info_line(&format!(
                    "  {} none — run `flux artifact push <path>`",
                    log::dim(log::DOT)
                ));
            }
            // Group by name/version for a tree-like view.
            let mut current = String::new();
            let mut current_ver = String::new();
            for a in &list {
                if a.name != current {
                    println!("\n  {}", log::bold(&a.name));
                    current = a.name.clone();
                    current_ver.clear();
                }
                if a.version != current_ver {
                    println!("    {}", log::cyan(&a.version));
                    current_ver = a.version.clone();
                }
                println!(
                    "      {} {} {}",
                    log::dim("├──"),
                    a.platform,
                    log::dim(&format!("({} bytes)", a.bytes))
                );
            }
            Ok(0)
        }
    }
}

fn cmd_release(root: &Path, action: ReleaseAction) -> anyhow::Result<i32> {
    match action {
        ReleaseAction::Create { version } => {
            let (_, _, pipeline) = load_context(root)?;
            let reg = Registry::new(root);
            let downloads = reg.create_release(&pipeline.project, &version)?;
            log::banner(VERSION_LABEL);
            log::ok_line(&format!("Release {} created", log::bold(&version)));
            log::heading("Downloads:");
            if downloads.is_empty() {
                log::info_line(&format!(
                    "  {} no artifacts yet — push some with `flux artifact push`",
                    log::dim(log::DOT)
                ));
            }
            for d in downloads {
                println!(
                    "  {} {}-{} {}",
                    log::green(log::CHECK),
                    d.name,
                    d.platform,
                    log::dim(&format!("[{}]", d.version))
                );
            }
            Ok(0)
        }
    }
}

// ---------------------------------------------------------------------------
// Secrets
// ---------------------------------------------------------------------------

fn cmd_secret(root: &Path, action: SecretAction) -> anyhow::Result<i32> {
    match action {
        SecretAction::Set { name, value, env } => {
            let store = SecretStore::open_env(root, &env)?;
            store.set(&name, &value)?;
            log::banner(VERSION_LABEL);
            log::ok_line(&format!(
                "Secret {} stored (encrypted) in environment {}",
                log::bold(&name),
                log::cyan(&env)
            ));
            Ok(0)
        }
        SecretAction::List { env } => {
            let store = SecretStore::open_env(root, &env)?;
            let names = store.list()?;
            log::banner(VERSION_LABEL);
            log::heading(&format!("Secrets ({env}):"));
            if names.is_empty() {
                log::info_line(&format!("  {} none set", log::dim(log::DOT)));
            }
            for n in names {
                println!(
                    "  {} {}  {}",
                    log::green(log::CHECK),
                    n,
                    log::dim("(encrypted)")
                );
            }
            Ok(0)
        }
    }
}

// ---------------------------------------------------------------------------
// Plugins
// ---------------------------------------------------------------------------

fn cmd_plugin(root: &Path, action: PluginAction) -> anyhow::Result<i32> {
    match action {
        PluginAction::List => {
            log::banner(VERSION_LABEL);
            log::heading("Plugins:");
            for plugin in crate::plugins::registry() {
                let tag = if plugin.builtin() {
                    "built-in"
                } else {
                    "installed"
                };
                println!(
                    "  {} {}  {}  {}",
                    log::green(log::CHECK),
                    log::bold(plugin.id()),
                    log::dim(&format!("[{tag}]")),
                    plugin.description(),
                );
                println!(
                    "      {}",
                    log::dim(&format!("markers: {}", plugin.markers().join(", ")))
                );
            }
            for name in crate::plugins::installed(root) {
                println!(
                    "  {} {}  {}",
                    log::green(log::CHECK),
                    log::bold(&name),
                    log::dim("[installed]")
                );
            }
            log::info_line(&format!(
                "\n  {}",
                log::dim(
                    "Install more with `flux plugin install <name>` (e.g. aws, docker, terraform)"
                )
            ));
            Ok(0)
        }
        PluginAction::Install { name } => {
            crate::plugins::install(root, &name)?;
            log::banner(VERSION_LABEL);
            log::ok_line(&format!("Plugin {} installed", log::bold(&name)));
            log::info_line(&format!(
                "  {} recorded under .flux-cache/plugins/ — a future phase will load its behaviour",
                log::dim(log::DOT)
            ));
            Ok(0)
        }
        PluginAction::Create { name } => {
            let dir = crate::plugins::create(root, &name)?;
            log::banner(VERSION_LABEL);
            log::ok_line(&format!("Scaffolded plugin {} (PDK)", log::bold(&name)));
            log::info_line(&format!("  {}", log::dim(&dir.display().to_string())));
            for f in [
                "manifest.toml",
                "src/plugin.rs",
                "tests/plugin_test.rs",
                "README.md",
            ] {
                log::info_line(&format!("    {} {f}", log::green(log::CHECK)));
            }
            Ok(0)
        }
        PluginAction::Search { query } => {
            let results = crate::plugins::search(&query);
            log::banner(VERSION_LABEL);
            log::heading(&format!("Plugins matching '{query}':"));
            if results.is_empty() {
                log::info_line(&format!("  {} no matches", log::dim(log::DOT)));
            }
            for (name, category) in results {
                println!(
                    "  {} {}  {}",
                    log::green(log::CHECK),
                    log::bold(name),
                    log::dim(&format!("[{category}]"))
                );
            }
            log::info_line(&format!(
                "\n  {}",
                log::dim("Install with `flux plugin install <name>`.")
            ));
            Ok(0)
        }
        PluginAction::Verify => {
            let checks = crate::plugins::verify(root);
            log::banner(VERSION_LABEL);
            log::heading("Installed plugins:");
            if checks.is_empty() {
                log::info_line(&format!("  {} none installed", log::dim(log::DOT)));
                return Ok(0);
            }
            let mut bad = 0;
            for c in &checks {
                if c.ok {
                    log::ok_line(&format!("{}  {}", c.name, log::dim(&c.detail)));
                } else {
                    bad += 1;
                    log::fail_line(&format!("{}  {}", c.name, log::dim(&c.detail)));
                }
            }
            Ok(if bad == 0 { 0 } else { 1 })
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 4: workspace / policy / status / graph / dev tools
// ---------------------------------------------------------------------------

fn cmd_workspace(root: &Path, action: WorkspaceAction) -> anyhow::Result<i32> {
    let ws = Workspace::load(root)?.ok_or_else(|| {
        anyhow::anyhow!(
            "no {} found in this directory",
            crate::workspace::WORKSPACE_FILE
        )
    })?;
    let ordered = ws.ordered()?;
    let affected = ws.affected(root)?;

    match action {
        WorkspaceAction::Status => {
            log::banner(VERSION_LABEL);
            log::heading(&format!("Workspace: {}", ws.name));
            for m in &ordered {
                let state = if affected.contains(&m.name) {
                    log::yellow("affected")
                } else {
                    log::dim("unchanged")
                };
                let deps = if m.needs.is_empty() {
                    String::new()
                } else {
                    log::dim(&format!("  needs: {}", m.needs.join(", ")))
                };
                println!(
                    "  {} {}  {state}{deps}",
                    log::cyan(log::DOT),
                    log::bold(&m.name)
                );
            }
            Ok(0)
        }
        WorkspaceAction::Build => {
            log::banner(&format!("{VERSION_LABEL}  ·  workspace build"));
            log::field("Workspace", &ws.name);
            log::info_line(&log::dim(&format!(
                "  {} of {} members affected",
                affected.len(),
                ordered.len()
            )));

            let mut all_ok = true;
            for m in &ordered {
                if !affected.contains(&m.name) {
                    println!(
                        "  {} {}  {}",
                        log::green(log::CHECK),
                        m.name,
                        log::dim("skipped (unchanged)")
                    );
                    continue;
                }
                let member_root = ws.member_path(root, m);
                log::heading(&format!("→ {} ({})", m.name, m.path));
                let (config, _, pipeline) = match load_context(&member_root) {
                    Ok(v) => v,
                    Err(e) => {
                        log::fail_line(&format!("{}: {e}", m.name));
                        all_ok = false;
                        break;
                    }
                };
                if pipeline.steps.is_empty() {
                    log::info_line(&log::dim("  no pipeline — skipped"));
                    continue;
                }
                let outcome = execute_steps(&member_root, &config, &pipeline.steps, true)?;
                if !outcome.success {
                    log::fail_line(&format!("member '{}' failed", m.name));
                    all_ok = false;
                    break;
                }
            }

            if all_ok {
                ws.record_hashes(root)?;
                println!(
                    "\n{}",
                    log::green(&format!("{} Workspace build succeeded", log::CHECK))
                );
                Ok(0)
            } else {
                println!("\n{}", log::red("Workspace build failed"));
                Ok(1)
            }
        }
    }
}

fn cmd_policy(root: &Path) -> anyhow::Result<i32> {
    let config = load_config(root)?;
    log::banner(VERSION_LABEL);
    if config.policies.is_empty() {
        log::info_line(&format!("  {} no policies declared", log::dim(log::DOT)));
        return Ok(0);
    }
    let violations = policy::evaluate(&config, policy::approvals_from_env());
    log::heading("Policy check:");
    if violations.is_empty() {
        log::ok_line("all policies satisfied");
        Ok(0)
    } else {
        for v in &violations {
            log::fail_line(&format!("[{}] {}", v.policy, v.message));
        }
        Ok(1)
    }
}

fn cmd_status(root: &Path) -> anyhow::Result<i32> {
    let detection = detect::detect(root);
    let config = load_config(root).unwrap_or_default();
    log::banner(VERSION_LABEL);
    log::heading("Status:");
    log::field(
        "Project",
        &config
            .project
            .clone()
            .or_else(|| detection.name.clone())
            .unwrap_or_else(|| "(unknown)".into()),
    );
    log::field("Language", &detection.language_label());
    log::field("Steps", &config.steps.len().to_string());
    log::field("Policies", &config.policies.len().to_string());
    if let Ok(Some(ws)) = Workspace::load(root) {
        log::field(
            "Workspace",
            &format!("{} ({} members)", ws.name, ws.members.len()),
        );
    }
    let siblings = integrations::detect(root);
    log::field(
        "Siblings",
        &format!(
            "Blink {}, Killer {}",
            yes_no(siblings.has_blink()),
            yes_no(siblings.has_killer())
        ),
    );
    let a = analytics::analyze(root).unwrap_or_default();
    if a.runs > 0 {
        log::field("Runs recorded", &a.runs.to_string());
    }
    Ok(0)
}

fn cmd_graph(root: &Path) -> anyhow::Result<i32> {
    let (_, _, pipeline) = load_context(root)?;
    ensure_runnable(&pipeline)?;
    log::banner(VERSION_LABEL);
    log::heading("Pipeline graph:");
    for step in &pipeline.steps {
        if step.needs.is_empty() {
            println!("  {} {}", log::cyan(log::DOT), log::bold(&step.name));
        } else {
            println!(
                "  {} {}  {}",
                log::cyan(log::DOT),
                log::bold(&step.name),
                log::dim(&format!("needs {}", step.needs.join(", ")))
            );
        }
    }
    if let Ok(graph) = crate::core::graph::Graph::build(&pipeline.steps) {
        log::info_line(&format!(
            "\n  {}",
            log::dim(&format!(
                "execution order: {}",
                graph.topo_order().join(" \u{2192} ")
            ))
        ));
    }
    Ok(0)
}

fn cmd_fmt(root: &Path) -> anyhow::Result<i32> {
    let language = require_language(root)?;
    log::banner(VERSION_LABEL);
    match tools::fmt(root, &language) {
        tools::ToolOutcome::Ran { success } if success => {
            log::ok_line("Formatting complete");
            Ok(0)
        }
        tools::ToolOutcome::Ran { .. } => {
            log::fail_line("Formatter reported problems");
            Ok(1)
        }
        tools::ToolOutcome::NoCommand => {
            log::fail_line(&format!(
                "no formatter available for {language} (would run: {})",
                tools::fmt_command(&language).unwrap_or("n/a")
            ));
            Ok(1)
        }
    }
}

fn cmd_lint(root: &Path) -> anyhow::Result<i32> {
    let language = require_language(root)?;
    log::banner(VERSION_LABEL);
    match tools::lint(root, &language) {
        tools::ToolOutcome::Ran { success } if success => {
            log::ok_line("Lint passed");
            Ok(0)
        }
        tools::ToolOutcome::Ran { .. } => {
            log::fail_line("Lint found issues");
            Ok(1)
        }
        tools::ToolOutcome::NoCommand => {
            log::fail_line(&format!(
                "no linter available for {language} (would run: {})",
                tools::lint_command(&language).unwrap_or("n/a")
            ));
            Ok(1)
        }
    }
}

fn cmd_changelog(root: &Path) -> anyhow::Result<i32> {
    let md = tools::changelog::generate(root)?;
    print!("{md}");
    Ok(0)
}

fn cmd_version(root: &Path, part: &str) -> anyhow::Result<i32> {
    let part = tools::version::Part::parse(part).ok_or_else(|| {
        anyhow::anyhow!("unknown version part '{part}' (use major, minor, or patch)")
    })?;
    let language = require_language(root)?;
    let (old, new) = tools::version::bump_project(root, &language, part)?;
    log::banner(VERSION_LABEL);
    log::ok_line(&format!(
        "Version bumped {} → {}",
        log::dim(&old),
        log::bold(&new)
    ));
    Ok(0)
}

fn cmd_deps(root: &Path) -> anyhow::Result<i32> {
    let language = require_language(root)?;
    let report = tools::deps::inspect(root, &language)?;
    log::banner(VERSION_LABEL);
    log::heading("Dependencies:");
    log::field("Total", &report.total.to_string());
    log::field(
        "Duplicates",
        &if report.duplicates.is_empty() {
            "0".to_string()
        } else {
            format!(
                "{} ({})",
                report.duplicates.len(),
                report.duplicates.join(", ")
            )
        },
    );
    log::info_line(&format!(
        "  {} {}",
        log::dim(log::DOT),
        log::dim(report.outdated_note())
    ));
    Ok(0)
}

fn cmd_doctor(root: &Path, all: bool) -> anyhow::Result<i32> {
    let detection = detect::detect(root);
    let mut checks = tools::doctor::run(root, &detection);
    if all {
        checks.extend(tools::doctor::repository_checks(root));
    }
    log::banner(&format!("{VERSION_LABEL}  ·  doctor"));
    log::heading(if all { "Repository Health:" } else { "Checks:" });
    let mut failures = 0;
    for c in &checks {
        if c.ok {
            log::ok_line(&format!("{}  {}", c.name, log::dim(&c.detail)));
        } else {
            failures += 1;
            log::fail_line(&format!("{}  {}", c.name, log::dim(&c.detail)));
        }
    }
    let total = checks.len().max(1);
    let health = ((total - failures) * 100) / total;
    if failures == 0 {
        println!(
            "\n{}  {}",
            log::green(&format!("{} Everything looks healthy", log::CHECK)),
            log::dim(&format!("({health}%)"))
        );
        Ok(0)
    } else {
        println!(
            "\n{}  {}",
            log::yellow(&format!("{failures} check(s) need attention")),
            log::dim(&format!("(health {health}%)"))
        );
        Ok(1)
    }
}

fn cmd_validate(root: &Path) -> anyhow::Result<i32> {
    log::banner(&format!("{VERSION_LABEL}  ·  validate"));
    let path = root.join(config::CONFIG_FILE);
    if !path.is_file() {
        log::fail_line("no .flux file found — run `flux init`");
        return Ok(1);
    }

    // Syntax + module resolution.
    let config = match config::load(&path) {
        Ok(c) => c,
        Err(e) => {
            log::fail_line(&format!("{e}"));
            return Ok(1);
        }
    };

    // Semantic checks: the pipeline graph must be valid (no cycles / unknown needs).
    if !config.steps.is_empty() {
        if let Err(e) = Graph::build(&config.steps) {
            log::fail_line(&format!("invalid pipeline: {e}"));
            return Ok(1);
        }
    }

    log::ok_line(".flux is valid");
    log::field("Project", config.project.as_deref().unwrap_or("(detected)"));
    log::field(
        "Language",
        config.language.as_deref().unwrap_or("(detected)"),
    );
    log::field("Steps", &config.steps.len().to_string());
    if !config.policies.is_empty() {
        log::field("Policies", &config.policies.len().to_string());
    }
    if !config.secrets.is_empty() {
        log::field("Secrets declared", &config.secrets.len().to_string());
    }
    Ok(0)
}

/// `flux verify` — run the project's full check suite (6.2).
fn cmd_verify(root: &Path, release: bool, full: bool) -> anyhow::Result<i32> {
    let language = require_language(root)?;
    log::banner(&format!("{VERSION_LABEL}  ·  verify"));

    // Language-appropriate checks.
    let mut checks: Vec<(&str, String)> = match language.as_str() {
        "rust" => vec![
            ("format", "cargo fmt --all -- --check".into()),
            ("clippy", "cargo clippy --all-targets -- -D warnings".into()),
            ("tests", "cargo test".into()),
        ],
        "node" => vec![
            ("lint", "npx --yes eslint .".into()),
            ("tests", "npm test".into()),
        ],
        "python" => vec![("tests", "python -m unittest discover".into())],
        other => anyhow::bail!("`flux verify` doesn't know how to check '{other}' projects yet"),
    };
    if (release || full) && language == "rust" {
        checks.push(("release build", "cargo build --release".into()));
    }

    let mut failed = 0;
    for (name, cmd) in &checks {
        log::info_line(&format!(
            "  {} {}  {}",
            log::cyan(log::ARROW),
            name,
            log::dim(cmd)
        ));
        match crate::runners::shell::run(cmd, root) {
            Ok(r) if r.success => log::ok_line(name),
            _ => {
                log::fail_line(&format!("{name} failed"));
                failed += 1;
            }
        }
    }

    // `--full` also validates every example project.
    if full {
        log::heading("Examples:");
        let examples = root.join("examples");
        if let Ok(entries) = std::fs::read_dir(&examples) {
            for e in entries.flatten() {
                let p = e.path();
                let flux = p.join(config::CONFIG_FILE);
                if !flux.is_file() {
                    continue;
                }
                let name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string();
                match validate_flux(&p) {
                    Ok(()) => log::ok_line(&name),
                    Err(e) => {
                        log::fail_line(&format!("{name}: {e}"));
                        failed += 1;
                    }
                }
            }
        }
    }

    if failed == 0 {
        println!("\n{}", log::green(&format!("{} Verify passed", log::CHECK)));
        Ok(0)
    } else {
        println!(
            "\n{}",
            log::red(&format!("{} {failed} check(s) failed", log::CROSS))
        );
        Ok(1)
    }
}

/// Parse + graph-validate a `.flux` in `dir` without printing.
fn validate_flux(dir: &Path) -> anyhow::Result<()> {
    let cfg = config::load(&dir.join(config::CONFIG_FILE))?;
    if !cfg.steps.is_empty() {
        Graph::build(&cfg.steps).map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    Ok(())
}

/// `flux explain` — describe the pipeline in plain language (6.12).
fn cmd_explain(root: &Path) -> anyhow::Result<i32> {
    let (_, _, pipeline) = load_context(root)?;
    ensure_runnable(&pipeline)?;
    let graph =
        Graph::build(&pipeline.steps).map_err(|e| anyhow::anyhow!("invalid pipeline: {e}"))?;

    log::banner(&format!("{VERSION_LABEL}  ·  explain"));
    println!(
        "\n{} is a {} project. Its pipeline has {} step(s){}.\n",
        log::bold(&pipeline.project),
        language_label(&pipeline.language),
        pipeline.steps.len(),
        if graph.is_explicit() {
            ", run as a dependency graph (independent steps in parallel)"
        } else {
            ", run in order"
        }
    );
    for name in graph.topo_order() {
        if let Some(step) = pipeline.steps.iter().find(|s| s.name == name) {
            print!("  {} {}", log::cyan(log::DOT), log::bold(&step.name));
            if let Some(cmd) = &step.command {
                print!(" runs {}", log::dim(&format!("`{cmd}`")));
            } else if let Some(tool) = &step.tool {
                print!(" invokes the {} tool", log::dim(tool));
            }
            if !step.needs.is_empty() {
                print!(" after {}", step.needs.join(", "));
            }
            if let Some(cond) = &step.only_if {
                print!(", only if {}", cond.describe());
            }
            if step.retries > 0 {
                print!(", retrying up to {} time(s)", step.retries);
            }
            println!(".");
        }
    }
    Ok(0)
}

/// `flux format` — canonically format the `.flux` file (6.12).
fn cmd_format(root: &Path, check: bool) -> anyhow::Result<i32> {
    let path = root.join(config::CONFIG_FILE);
    let src = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("could not read {}: {e}", path.display()))?;
    // Format the raw file (not module-resolved), so `use` and structure survive.
    let cfg = config::parse(&src).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let formatted = render_config(&cfg);

    log::banner(&format!("{VERSION_LABEL}  ·  format"));
    if src == formatted {
        log::ok_line(".flux is already formatted");
        return Ok(0);
    }
    if check {
        log::fail_line(".flux is not formatted (run `flux format`)");
        return Ok(1);
    }
    std::fs::write(&path, formatted)?;
    log::ok_line("Formatted .flux");
    Ok(0)
}

/// Resolve the project language or bail with a helpful message.
fn require_language(root: &Path) -> anyhow::Result<String> {
    let config = load_config(root).unwrap_or_default();
    config
        .language
        .or_else(|| detect::detect(root).language)
        .ok_or_else(|| anyhow::anyhow!("could not determine the project language"))
}

fn yes_no(b: bool) -> String {
    if b {
        log::green("yes")
    } else {
        log::dim("no").to_string()
    }
}

// ---------------------------------------------------------------------------
// Clean / info / init
// ---------------------------------------------------------------------------

fn cmd_clean(root: &Path) -> anyhow::Result<i32> {
    Cache::new(root).clear_builds()?;
    log::banner(VERSION_LABEL);
    log::ok_line("Cleared the Flux build cache (forces a full rebuild)");
    log::info_line(&log::dim(
        "  Secrets, artifacts, runners, and analytics are preserved. Language build outputs (e.g. target/) are untouched.",
    ));
    Ok(0)
}

fn cmd_info(root: &Path) -> anyhow::Result<i32> {
    let detection = detect::detect(root);
    let config = load_config(root).ok();

    log::banner(VERSION_LABEL);
    log::heading("Project:");
    let name = config
        .as_ref()
        .and_then(|c| c.project.clone())
        .or_else(|| detection.name.clone())
        .unwrap_or_else(|| "(unknown)".into());
    log::field("Name", &name);
    log::field("Language", &detection.language_label());

    log::heading("Detected:");
    if detection.markers.is_empty() {
        log::fail_line("no known project markers found");
    } else {
        for (file, _) in &detection.markers {
            log::ok_line(file);
        }
    }
    if detection.toolchain_available {
        log::ok_line(&format!(
            "{} toolchain available",
            detection.language_label()
        ));
    } else if detection.language.is_some() {
        log::fail_line(&format!(
            "{} toolchain not found on PATH",
            detection.language_label()
        ));
    }
    if detection.has_tests {
        log::ok_line("Tests found");
    } else {
        log::info_line(&format!("  {} no tests detected", log::dim(log::DOT)));
    }

    log::heading("Platform:");
    match containers::engine() {
        Some(e) => log::ok_line(&format!("container engine: {e}")),
        None => log::info_line(&format!(
            "  {} no container engine (docker/podman)",
            log::dim(log::DOT)
        )),
    }
    let runners = agent::list(root).unwrap_or_default();
    log::field("Runners", &runners.len().to_string());

    let flux_present = root.join(config::CONFIG_FILE).is_file();
    log::heading("Flux config:");
    if flux_present {
        log::ok_line(".flux present");
    } else {
        log::info_line(&format!(
            "  {} no .flux file — run `flux init` to create one",
            log::dim(log::DOT)
        ));
    }
    Ok(0)
}

fn cmd_init(root: &Path, template: Option<String>, force: bool) -> anyhow::Result<i32> {
    let path = root.join(config::CONFIG_FILE);
    if path.exists() && !force {
        anyhow::bail!(".flux already exists (use --force to overwrite)");
    }

    let project = detect::detect(root)
        .name
        .or_else(|| dir_name(root))
        .unwrap_or_else(|| "my-app".into());

    // A named template (4.6) uses a curated pipeline; otherwise detect.
    let (language, contents) = match &template {
        Some(name) => match template_config(name, &project) {
            Some(cfg) => (template_language(name).to_string(), cfg),
            None => anyhow::bail!(
                "unknown template '{name}' (available: react, node-service, rust-api, library, cli)"
            ),
        },
        None => {
            let detection = detect::detect(root);
            let language = detection.language.clone().ok_or_else(|| {
                anyhow::anyhow!(
                    "could not detect a supported project (looked for Cargo.toml, package.json, requirements.txt, ...).\n       \
                     Try a template, e.g. `flux init rust-api`."
                )
            })?;
            let steps = crate::runners::default_steps(&language).unwrap_or_default();
            (
                language.clone(),
                generate_config(&project, &language, &steps),
            )
        }
    };

    std::fs::write(&path, contents)?;

    log::banner(VERSION_LABEL);
    println!("{}", log::bold("Flux configured:"));
    if let Some(name) = &template {
        log::ok_line(&format!("Template '{}' applied", log::bold(name)));
    }
    log::ok_line(&format!(
        "{} build pipeline created",
        language_label(&language)
    ));
    log::ok_line("Test runner configured");
    log::ok_line("Cache enabled");
    log::info_line(&format!(
        "\n  wrote {}",
        log::cyan(&path.display().to_string())
    ));
    Ok(0)
}

/// The language a template targets.
fn template_language(name: &str) -> &'static str {
    match name {
        "react" | "node-service" => "node",
        "rust-api" | "library" | "cli" => "rust",
        _ => "rust",
    }
}

/// Curated `.flux` contents for a named template (4.6). Returns `None` for
/// unknown templates.
fn template_config(name: &str, project: &str) -> Option<String> {
    let body = match name {
        "react" => {
            "language node\n\npipeline {\n\
             \x20   step dependencies { command \"npm install\" }\n\
             \x20   step lint  { needs dependencies command \"npm run lint\" }\n\
             \x20   step build { needs dependencies command \"npm run build\" inputs [ \"src/**\" ] }\n\
             \x20   step test  { needs build command \"npm test\" }\n}\n\n\
             deployment { target static }\n"
        }
        "node-service" => {
            "language node\n\nenvironment { image \"node:latest\" }\n\npipeline {\n\
             \x20   step dependencies { command \"npm install\" }\n\
             \x20   step build { needs dependencies command \"npm run build\" }\n\
             \x20   step test  { needs build command \"npm test\" }\n}\n\n\
             deployment { target docker replicas 2 }\n"
        }
        "rust-api" => {
            "language rust\n\nenvironment { image \"rust:latest\" }\n\npipeline {\n\
             \x20   step dependencies { command \"cargo fetch\" }\n\
             \x20   step build { needs dependencies command \"cargo build --release\" inputs [ \"src/**\" ] }\n\
             \x20   step test  { needs build command \"cargo test\" }\n}\n\n\
             deployment { target kubernetes replicas 3 }\n"
        }
        "library" => {
            "language rust\n\npipeline {\n\
             \x20   step build { command \"cargo build --release\" }\n\
             \x20   step test  { needs build command \"cargo test\" }\n\
             \x20   step docs  { needs build command \"cargo doc --no-deps\" }\n}\n"
        }
        "cli" => {
            "language rust\n\npipeline {\n\
             \x20   step build { command \"cargo build --release\" inputs [ \"src/**\" ] }\n\
             \x20   step test  { needs build command \"cargo test\" }\n}\n"
        }
        _ => return None,
    };
    Some(format!("project \"{project}\"\n{body}"))
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn pass_fail(ok: bool) -> String {
    if ok {
        log::green("PASS")
    } else {
        log::red("FAIL")
    }
}

/// Render a parsed config back to canonical `.flux` text (used by `flux format`).
fn render_config(cfg: &FluxConfig) -> String {
    let mut out = String::new();
    if let Some(p) = &cfg.project {
        out.push_str(&format!("project \"{p}\"\n"));
    }
    if let Some(l) = &cfg.language {
        out.push_str(&format!("language {l}\n"));
    }
    for i in &cfg.imports {
        out.push_str(&format!("import {i}\n"));
    }
    if let Some(env) = &cfg.environment {
        if let Some(img) = &env.image {
            out.push_str(&format!("\nenvironment {{ image \"{img}\" }}\n"));
        }
    }
    for s in &cfg.secrets {
        out.push_str(&format!("secret {s}\n"));
    }

    if !cfg.uses.is_empty() || !cfg.steps.is_empty() {
        out.push_str("\npipeline {\n");
        for u in &cfg.uses {
            out.push_str(&format!("    use {u}\n"));
        }
        for step in &cfg.steps {
            out.push_str(&format!("    step {} {{\n", step.name));
            if let Some(d) = &step.description {
                out.push_str(&format!("        description \"{d}\"\n"));
            }
            if let Some(c) = &step.command {
                out.push_str(&format!("        command \"{c}\"\n"));
            }
            if let Some(t) = &step.tool {
                out.push_str(&format!("        tool {t}\n"));
            }
            if !step.needs.is_empty() {
                out.push_str(&format!("        needs [ {} ]\n", step.needs.join(", ")));
            }
            if !step.inputs.is_empty() {
                let quoted: Vec<String> = step.inputs.iter().map(|i| format!("\"{i}\"")).collect();
                out.push_str(&format!("        inputs [ {} ]\n", quoted.join(", ")));
            }
            if !step.env.is_empty() {
                out.push_str(&format!("        env [ {} ]\n", step.env.join(", ")));
            }
            if let Some(cond) = &step.only_if {
                out.push_str(&format!("        only_if {}\n", cond.describe()));
            }
            if step.retries > 0 {
                out.push_str(&format!("        retries {}\n", step.retries));
            }
            if let Some(pool) = &step.pool {
                out.push_str(&format!("        pool \"{pool}\"\n"));
            }
            if !step.cache {
                out.push_str("        cache off\n");
            }
            out.push_str("    }\n");
        }
        out.push_str("}\n");
    }

    if let Some(dep) = &cfg.deployment {
        out.push_str("\ndeployment {");
        if let Some(t) = &dep.target {
            out.push_str(&format!(" target {t}"));
        }
        if let Some(r) = dep.replicas {
            out.push_str(&format!(" replicas {r}"));
        }
        if let Some(img) = &dep.image {
            out.push_str(&format!(" image \"{img}\""));
        }
        out.push_str(" }\n");
    }

    for policy in &cfg.policies {
        out.push_str(&format!("\npolicy {} {{\n", policy.name));
        if policy.require_tests {
            out.push_str("    require tests\n");
        }
        if policy.require_security {
            out.push_str("    require security\n");
        }
        if policy.require_approvals > 0 {
            out.push_str(&format!(
                "    require approvals {}\n",
                policy.require_approvals
            ));
        }
        out.push_str("}\n");
    }

    out
}

/// Generate `.flux` file contents from resolved defaults, advertising the
/// Phase 2 features as commented examples.
fn generate_config(project: &str, language: &str, steps: &[config::Step]) -> String {
    let mut out = String::new();
    out.push_str(&format!("project \"{project}\"\n"));
    out.push_str(&format!("language {language}\n\n"));

    out.push_str("# Run the build inside a container image (needs docker or podman):\n");
    out.push_str(&format!(
        "# environment {{ image \"{language}:latest\" }}\n\n"
    ));

    out.push_str("pipeline {\n");
    for step in steps {
        out.push_str(&format!("    step {} {{\n", step.name));
        if let Some(desc) = &step.description {
            out.push_str(&format!("        description \"{desc}\"\n"));
        }
        if let Some(cmd) = &step.command {
            out.push_str(&format!("        command \"{cmd}\"\n"));
        }
        if let Some(tool) = &step.tool {
            out.push_str(&format!("        tool {tool}\n"));
        }
        out.push_str("    }\n\n");
    }
    out.push_str("    # Security scanning is handled by Killer. Uncomment to enable:\n");
    out.push_str("    # step security { tool killer }\n\n");
    out.push_str("    # Steps can depend on each other, retry, and run conditionally:\n");
    out.push_str("    # step deploy {\n");
    out.push_str("    #     needs [ test ]\n");
    out.push_str("    #     command \"./deploy.sh\"\n");
    out.push_str("    #     only_if branch == \"main\"\n");
    out.push_str("    #     retries 2\n");
    out.push_str("    #     env [ DATABASE_URL ]\n");
    out.push_str("    # }\n");
    out.push_str("}\n\n");

    out.push_str(
        "# secret DATABASE_URL            # set with: flux secret set DATABASE_URL <value>\n",
    );
    out.push_str("# deployment { target kubernetes replicas 3 }\n");
    out
}

/// The final path component of `root`, if any.
fn dir_name(root: &Path) -> Option<String> {
    root.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
}
