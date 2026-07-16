# Repository intelligence

Flux understands the repository it's pointed at, then writes that understanding
down in a form both people and AI agents can use. There is **no language model
inside Flux** — every number is derived by walking the tree and reading
manifests, so the analysis is deterministic and reproducible.

## `flux project`

```
flux project          # human-readable intelligence report
flux project --json   # emit the architecture graph as JSON
```

The report covers:

- **Languages** — a file histogram by extension (Rust, TypeScript, Python, …).
- **Architecture** — top-level source components and their dependency edges. For
  Rust these edges are inferred from `use crate::<module>` references; for other
  languages the component list is shown without edges.
- **Dependencies** — declared direct dependencies, read from `Cargo.toml`,
  `package.json`, `requirements.txt`, or `go.mod`, plus whether a lockfile pins
  them. Flux does **not** hit the network, so it never fabricates "outdated".
- **Activity** — commit count, contributors, and the last-commit date via `git`
  (empty and honest when git is unavailable or the tree isn't a repo).
- **Health** — a weighted 0–100 score. Recommendations list the unmet signals,
  highest-value first (`+15 if you add a CI workflow`).

### The health score

The score is a weighted sum of concrete, pass/fail signals — run it twice on the
same tree and you get the same number:

| Signal              | Weight | Earned when …                                   |
| ------------------- | -----: | ----------------------------------------------- |
| Tests               |     20 | the project has an automated test suite         |
| CI                  |     15 | a CI workflow is present                        |
| README              |     10 | a top-level `README.md` exists                  |
| Documentation       |     10 | a `docs/` directory exists                      |
| Flux pipeline       |     10 | a `.flux` file exists                            |
| Locked dependencies |     10 | a lockfile pins versions                         |
| Toolchain           |     10 | the language toolchain is installed             |
| Low TODO debt       |     10 | ≤ 25 `TODO`/`FIXME` markers in source           |
| Version control     |      5 | the project is tracked in git                   |

## The knowledge graph

`flux project` (and `flux init`) write a structured, AI-legible description of
the project under `.flux-cache/knowledge/`:

```
.flux-cache/knowledge/
  architecture.json   components + dependency edges
  dependencies.json   declared direct dependencies
  patterns.json       detected conventions (languages, signals)
  history.json        git activity
  decisions.json      append-only decision log (seeded once, never clobbered)
```

These files are the substrate agents work on: an external tool can read a stable
description of the project without re-deriving it. `decisions.json` is yours (and
your AI's) to append to — Flux seeds it once and then leaves it alone.

## `flux ask`

```
flux ask "explain this repository"
flux ask "what should I work on next?"
flux ask --context                     # print the raw context bundle
```

`ask` assembles the intelligence into a context bundle. With `ai.command` set in
`flux.yaml` it pipes that bundle to your model and returns a grounded answer;
otherwise it answers offline from data Flux already has, clearly labelled as a
heuristic answer. `--context` prints the bundle itself — the "for AI to use"
surface you can pipe into any tool.

## `flux dashboard`

Renders a self-contained HTML dashboard (`.flux-cache/reports/dashboard.html`)
from the same intelligence — health ring, recommendations, languages,
architecture, dependencies, and signals. Inline CSS, no network: open the file
in a browser. It's a local artifact, not a hosted service.
