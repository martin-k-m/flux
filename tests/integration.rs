//! End-to-end tests that drive the compiled `flux` binary against throwaway
//! project directories.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Path to the freshly built binary (provided by Cargo for integration tests).
fn flux() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flux"));
    // Keep output deterministic for assertions.
    cmd.env("NO_COLOR", "1");
    cmd
}

/// Create a unique, empty temp directory for one test.
fn temp_project(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("flux-it-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents).unwrap();
}

fn run(dir: &Path, args: &[&str]) -> (String, bool) {
    let out = flux().args(args).current_dir(dir).output().unwrap();
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    (s, out.status.success())
}

#[test]
fn init_detects_rust_and_writes_config() {
    let dir = temp_project("init");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"widget\"\nversion = \"0.1.0\"\n",
    );

    let (out, ok) = run(&dir, &["init"]);
    assert!(ok, "init should succeed: {out}");
    assert!(out.contains("Flux configured"), "{out}");

    let cfg = std::fs::read_to_string(dir.join(".flux")).unwrap();
    assert!(cfg.contains("project \"widget\""), "{cfg}");
    assert!(cfg.contains("language rust"), "{cfg}");
    assert!(cfg.contains("cargo build --release"), "{cfg}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_runs_pipeline_then_caches() {
    let dir = temp_project("build");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"c\"\nversion = \"0.1.0\"\n",
    );
    write(&dir, "src.txt", "hello");
    write(
        &dir,
        ".flux",
        "project \"c\"\nlanguage rust\npipeline {\n  step build { command \"echo compiling\" }\n}\n",
    );

    let (first, ok1) = run(&dir, &["build"]);
    assert!(ok1, "first build should succeed: {first}");
    assert!(
        first.contains("compiling"),
        "should run the command: {first}"
    );
    assert!(first.contains("Build completed"), "{first}");

    let (second, ok2) = run(&dir, &["build"]);
    assert!(ok2, "second build should succeed: {second}");
    assert!(
        second.contains("cached"),
        "second build should be cached: {second}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn failing_step_stops_pipeline_with_nonzero_exit() {
    let dir = temp_project("fail");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"f\"\nversion = \"0.1.0\"\n",
    );
    write(
        &dir,
        ".flux",
        "project \"f\"\nlanguage rust\npipeline {\n  step build { command \"exit 3\" }\n  step test { command \"echo should-not-run\" }\n}\n",
    );

    let (out, ok) = run(&dir, &["build"]);
    assert!(!ok, "build should fail: {out}");
    assert!(out.contains("Build failed"), "{out}");
    assert!(
        !out.contains("should-not-run"),
        "later steps must not run: {out}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dependency_graph_runs_needs_and_propagates_failure() {
    let dir = temp_project("dag");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"d\"\nversion = \"0.1.0\"\n",
    );
    write(
        &dir,
        ".flux",
        "project \"d\"\nlanguage rust\npipeline {\n\
           step frontend { command \"echo fe\" }\n\
           step backend  { command \"echo be\" }\n\
           step tests { needs [ frontend, backend ] command \"exit 1\" }\n\
           step package { needs tests command \"echo packaging\" }\n\
         }\n",
    );

    let (out, ok) = run(&dir, &["build"]);
    assert!(!ok, "build should fail when tests fail: {out}");
    // The explicit-graph banner should appear.
    assert!(out.contains("dependency graph"), "{out}");
    // package needs tests, which failed → package must be skipped.
    assert!(!out.contains("packaging"), "dependent must not run: {out}");
    assert!(out.contains("skipped (dependency failed)"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn secret_is_injected_into_step_env() {
    let dir = temp_project("secret");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"s\"\nversion = \"0.1.0\"\n",
    );
    // Expand the injected env var in a shell-appropriate way: `%VAR%` on the
    // Windows `cmd` shell, `$VAR` on the POSIX `sh` shell.
    let echo = if cfg!(windows) {
        "echo token=%TOKEN%"
    } else {
        "echo token=$TOKEN"
    };
    write(
        &dir,
        ".flux",
        &format!(
            "project \"s\"\nlanguage rust\nsecret TOKEN\npipeline {{\n\
               step show {{ command \"{echo}\" env [ TOKEN ] }}\n\
             }}\n"
        ),
    );

    let (set_out, set_ok) = run(&dir, &["secret", "set", "TOKEN", "abc123"]);
    assert!(set_ok, "secret set should succeed: {set_out}");

    let (out, ok) = run(&dir, &["build"]);
    assert!(ok, "build should succeed: {out}");
    assert!(out.contains("token=abc123"), "secret not injected: {out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cyclic_pipeline_is_rejected() {
    let dir = temp_project("cycle");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"c\"\nversion = \"0.1.0\"\n",
    );
    write(
        &dir,
        ".flux",
        "pipeline {\n\
           step a { needs b command \"echo a\" }\n\
           step b { needs a command \"echo b\" }\n\
         }\n",
    );

    let (out, ok) = run(&dir, &["build"]);
    assert!(!ok, "cyclic pipeline must be rejected: {out}");
    assert!(out.contains("cycle"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn intelligent_cache_rebuilds_only_affected() {
    let dir = temp_project("intel");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"m\"\nversion = \"0.1.0\"\n",
    );
    std::fs::create_dir_all(dir.join("frontend")).unwrap();
    std::fs::create_dir_all(dir.join("backend")).unwrap();
    write(&dir, "frontend/a.ts", "v1");
    write(&dir, "backend/b.rs", "v1");
    // The `all` step's `needs` makes this an explicit graph, so frontend and
    // backend are independent roots (not an implicit chain).
    write(
        &dir,
        ".flux",
        "project \"m\"\nlanguage rust\npipeline {\n\
           step frontend { command \"echo fe\" inputs [ \"frontend/**\" ] }\n\
           step backend  { command \"echo be\" inputs [ \"backend/**\" ] }\n\
           step all { needs [ frontend, backend ] command \"echo all\" }\n\
         }\n",
    );

    let (_, ok1) = run(&dir, &["build"]);
    assert!(ok1);
    // Change only a frontend file.
    write(&dir, "frontend/a.ts", "v2");
    let (out, ok2) = run(&dir, &["build"]);
    assert!(ok2, "{out}");
    // backend stays cached (its inputs are unchanged); frontend rebuilds.
    assert!(
        out.contains("backend  (cached"),
        "backend should stay cached: {out}"
    );
    assert!(out.contains("fe"), "frontend should rebuild: {out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn module_use_splices_reusable_steps() {
    let dir = temp_project("module");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\n",
    );
    std::fs::create_dir_all(dir.join("modules")).unwrap();
    write(
        &dir,
        "modules/base.flux",
        "pipeline {\n  step compile { command \"echo compiling\" }\n  step test { command \"echo testing\" }\n}\n",
    );
    write(
        &dir,
        ".flux",
        "project \"a\"\nlanguage rust\npipeline {\n  use base\n  step package { command \"echo packaging\" }\n}\n",
    );

    let (out, ok) = run(&dir, &["build"]);
    assert!(ok, "{out}");
    for step in ["compile", "test", "package"] {
        assert!(out.contains(step), "module step '{step}' missing: {out}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn workspace_builds_only_affected_members() {
    let dir = temp_project("ws");
    for member in ["shared", "auth"] {
        let mroot = dir.join(member);
        std::fs::create_dir_all(&mroot).unwrap();
        std::fs::write(
            mroot.join("Cargo.toml"),
            format!("[package]\nname=\"{member}\"\nversion=\"0.1.0\"\n"),
        )
        .unwrap();
        std::fs::write(
            mroot.join(".flux"),
            format!("project \"{member}\"\nlanguage rust\npipeline {{ step build {{ command \"echo build-{member}\" }} }}\n"),
        )
        .unwrap();
        std::fs::write(mroot.join("src.txt"), "v1").unwrap();
    }
    write(
        &dir,
        "flux.workspace",
        "workspace \"w\"\nmember shared { path \"shared\" }\nmember auth { path \"auth\" needs [ shared ] }\n",
    );

    // First build: both affected.
    let (out1, ok1) = run(&dir, &["workspace", "build"]);
    assert!(ok1, "{out1}");
    assert!(
        out1.contains("build-shared") && out1.contains("build-auth"),
        "{out1}"
    );

    // Change only `auth`; rebuild should skip `shared`.
    std::fs::write(dir.join("auth").join("src.txt"), "v2").unwrap();
    let (out2, ok2) = run(&dir, &["workspace", "build"]);
    assert!(ok2, "{out2}");
    assert!(
        out2.contains("shared  skipped (unchanged)"),
        "shared should be skipped: {out2}"
    );
    assert!(out2.contains("build-auth"), "auth should rebuild: {out2}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn policy_blocks_ci_when_violated() {
    let dir = temp_project("policy");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"p\"\nversion = \"0.1.0\"\n",
    );
    write(
        &dir,
        ".flux",
        "project \"p\"\nlanguage rust\npolicy prod { require tests require security }\n\
         pipeline { step build { command \"echo building\" } }\n",
    );

    let (out, ok) = run(&dir, &["ci"]);
    assert!(!ok, "ci must be blocked by policy: {out}");
    assert!(out.contains("Policy violations"), "{out}");
    assert!(
        !out.contains("building"),
        "the pipeline must not run: {out}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn init_template_writes_tailored_pipeline() {
    let dir = temp_project("tpl");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"api\"\nversion = \"0.1.0\"\n",
    );

    let (out, ok) = run(&dir, &["init", "rust-api"]);
    assert!(ok, "{out}");
    let cfg = std::fs::read_to_string(dir.join(".flux")).unwrap();
    assert!(
        cfg.contains("environment { image \"rust:latest\" }"),
        "{cfg}"
    );
    assert!(cfg.contains("target kubernetes"), "{cfg}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn validate_accepts_good_and_rejects_bad() {
    let dir = temp_project("validate");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"v\"\nversion = \"0.1.0\"\n",
    );

    // Valid pipeline.
    write(
        &dir,
        ".flux",
        "project \"v\"\nlanguage rust\npipeline { step build { command \"cargo build\" } }\n",
    );
    let (out, ok) = run(&dir, &["validate"]);
    assert!(ok, "valid .flux should pass: {out}");
    assert!(out.contains(".flux is valid"), "{out}");

    // Syntax error → non-zero exit.
    write(
        &dir,
        ".flux",
        "project \"v\"\npipeline { step build { commnd \"x\" } }\n",
    );
    let (bad, bad_ok) = run(&dir, &["validate"]);
    assert!(!bad_ok, "invalid .flux must fail: {bad}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn format_is_idempotent() {
    let dir = temp_project("fmt");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"f\"\nversion = \"0.1.0\"\n",
    );
    write(
        &dir,
        ".flux",
        "project \"f\"\nlanguage rust\npipeline{step build{command \"cargo build\"}step test{needs build command \"cargo test\"}}\n",
    );

    let (_, ok) = run(&dir, &["format"]);
    assert!(ok);
    let (out2, ok2) = run(&dir, &["format", "--check"]);
    assert!(ok2, "formatted file should pass --check: {out2}");
    assert!(out2.contains("already formatted"), "{out2}");
    let cfg = std::fs::read_to_string(dir.join(".flux")).unwrap();
    assert!(cfg.contains("    step build {"), "{cfg}");
    assert!(cfg.contains("needs [ build ]"), "{cfg}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn explain_describes_the_pipeline() {
    let dir = temp_project("explain");
    write(
        &dir,
        "Cargo.toml",
        "[package]\nname = \"e\"\nversion = \"0.1.0\"\n",
    );
    write(
        &dir,
        ".flux",
        "project \"e\"\nlanguage rust\npipeline {\n\
           step build { command \"cargo build\" }\n\
           step test { needs build command \"cargo test\" }\n\
         }\n",
    );
    let (out, ok) = run(&dir, &["explain"]);
    assert!(ok, "{out}");
    assert!(out.contains("2 step"), "{out}");
    assert!(out.contains("after build"), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plugin_search_filters_catalog() {
    let dir = temp_project("psearch");
    let (out, ok) = run(&dir, &["plugin", "search", "docker"]);
    assert!(ok, "{out}");
    assert!(out.contains("docker"), "{out}");
    assert!(
        !out.contains("terraform"),
        "unrelated plugin should not match: {out}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
