# CLAUDE.md — Flux

Context for AI agents working in this repository.

## What this is

**Flux** is a local-first developer automation / infrastructure platform: a single
Rust CLI (`flux`) that takes a project from build → test → package → deploy from
one `.flux` config file. Repo: <https://github.com/martin-k-m/flux>.

Flux is a standalone tool — it coordinates a developer's existing tools rather
than replacing them. The marketing + docs website lives in a separate repo
(`flux-web`, deployed at `flux.blinkdev.me`).

## Status: Phases 1–5 are implemented and tested

- **Phase 1 — core engine:** `.flux` language (hand-written lexer + parser),
  project detection, pipeline runner, content-hash cache, plugin foundation.
- **Phase 2 — automation:** dependency-graph execution (parallel, `needs`,
  failure propagation, retries, `only_if`), artifact registry + releases,
  encrypted secrets (ChaCha20), container build-environments, deployment
  dispatch, local runners, plugins, heuristic failure assist.
- **Phase 3 — infrastructure:** intelligent cache (per-step `inputs` globs +
  graph-aware invalidation), modules (`use`), analytics, reproducibility lock
  (`.flux.lock`), runner pools, per-environment secrets.
- **Phase 4 — platform:** workspaces (multi-project, affected-detection),
  policy engine, first-party dev tools (`fmt`/`lint`/`doctor`/`changelog`/
  `version`/`deps`), pipeline templates, plugin PDK, `status`/`graph`.
- **Phase 5 — AI-native platform:** repository intelligence (`flux project`,
  `intel/`), knowledge graph (`knowledge/` → `.flux-cache/knowledge/*.json`),
  honest heuristic AI agents (`agents/`: planner/reviewer/tester/documentation/
  maintenance/release, reports to `.flux-cache/reports/`), `flux ask` (`ask/`),
  GitHub integration (`github/`, local + `gh`), docs engine (`docs_engine/`,
  regenerates `docs/commands.md`/`agents.md`/`manifest.json`), and a static-HTML
  `flux dashboard` (`dashboard/`). **Flux embeds no LLM** — it makes the repo
  *AI-legible* and can pipe context to an external `ai.command` from `flux.yaml`.

**Deliberately deferred** (documented as non-goals in `docs/architecture.md`):
cross-machine distributed execution (gRPC), *served* web dashboard, REST API &
SDKs, visual pipeline editor, live notification delivery, enterprise teams/RBAC,
hosted plugin-marketplace fetch, a **hosted GitHub App**, and an **embedded LLM**
(use `ai.command` instead). Production APM (`incident`/`monitor` as live
monitors) is also out. Don't fake any of these with stubs that print success.

## Platform config & layout (Phase 5)

`.flux` (the pipeline) is a **file**, so the AI layer can't use a `.flux/`
directory — it would collide. Instead:
- `flux.yaml` — committed platform config (project, agents, `ai.provider`/
  `ai.command`, github, deployment). Parsed by a hand-rolled YAML-subset reader
  in `platform/` (no YAML crate → stays `windows-sys`-free).
- `.flux.d/{agents,rules,memory}/` — committed, authored platform assets.
- `.flux-cache/{knowledge,reports}/` — generated, git-ignored.
`flux agent` now means **AI agents**; local runner registration moved to
`flux runners start` / `flux runners list`.

## Build & test

Plain Cargo works — no special setup:

```sh
cargo build              # debug
cargo build --release    # release binary at target/release/flux
cargo test               # unit + integration tests (should stay green)
cargo clippy --all-targets   # keep clean
cargo fmt                # keep formatted
```

## ⚠️ Toolchain constraint — keep dependencies `windows-sys`-free

This machine's default toolchain is `stable-x86_64-pc-windows-gnu`, and its
bundled binutils has **no assembler**, so any crate that pulls in **`windows-sys`**
(via `#[link(kind="raw-dylib")]`) fails to link with a `dlltool` error. The MSVC
toolchain is installed but has **no linker** either. Net effect: the dependency
tree must avoid `windows-sys`.

Because of this the code deliberately:

- uses `clap` with `default-features = false` (no `color` feature → no
  `anstream`/`anstyle-wincon` → no `windows-sys`);
- hand-rolls the directory walk instead of using `walkdir` (→ `winapi-util`);
- hand-rolls **ChaCha20** for secrets (verified against the RFC 8439 vector)
  instead of a crypto crate that needs `getrandom`.

**Before adding any dependency, check it doesn't drag in `windows-sys`.** If you
truly need one (or want to enable a deferred feature that needs `axum`/`reqwest`/
`tonic`/`sysinfo`), the linker must be fixed first — install the MSVC "C++ build
tools" or a full MinGW-w64 (so `dlltool`/`as` exist). Until then, stay pure-Rust.
Current deps: `clap` (no default features), `sha2`, `anyhow`.

## Layout

```
src/
  cli/        clap CLI + command handlers
  core/
    config/   .flux lexer + recursive-descent parser + AST
    detect    project detection
    graph     dependency-graph execution engine (the heart)
    pipeline  config+detection → resolved steps
    logging   styled terminal output
  runners/    shell runner, container wrapper, per-language defaults
  cache/      content-hash cache + glob matcher
  artifacts/  registry + releases
  secrets/    encrypted store (chacha20.rs) + env scoping
  deploy/     local/docker/kubernetes dispatch
  agent/      local runner registration
  analytics/  run history + aggregation
  repro/      .flux.lock capture/verify
  assist/     heuristic failure diagnosis
  workspace/  multi-project workspaces + affected-detection
  policy/     policy engine
  tools/      fmt/lint/doctor/changelog/version/deps
  plugins/    registry + install + PDK scaffolding
  platform/   flux.yaml config (hand-rolled YAML-subset parser)
  fsutil.rs   shared ignore-aware directory walker
  intel/      repository intelligence (languages/deps/git/health)
  knowledge/  knowledge-graph JSON writer (+ json.rs)
  agents/     AI agent framework + built-in heuristic agents
  ask/        `flux ask` context bundle + offline answerer
  github/     CI scaffolding, PR review, issue planning (local + gh)
  docs_engine/ regenerate docs + manifest.json from live sources
  dashboard/  self-contained static-HTML project dashboard
docs/         flux-language.md, architecture.md   (authoritative reference)
tests/        integration.rs (drives the built binary)
```

## Conventions

- **Honest degradation:** tools/integrations that shell out (docker, kubectl,
  formatters) act when the tool is present and clearly say so when it isn't —
  never pretend success.
- **Test the logic:** pure logic (semver bump, glob match, changelog grouping,
  policy eval, graph ordering, chacha20) has direct unit tests; end-to-end paths
  have integration tests that run the real binary.
- The `.flux` and CLI reference in `docs/` is authoritative — keep it in sync
  with the parser and CLI when you change them.
- `.flux` is the config file; Flux keeps its state under `.flux-cache/`
  (git-ignored). `.flux.lock` is committed.
