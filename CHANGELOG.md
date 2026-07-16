# Changelog

All notable changes to Flux are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/), and this project adheres to
[Semantic Versioning](https://semver.org/).

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
