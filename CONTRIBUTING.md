# Contributing to Flux

Thanks for your interest in Flux! It's an open-source project (v0.3) and
contributions are welcome — bug reports, docs, tests, and code.

## Development setup

You need a recent **Rust toolchain** (1.74+). Then:

```sh
git clone https://github.com/martin-k-m/flux
cd flux
cargo build            # debug build
cargo test             # run the test suite
cargo run -- --help    # try the CLI
```

The release binary lands at `target/release/flux` after `cargo build --release`.

> **Windows note:** the default `windows-gnu` toolchain on some setups can't link
> crates that pull in `windows-sys` (a missing assembler). Flux deliberately keeps
> its dependency tree `windows-sys`-free so it builds anywhere. Please don't add a
> dependency that reintroduces it — see [CLAUDE.md](CLAUDE.md) for the details.

## Before you open a PR

Run the same checks CI runs:

```sh
cargo fmt --all                    # format
cargo clippy --all-targets         # lint (CI denies warnings)
cargo test --all                   # tests
```

Keep them all green. If you change the CLI or the `.flux` language, update the
docs in `docs/` and the `README.md` to match — they're the authoritative
reference.

## Coding style

- Follow `rustfmt` (default config) and keep `clippy` clean.
- Match the surrounding code's naming and comment density.
- **Honest degradation:** commands that shell out to external tools (docker,
  kubectl, formatters) must act when the tool is present and say so clearly when
  it isn't — never pretend success.
- **Test the logic:** pure logic gets direct unit tests; end-to-end paths get
  integration tests in `tests/` that drive the real binary.
- Don't add features by stubbing them to print fake success. If something isn't
  built, mark it as such.

## Pull request process

1. Fork and create a topic branch (`feat/…`, `fix/…`, `docs/…`).
2. Make focused commits with clear messages.
3. Ensure fmt + clippy + tests pass locally.
4. Open a PR against `main` describing **what** changed and **why**, plus how you
   verified it. Link any related issue.
5. A maintainer reviews; address feedback and keep the branch up to date.

## Issues

- **Bugs:** include the `flux` version (`flux --version`), OS, the `.flux` file (or
  a minimal repro), the command you ran, and the full output.
- **Features:** describe the problem you're trying to solve, not just the
  solution. Check the roadmap in the README first.

## Scope

Flux aims to be the local-first orchestration layer between writing code and
shipping it — coordinating a developer's existing tools rather than replacing
all of them. Keep contributions aligned with that focus.

## License

By contributing, you agree that your contributions are licensed under the
project's [Apache-2.0](LICENSE) license.
