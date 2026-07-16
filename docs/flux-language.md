# The `.flux` configuration language

Flux has its own small declarative language. A `.flux` file is the single
source of truth for how a project builds, tests, packages, and ships.

## A complete example

```flux
# Comments start with '#' (or '//') and run to end of line.
project "my-app"
language rust

# Optional: run every command inside a container build environment.
environment { image "rust:latest" }

pipeline {
    step frontend { command "npm --prefix web run build" }
    step backend  { command "cargo build --release" }

    step tests {
        needs [ frontend, backend ]     # runs after both (which run in parallel)
        command "cargo test"
    }

    step security {
        needs tests
        tool killer                     # hand off to the Killer scanner
    }

    step deploy {
        needs security
        command "./deploy.sh"
        only_if branch == "main"        # conditional
        retries 2                       # retry on failure
        env [ DATABASE_URL ]            # inject an encrypted secret
    }
}

secret DATABASE_URL
deployment { target kubernetes replicas 3 }
```

## Top-level directives

| Directive                | Meaning                                             |
| ------------------------ | --------------------------------------------------- |
| `project "<name>"`       | The project name.                                   |
| `language <id>`          | `rust`, `node`, `python`, …                         |
| `environment { … }`      | A container build environment (see below).          |
| `secret <NAME>`          | Declares a secret the pipeline uses.                |
| `deployment { … }`       | A deploy target (see below).                        |
| `pipeline { … }`         | The steps.                                          |

All are optional. Omit `language` and Flux detects it; omit the `pipeline` and
Flux uses the language's default steps.

## Steps

`step <name> { <fields> }`. Every step needs **either** a `command` or a `tool`.

| Field                    | Meaning                                                       |
| ------------------------ | ------------------------------------------------------------- |
| `command "<shell>"`      | Shell command (`cmd /C` on Windows, `sh -c` elsewhere).       |
| `tool <id>`              | Hand off to a tool integration (e.g. `killer`).               |
| `description "<text>"`   | Optional human description.                                   |
| `cache on` / `cache off` | Participate in the build cache (default `on`).                |
| `needs <a>` / `needs [ a, b ]` | Steps that must succeed first.                          |
| `env <A>` / `env [ A, B ]` | Secret names to inject as environment variables.            |
| `retries <n>`            | Retry the command up to *n* times on failure.                 |
| `only_if <var> == "v"`   | Run only when the condition holds (also `!=`).                |
| `inputs [ "glob", … ]`   | Scope the cache to these files (intelligent cache, 3.2).       |
| `pool "<name>"`          | Prefer a declared runner pool (3.1).                          |

### Execution model

- With **no** `needs` anywhere, the pipeline runs as a **linear chain** in
  declared order (Phase 1 behaviour).
- With any `needs`, it becomes a **dependency graph**: steps without `needs` are
  roots and run in parallel; each step waits for its dependencies.
- If a step fails (after its retries), its transitive dependents are **skipped**.
- `only_if` that evaluates false marks the step **skipped**; this does *not*
  cascade — dependents still run.

### Conditions

`only_if <var> == "value"` or `only_if <var> != "value"`. The only variable
currently provided is `branch` (the current git branch), so:

```flux
step deploy {
    only_if branch == "main"
}
```

The colon style from the spec (`only_if:`, `env:`) is also accepted — a `:`
after a field keyword is ignored.

## Environments (containers)

```flux
environment { image "rust:latest" }
```

When set, each command runs inside that image via Docker or Podman
(`docker run --rm -v <project>:/workspace -w /workspace <image> sh -c '<cmd>'`).
If no engine is installed, Flux runs the command natively and says so.

## Deployment

```flux
deployment {
    target kubernetes     # local | docker | kubernetes | vm
    replicas 3
    image "myapp:1.0"     # optional
}
```

`flux deploy` dispatches to the target. For `kubernetes` it generates a real
Deployment manifest under `.flux-cache/deploy/` and applies it with `kubectl`
when available.

## Secrets

Declare with `secret NAME`, set with `flux secret set NAME value` (encrypted at
rest), and inject with a step's `env [ NAME ]`. Set per-environment values with
`--env` (e.g. `flux secret set DB_URL … --env production`); the pipeline reads
the environment named by `FLUX_ENV` (default `default`).

## Intelligent cache (3.2)

By default a step's cache tracks the whole project. Declare `inputs` to scope it:

```flux
step frontend { command "npm run build" inputs [ "frontend/**" ] }
step backend  { command "cargo build"   inputs [ "backend/**" ] }
```

Now editing a `backend/` file leaves `frontend` cached. Because the engine knows
the graph, any step that `needs` a *rebuilt* step is itself rebuilt — so only
the affected packages and their downstream steps run. Globs support `**` (any
depth), `*` and `?` (within a path segment).

## Modules (3.3)

Put a reusable pipeline in `modules/<name>.flux` and pull it in with `use`:

```flux
# modules/rust-library.flux
pipeline {
    step deps  { command "cargo fetch" }
    step build { command "cargo build --release" }
    step test  { command "cargo test" }
}
```

```flux
# .flux
project "my-api"
language rust
pipeline {
    use rust-library        # splices in deps/build/test
    step package { command "docker build ." }
}
```

Module steps are spliced ahead of the pipeline's own; explicit steps win on
name collisions. `import <name>` declares an intended module (optional).

## Runner pools (3.1)

```flux
runners {
    pool "gpu-builders" {
        requirements { gpu true, memory "32gb" }
    }
    pool "linux" { os linux }
}
```

View pools and registered runners with `flux runners list`. A step can prefer a
pool with `pool "gpu-builders"`. (Scheduling *across machines* is part of the
deferred distributed runner network; locally the graph engine uses this machine.)

## Grammar

```text
config    := item*
item      := "project" STRING
           | "language" IDENT
           | "environment" "{" ("image" STRING)* "}"
           | "secret" IDENT
           | "deployment" "{" dep_field* "}"
           | "pipeline" "{" step* "}"
dep_field := "target" IDENT | "replicas" NUM | "image" STRING
step      := "step" IDENT "{" field* "}"
field     := "command" STRING | "tool" IDENT | "description" STRING
           | "cache" IDENT | "needs" ident_or_list | "env" ident_or_list
           | "retries" NUM | "only_if" IDENT ("==" | "!=") STRING
ident_or_list := IDENT | "[" (IDENT ("," IDENT)*)? "]"
```

Strings support `\n`, `\t`, `\"`, `\\`. Identifiers are
`[A-Za-z_][A-Za-z0-9_.-]*`. Parse errors report a 1-based line number.
