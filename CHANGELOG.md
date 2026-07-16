# Changelog

All notable changes to Flux are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.2.0] — 2026-07-16

**AI-native platform (Phase 5).** Flux becomes AI-legible without embedding a
model. Mostly additive; one intentional rename (see *Changed*).

### Added

- **Repository intelligence (`flux project`, `--json`)** — languages,
  architecture with dependency edges, dependency inventory, git activity, and a
  deterministic weighted **health score**. Writes a knowledge graph to
  `.flux-cache/knowledge/*.json`.
- **AI agents (`flux agent list|run|status|create|install`)** — honest heuristic
  analyzers (`planner`, `reviewer`, `tester`, `documentation`, `maintenance`,
  `release`) that write structured reports to `.flux-cache/reports/`. With
  `ai.command` set in `flux.yaml`, each report is expanded by an external model.
- **`flux ask "…"`** — natural-language front door; `--context` prints the raw
  context bundle for any tool. Answers offline or via `ai.command`.
- **`flux github init|review|plan`** — local CI scaffolding, working-tree/PR
  review, and issue → plan (optional `gh` CLI enrichment; never auto-posts).
- **`flux docs [--check]`** — regenerates `docs/commands.md`, `docs/agents.md`,
  and a `docs/manifest.json` feed from live sources; `--check` guards drift.
- **`flux dashboard`** — a self-contained static HTML project report (no network).
- **`flux rollback`** — redeploy the previous release through the deploy path.
- **Platform config** — `flux init` now scaffolds `flux.yaml` and `.flux.d/`
  (authored assets) alongside the `.flux` pipeline.

### Changed

- **`flux agent` now runs AI agents.** Local build-runner registration moved to
  **`flux runners start`** (and `flux runners list`).

### Phase 6 — platform consolidation & release automation

- **`flux verify`** (`--release`, `--full`) — run the project's full check suite
  (format, lint, tests; release build; validate examples).
- **`flux doctor --all`** — repository-wide health checks (CI, release workflow,
  examples, docs, community files) with an overall health percentage.
- **`flux explain`** — describe the pipeline in plain language.
- **`flux format`** (`--check`) — canonically format a `.flux` file.
- **`flux plugin search` / `flux plugin verify`** — search the catalog and verify
  installed plugin manifests.
- **Release automation** — `.github/workflows/release.yml` builds cross-platform
  binaries (Linux/macOS/Windows) and attaches them to the GitHub Release on each
  version tag; `nightly.yml` runs a security audit, build/test, and link check.
- **Packaging** — `Dockerfile` plus prepared Homebrew/Scoop manifests under
  `packaging/` (see `packaging/README.md` for publishing status).
- **Community** — issue/PR templates, `CODE_OF_CONDUCT.md`.
- **Example** — `examples/workspace-demo` (a `flux.workspace`).
- Flux now dogfoods itself via a root `.flux`.

## [0.1.0] — 2026-07-16

First public release. Flux is a local-first developer automation platform: a
single Rust CLI that builds, tests, packages, and ships a project from one
`.flux` file.

### Added

- **`.flux` configuration language** — hand-written lexer + recursive-descent
  parser with line-numbered errors; `flux validate` to check a file.
- **Dependency-graph execution engine** — parallel steps, `needs`, failure
  propagation, retries, and `only_if` conditionals. Backward-compatible linear
  pipelines when no `needs` are declared.
- **Intelligent build cache** — content-hash cache with per-step `inputs` glob
  scoping and graph-aware invalidation (rebuilds only what changed and its
  downstream).
- **Project detection & templates** — Rust / Node / Python out of the box;
  `flux init <template>` for `rust-api`, `react`, `node-service`, `library`, `cli`.
- **Modules** — reusable pipelines via `use`, resolved from a `modules/` directory.
- **Artifacts & releases** — a local registry (`flux artifact push/list`) and
  `flux release create`.
- **Encrypted secrets** — ChaCha20 (verified against the RFC 8439 vector), scoped
  per environment; injected into a step's `env`.
- **Container build-environments** — `environment { image "…" }` runs steps in
  Docker/Podman when available, degrading to native otherwise.
- **Deployment** — `flux deploy` dispatches to local / docker / kubernetes,
  generating a real manifest and acting when the tool is present.
- **Workspaces** — multi-project builds (`flux.workspace`) with cross-project
  affected-detection.
- **Policy engine** — `policy { require tests, require security, require approvals N }`;
  `flux ci` blocks on violations.
- **Reproducibility** — `flux lock` / `flux reproduce` capture and diff the
  environment (`.flux.lock`).
- **First-party dev tools** — `flux fmt`, `lint`, `doctor`, `changelog`,
  `version`, `deps`.
- **Observability** — run history + `flux analytics`; `flux status` and
  `flux graph`.
- **Plugins** — registry, `flux plugin install`, and a PDK (`flux plugin create`).
- **Local runners & runner pools**, and automatic **Blink/Killer** integration.

### Deferred (roadmap, not in this release)

Cross-machine distributed execution, a web dashboard, a REST API & SDKs, a visual
pipeline editor, live notification delivery, enterprise teams/RBAC, a hosted
plugin marketplace, and a prebuilt installer. See the README roadmap.

[0.1.0]: https://github.com/martin-k-m/flux/releases/tag/v0.1.0
