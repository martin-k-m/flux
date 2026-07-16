# Flux architecture (Phase 2)

Flux is a thin CLI over a core engine, with pluggable runners and a set of
platform subsystems.

```
flux/
├── cli/          the `flux` command-line interface (clap)
├── core/
│   ├── config/     the .flux parser (lexer + recursive-descent + AST)
│   ├── detect      project detection
│   ├── graph       the dependency-graph execution engine
│   ├── pipeline    pipeline resolution (config + detection → steps)
│   ├── runner      shared helpers (duration formatting)
│   └── logging     styled terminal output
├── runners/      shell runner, container wrapper, per-language defaults
├── cache/        content-hash build cache
├── artifacts/    artifact registry + releases
├── secrets/      encrypted secret store (ChaCha20), per-environment
├── deploy/       deployment dispatch (local/docker/kubernetes/vm)
├── agent/        local runner registration
├── analytics/    run history + build-performance aggregation
├── repro/        reproducibility lock (.flux.lock)
├── assist/       heuristic failure diagnosis
├── workspace/    multi-project workspaces + affected-detection
├── policy/       policy engine (require tests/security/approvals)
├── integrations/ Blink/Killer auto-detection
├── tools/        first-party dev tools (fmt/lint/doctor/changelog/version/deps)
└── plugins/      plugin registry + install + PDK scaffolding
```

## Execution flow

```
.flux ──▶ Parser ──▶ FluxConfig ──▶ Pipeline (resolve)
                                        │
                            Graph::build (validate: cycles, unknown needs)
                                        │
                            Graph::execute (worker pool)
                              ├─ topological scheduling
                              ├─ parallel workers (thread::scope)
                              ├─ cache short-circuit (per step)
                              ├─ secret env injection
                              ├─ container wrapping (optional)
                              ├─ retries + only_if
                              ├─ failure propagation (cascade-skip)
                              └─ Flux Assist on failure
```

## The graph engine (`core/graph`)

The heart of the platform. `Graph::build` turns the step list into a DAG (or an
implicit linear chain when no `needs` are used) and validates it with Kahn's
algorithm. `Graph::execute` runs it:

- A fixed pool of worker threads (via `std::thread::scope`) pulls ready nodes
  from a shared channel; a coordinator releases dependents as nodes complete.
- Each worker produces a fully-formatted output **block**; the coordinator is
  the only thing that prints, so concurrent steps never interleave.
- A failed node cascade-skips its transitive dependents.
- **Intelligent cache (3.2):** each step's freshness is checked against a hash
  scoped to its `inputs` globs; the coordinator marks a node `force`-rebuild when
  any of its dependencies rebuilt, so invalidation propagates downstream.

Worker count defaults to the core count (capped at 16).

## Subsystems

- **`artifacts`** — filesystem registry at `.flux-cache/artifacts/<name>/<version>/<platform>/`,
  plus release manifests under `releases/`.
- **`secrets`** — per-project ChaCha20 key; secrets stored as `nonce ‖ ciphertext`.
  The cipher is verified against the RFC 8439 test vector. Threat model: casual
  exposure, not a determined local attacker (documented in the module).
- **`deploy`** — target handlers that act when their tool (docker/kubectl) is
  present and degrade honestly otherwise; generates a real k8s manifest.
- **`agent`** — records this machine as a runner under `.flux-cache/runners/`.
- **`assist`** — a static table of failure signatures → suggestions.
- **`runners/containers`** — wraps commands for Docker/Podman when an
  `environment` image is declared.

## Integrations

- **Blink** — `blink create` can generate a `.flux`; Blink does not run builds.
- **Killer** — a `tool killer` step hands off to the Killer scanner between
  build and release.

## Modules & reproducibility

- **Modules (`core/config`)** — `use <name>` is resolved at load time by reading
  `modules/<name>.flux`, recursively expanding its own `use`s (with a visited set
  to guard cycles), and splicing its steps ahead of the pipeline's.
- **Reproducibility (`repro`)** — `Lock::capture` records toolchain versions
  (`<tool> --version`), the container image, and a source hash into `.flux.lock`;
  `diff` reports drift. The lock file is excluded from the source hash so a fresh
  lock verifies as reproducible.
- **Analytics (`analytics`)** — each run appends a tab-separated record; `analyze`
  aggregates averages, cache-hit rate, and the most expensive step.

## Platform layer (Phase 4)

- **Workspaces (`workspace`)** — a hand-written parser reads `flux.workspace`;
  ordering and cycle-detection reuse the pipeline `Graph`. Affected-detection
  hashes each member's path (via the scoped cache) and propagates downstream, so
  `workspace build` rebuilds only what changed.
- **Policy (`policy`)** — `evaluate` checks a parsed config's `policy` blocks
  against its steps and an approvals count; `flux ci` blocks on violations.
- **Integrations (`integrations`)** — detects Blink/Killer config files and can
  inject a `tool killer` security step into the pipeline automatically.
- **Tools (`tools`)** — language-aware `fmt`/`lint` wrappers, plus pure-logic
  `version` (semver bump), `changelog` (git-commit grouping), `deps`, and
  `doctor` (environment checks). The logic pieces are unit-tested directly.
- **PDK (`plugins::create`)** — scaffolds a plugin project.

## Deliberate non-goals for these phases

- **Distributed execution (2.3 / 3.1 networking)** — real gRPC controller/agents,
  heartbeats, a job queue, auth, encrypted transport, and cross-machine pool
  scheduling. Foundation: the local runner model, runner pools, and the parallel
  engine.
- **Web dashboard (2.8) / Cloud (2.9)** — a React/TS front end and hosted
  service. Foundation: the CLI and the `.flux-cache/` state it would read.
- **Enterprise teams/RBAC (3.10) / hosted marketplace fetch (3.4)** — need real
  identity and a registry service; `flux plugin install` records intent locally.
- **REST API & SDKs (4.16), visual pipeline editor (4.13), live notifications
  (4.12)** — a hosted HTTP server, a web front end, and outbound network calls.
  The CLI and `.flux-cache/` state are the data model these would build on.

## Toolchain note

Dependencies are kept free of `windows-sys` (clap without its `color` feature; a
hand-rolled directory walk instead of `walkdir`) so the crate links on a
`windows-gnu` toolchain that lacks a full MinGW binutils. Crypto is hand-rolled
(ChaCha20) rather than pulling `getrandom`/`windows-sys`.
