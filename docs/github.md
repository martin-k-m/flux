# GitHub integration

Flux integrates with GitHub **locally**. It is not a hosted GitHub App — that
would need a server Flux deliberately doesn't run (see the non-goals in
[architecture.md](architecture.md)). Instead Flux does the parts that are honest
offline, and enriches them with the [`gh` CLI](https://cli.github.com) when it's
installed and authenticated.

Flux never *posts* to GitHub on your behalf. Anything that publishes (a PR
comment, an issue) is left as an explicit `gh` command you run.

## `flux github init`

Scaffolds continuous integration:

```
flux github init            # write CI workflow + PR template
flux github init --force    # overwrite existing files
```

Writes:

- `.github/workflows/flux.yml` — runs `flux fmt`, `flux lint`, `flux verify`, and
  `flux build` on pushes and pull requests.
- `.github/pull_request_template.md` — a checklist referencing `flux verify` and
  `flux agent run reviewer`.

Existing files are skipped unless `--force` is passed.

## `flux github review`

```
flux github review          # review the working-tree diff
flux github review --pr 42   # review PR #42 (requires `gh`)
```

Without `--pr`, this runs the **reviewer** agent over your working tree: it
summarises changed files and flags source changes that lack a matching test
change. With `--pr N` it reads the PR's file list via `gh pr diff` (and fails
honestly if `gh` isn't installed). The report is written to
`.flux-cache/reports/`.

## `flux github plan`

```
flux github plan "add a notifications system"
flux github plan --issue 17    # fetch the issue title via `gh`
```

Turns a description — or a GitHub issue's title, fetched with `gh` — into a
structured implementation-plan skeleton via the **planner** agent (schema →
service → CLI/API → tests → docs). It's a scaffold for an AI or human to flesh
out; set `ai.command` in `flux.yaml` to have an external model expand each step
against your codebase.

## Honest degradation

Every command works without `gh` — you just lose the GitHub-specific reads (PR
diffs, issue titles). Flux tells you when `gh` would help rather than pretending
it succeeded.
