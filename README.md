# Flux

**Flux is a local-first developer infrastructure platform.** It provides a
simple configuration language for building, testing, packaging, and deploying
applications consistently across environments. Built in Rust, Flux aims to be
the orchestration layer that connects a developer's tools — not replace them.

> Give Flux a project, and it knows how to build, test, package, and ship it.

- **Phase 1** — build and test projects.
- **Phase 2** — a dependency-graph engine, parallel execution, artifacts,
  releases, encrypted secrets, container build-environments, deployment, local
  runners, and a plugin foundation.
- **Phase 3** — an *intelligent* cache that rebuilds only what changed,
  reusable pipeline **modules**, build **analytics**, reproducibility
  **locks**, runner **pools**, and per-environment secrets.
- **Phase 4** — a platform layer: multi-project **workspaces** with
  cross-project affected-detection, first-party dev tools (`fmt`, `lint`,
  `doctor`, `changelog`, `version`, `deps`), pipeline **templates**, a **policy
  engine**, and a plugin **PDK**.
- **Phase 5** — an **AI-native** layer: **repository intelligence**
  (`flux project`), a **knowledge graph**, honest **AI agents**
  (`flux agent run …`), a natural-language front door (`flux ask`), local
  **GitHub** integration (`flux github`), a docs engine (`flux docs`), and a
  self-contained HTML **dashboard**. Flux embeds no model — it makes the
  repository *legible* to the AI agents and people who work on it, and can
  delegate to an external model you configure.

---

## Why Flux?

A project's "how to build and ship me" knowledge is usually scattered across
`package.json` scripts, a `Makefile`, CI YAML, shell scripts, and stale docs.
Flux replaces that with one source of truth — a `.flux` file:

```flux
project "my-app"
language rust

environment { image "rust:latest" }        # optional container build env

pipeline {
    step frontend { command "npm --prefix web run build" }
    step backend  { command "cargo build --release" }

    step tests {
        needs [ frontend, backend ]         # runs after both, which run in parallel
        command "cargo test"
    }

    step deploy {
        needs tests
        command "./deploy.sh"
        only_if branch == "main"            # conditional
        retries 2                           # retry on failure
        env [ DATABASE_URL ]                # inject an encrypted secret
    }
}

secret DATABASE_URL
deployment { target kubernetes replicas 3 }
```

## Install

```sh
cargo build --release      # binary at target/release/flux
```

## Commands

| Command                              | What it does                                             |
| ------------------------------------ | -------------------------------------------------------- |
| `flux init`                          | Detect the project and write a starter `.flux`           |
| `flux build`                         | Run the pipeline as a dependency graph (parallel)        |
| `flux test`                          | Run the test step(s) and their dependencies              |
| `flux run <step>`                    | Run one step and its dependencies                        |
| `flux ci`                            | Clean, cache-free pipeline; records an artifact          |
| `flux clean`                         | Remove Flux's cache/artifacts/secrets/runner state       |
| `flux info`                          | Show detection, container engine, and runners            |
| `flux deploy [--target T]`           | Deploy per the `deployment { … }` block                  |
| `flux project` / `flux ask "…"`      | Repository intelligence / natural-language query         |
| `flux agent run <name>`              | Run an AI agent (planner, reviewer, maintenance, …)      |
| `flux github init` / `docs` / `dashboard` | GitHub CI scaffolding / regen docs / HTML dashboard |
| `flux artifact push <path>` / `list` | Push to / list the artifact registry                     |
| `flux release create <version>`      | Bundle a version's artifacts into a release              |
| `flux secret set <name> <value>` / `list` | Manage encrypted secrets                            |
| `flux plugin list` / `install <name>`| Inspect / install plugins                                |
| `flux runners start` / `list`        | Register / list local build runners and pools            |
| `flux analytics`                     | Build-performance stats from run history                 |
| `flux lock` / `flux reproduce`       | Capture / verify a reproducible environment              |
| `flux secret set <n> <v> --env prod` | Per-environment encrypted secrets                        |
| `flux init <template>`               | Scaffold from a template (react, rust-api, library, cli) |
| `flux workspace status` / `build`    | Multi-project workspace, builds only affected members    |
| `flux policy`                        | Check the pipeline against declared policies             |
| `flux fmt` / `lint` / `doctor`       | Language-aware format, lint, and environment diagnosis   |
| `flux changelog` / `version <part>`  | Generate a changelog / bump the semver                   |
| `flux deps` / `status` / `graph`     | Inspect dependencies, project state, and the pipeline    |
| `flux plugin create <name>`          | Scaffold a plugin with the PDK                            |

## The graph engine (2.1)

`flux build` compiles the pipeline into a dependency graph and executes it:

- **Parallel** — independent steps run concurrently (up to the core count).
- **`needs`** — declares dependencies; execution order comes from the graph.
- **Failure propagation** — dependents of a failed step are skipped.
- **`retries N`** — retry a failing command up to *N* times.
- **`only_if <var> == "value"`** — conditional steps (e.g. `branch == "main"`).
- **Cycle & unknown-dependency detection** — reported before anything runs.

Backward-compatible: a pipeline with **no** `needs` runs as a linear chain,
exactly like Phase 1.

```text
$ flux build
Pipeline:
  dependency graph · up to 16 steps in parallel
  plan: frontend → backend → tests → deploy
  ✓ frontend  (0.3s)
  ✓ backend  (1.9s)
  ✓ tests  (2.1s)
  • deploy  skipped (only_if branch == "main" is false)
```

## Platform features

- **Artifact registry (2.4)** — `flux artifact push/list`, versioned and
  multi-platform, stored under `.flux-cache/artifacts/`.
- **Releases (2.4)** — `flux release create v1.0` bundles a version's artifacts
  into a downloadable listing.
- **Secrets (2.6)** — `flux secret set` encrypts values at rest with ChaCha20
  and injects them into a step's `env`. See the honest threat model in
  [src/secrets/mod.rs](src/secrets/mod.rs).
- **Containers (2.5)** — `environment { image "…" }` runs each command inside an
  ephemeral Docker/Podman container; degrades to native when no engine exists.
- **Deployment (2.7)** — `flux deploy` targets local, docker, or kubernetes
  (generates a real Deployment manifest); acts when the tool is present,
  degrades honestly when it isn't.
- **Runners (2.2)** — `flux runners start` registers this machine as a worker; the
  graph engine schedules steps across its cores.
- **Plugins (2.10)** — `flux plugin install <name>` records a plugin; the
  built-in language plugins drive default pipelines.
- **Flux Assist (2.12 / 3.11)** — on failure, Flux matches the output against
  known signatures and suggests fixes. No AI, no network — just heuristics.

## Infrastructure features (Phase 3)

- **Intelligent cache (3.2)** — declare `inputs [ "frontend/**" ]` on a step and
  it's only invalidated when a matching file changes. Combined with the graph,
  editing one package rebuilds *only* that package and its downstream steps —
  like Turborepo/Bazel/Nix, simplified.
- **Modules (3.3)** — put reusable pipelines in `modules/*.flux` and pull them in
  with `use <name>`. Stop copy-pasting `.flux` between projects.
- **Observability (3.5)** — every run is recorded; `flux analytics` reports
  average build time, cache-hit rate, the most expensive step, and failures.
- **Reproducibility (3.6)** — `flux lock` writes a `.flux.lock` capturing
  toolchain versions + a source hash; `flux reproduce` reports any environment
  drift so "works on my machine" becomes checkable.
- **Runner pools (3.1)** — declare `runners { pool "gpu" { requirements { … } } }`
  and view them with `flux runners list`.
- **Secret environments (3.7)** — the same secret name holds different values in
  `--env development` vs. `--env production`, each with its own key.

## Platform features (Phase 4)

- **Workspaces (4.1/4.2)** — a `flux.workspace` file declares member projects and
  their dependencies. `flux workspace build` builds them in dependency order and,
  when `shared` changes, rebuilds only `shared` and what depends on it — the
  intelligent cache applied across repositories.
- **First-party tools (4.5)** — `flux fmt`, `flux lint` (language-aware), `flux
  doctor` (environment/toolchain/config health), `flux changelog` (from git
  commits), `flux version <major|minor|patch>` (bumps the manifest), `flux deps`.
- **Templates (4.6)** — `flux init rust-api|react|library|cli|node-service` writes
  a curated pipeline with best-practice defaults.
- **Policy engine (4.15)** — `policy production { require tests, require security,
  require approvals 2 }`. `flux ci` refuses to run a pipeline that violates policy
  (approvals come from `FLUX_APPROVALS`).
- **Plugin PDK (4.19)** — `flux plugin create <name>` scaffolds a plugin with a
  manifest, source, tests, and README.

## AI-native platform (Phase 5)

Flux embeds **no language model**. Instead it makes a repository *legible* — it
writes a structured, deterministic description that AI agents and humans consume,
and can delegate to an external model you configure in `flux.yaml`
(`ai.command`). See [docs/repository-intelligence.md](docs/repository-intelligence.md)
and [docs/agents.md](docs/agents.md).

- **Repository intelligence (`flux project`)** — languages, architecture (with
  dependency edges), dependency inventory, git activity, and a deterministic
  **health score**. Writes a knowledge graph to `.flux-cache/knowledge/*.json`.
- **AI agents (`flux agent run <name>`)** — `planner`, `reviewer`, `tester`,
  `documentation`, `maintenance`, `release`. Honest heuristic analyzers that write
  structured reports to `.flux-cache/reports/`; with `ai.command` set, each report
  is expanded by your external model.
- **Ask (`flux ask "…"`)** — a natural-language front door. `--context` prints the
  raw bundle for any tool to consume; otherwise it answers offline or via your
  model.
- **GitHub (`flux github init|review|plan`)** — CI scaffolding, working-tree/PR
  review, and issue → plan. Local-first, with optional `gh` CLI enrichment; never
  posts on your behalf.
- **Docs engine (`flux docs [--check]`)** — regenerates the command reference,
  agent catalogue, and a `manifest.json` feed from live sources; `--check` guards
  drift in CI.
- **Dashboard (`flux dashboard`)** — a self-contained static HTML report (no
  network, no server).

`flux init` scaffolds this layer: it writes `flux.yaml` and `.flux.d/`
(authored assets) alongside the `.flux` pipeline.

## Self-maintenance & release

Flux helps maintain itself and any project it runs on:

- `flux verify [--full]` — run the whole check suite (format, lint, tests,
  release build, validate examples) in one command.
- `flux doctor --all` — repository health: CI, release workflow, examples, docs,
  and community files, with an overall percentage.
- `flux validate` · `flux format` · `flux explain` — check, canonically format,
  and describe a `.flux` in plain language.
- `flux plugin search <q>` · `flux plugin verify` — search the catalog, verify
  installed plugins.

**Automated releases:** pushing a `vX.Y.Z` tag triggers
[`release.yml`](.github/workflows/release.yml), which builds Linux/macOS/Windows
binaries and attaches them to the GitHub Release. A nightly workflow runs a
security audit, build/test, and a broken-link check. Packaging files (Docker,
Homebrew, Scoop) live in [`packaging/`](packaging/).

## Detection

| Language | Detected by                           | Default pipeline                                       |
| -------- | ------------------------------------- | ------------------------------------------------------ |
| Rust     | `Cargo.toml`                          | `cargo fetch` → `cargo build --release` → `cargo test` |
| Node     | `package.json`                        | `npm install` → `npm run build` → `npm test`           |
| Python   | `requirements.txt` / `pyproject.toml` | `pip install -r …` → compile → `pytest`                |

## Examples

Runnable sample projects live in [`examples/`](examples/) — [rust-app](examples/rust-app),
[node-app](examples/node-app), [python-app](examples/python-app). From any of them:

```sh
flux validate    # check the .flux
flux build       # run the pipeline
flux test        # run the test step
```

## Roadmap

**Available now (v0.3)**

- ✅ `.flux` configuration language + `flux validate`
- ✅ Dependency-graph pipeline engine (parallel, `needs`, retries, `only_if`)
- ✅ Intelligent build cache (input scoping + graph-aware invalidation)
- ✅ Modules, artifacts & releases, encrypted secrets, reproducibility lock
- ✅ Deployment dispatch, workspaces, policy engine
- ✅ First-party tools (`fmt`/`lint`/`doctor`/`changelog`/`version`/`deps`)
- ✅ Plugins + PDK, local runners
- ✅ AI-native layer: repository intelligence, knowledge graph, AI agents,
  `flux ask`, local GitHub integration, docs engine, HTML dashboard

**Coming soon**

- ○ Cross-machine distributed runners
- ○ Served web dashboard (Flux ships a local static `flux dashboard` today)
- ○ Hosted GitHub App (local `flux github` + `gh` today)
- ○ REST API & SDKs
- ○ Hosted plugin marketplace
- ○ Team workflows
- ○ Prebuilt installer (`cargo install` / Homebrew / one-line script)

## Documentation

- [The `.flux` language](docs/flux-language.md)
- [Architecture](docs/architecture.md)
- [Repository intelligence](docs/repository-intelligence.md) · [AI agents](docs/agents.md) · [GitHub integration](docs/github.md)
- [Command reference](docs/commands.md) (generated by `flux docs`)
- [Contributing](CONTRIBUTING.md) · [Security policy](SECURITY.md) · [Changelog](CHANGELOG.md)

## Scope & honesty

Flux implements the graph engine and the self-contained subsystems across all
three phases to real, tested quality. Some spec items are deliberately
**deferred** because they can't be meaningfully built or verified in a
single-machine sandbox, and stubbing them would be dishonest:

- **Cross-machine distributed execution (2.3 / 3.1 networking)** — real gRPC
  controller/agents, heartbeats, a job queue, auth, and encrypted transport.
  Runner *pools* and the parallel graph engine are the foundation; pool-based
  *scheduling across machines* is the deferred part.
- **Web dashboard (2.8)** and **Flux Cloud (2.9)** — a React/TypeScript front
  end plus a hosted service. The CLI and `.flux-cache/` state are the data
  source a future dashboard would read.
- **Enterprise teams/RBAC (3.10)** and **hosted plugin marketplace fetch (3.4)**
  — these need real identity and a registry service. `flux plugin install`
  records intent locally; the marketplace catalog is built-in, not fetched.
- **REST API & SDKs (4.16)**, **visual pipeline editor (4.13)**, and **live
  notification delivery (4.12)** — a hosted HTTP service, a web front end, and
  outbound network calls. The CLI, `.flux` config, and `.flux-cache/` state are
  the data model these would sit on top of. The policy engine, workspaces, and
  first-party tools are all real and local.

## A note on paths

`.flux` is the **config file**. Flux keeps its state (cache, artifacts,
secrets, runners, deploy manifests) under **`.flux-cache/`**, which is
git-ignored.

## License

Apache-2.0. See [LICENSE](LICENSE).
