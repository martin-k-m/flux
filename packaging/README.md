# Packaging

Distribution files for Flux. **Honest status:** installing from source and via
Docker works today. The package-manager channels below are prepared but not yet
published — they need a release that carries binaries (produced by
[`.github/workflows/release.yml`](../.github/workflows/release.yml) on each
version tag) and, for some, a maintainer-owned account/tap and secrets.

| Channel  | Status | Notes |
| -------- | ------ | ----- |
| **Source** | ✅ works | `git clone … && cargo build --release` |
| **Docker** | ✅ works | `docker build -t flux .` (see [`Dockerfile`](../Dockerfile)) |
| **Homebrew** | prepared | [`homebrew/flux.rb`](homebrew/flux.rb) — publish to a `homebrew-flux` tap |
| **Scoop** | prepared | [`scoop/flux.json`](scoop/flux.json) — needs a release with Windows binaries |
| **winget** | planned | submit a manifest to `microsoft/winget-pkgs` once binaries ship |
| **crates.io** | planned | `cargo publish` needs a `CARGO_REGISTRY_TOKEN` and an available crate name |

## Once a release ships binaries

The Release workflow attaches `flux-<target>.tar.gz` / `.zip` to the GitHub
Release. After that:

- **Homebrew:** copy `homebrew/flux.rb` into `martin-k-m/homebrew-flux`; users run
  `brew install martin-k-m/flux/flux`.
- **Scoop:** add `scoop/flux.json` to a bucket; users run `scoop install flux`.
- **Docker:** publish the image, or users build locally.

Nothing here claims to work before it's actually published — see the roadmap.
