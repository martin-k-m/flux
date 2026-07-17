# Packaging

Distribution files for Flux. As of **v0.3.0** the GitHub Release carries prebuilt
binaries for all five targets (built by
[`.github/workflows/release.yml`](../.github/workflows/release.yml) on each
version tag), and the Homebrew tap and Scoop bucket are published.

| Channel  | Status | Install |
| -------- | ------ | ------- |
| **Source** | ✅ works | `git clone … && cargo build --release` |
| **Prebuilt binary** | ✅ works | download from [the GitHub Release](https://github.com/martin-k-m/flux/releases/latest) |
| **Docker** | ✅ works | `docker build -t flux .` (see [`Dockerfile`](../Dockerfile)) |
| **Homebrew** | ✅ published | `brew install martin-k-m/flux/flux` — tap: [martin-k-m/homebrew-flux](https://github.com/martin-k-m/homebrew-flux) |
| **Scoop** | ✅ published | `scoop bucket add flux https://github.com/martin-k-m/scoop-flux && scoop install flux` |
| **winget** | planned | submit a manifest to `microsoft/winget-pkgs` |
| **crates.io** | blocked | the crate name `flux` is taken (an InfluxDB client); publishing would need a different name (e.g. `flux-cli`) plus a `CARGO_REGISTRY_TOKEN` |

## Keeping the manifests in sync

On each release, [`homebrew/flux.rb`](homebrew/flux.rb) (source-tarball `url` +
`sha256`) and [`scoop/flux.json`](scoop/flux.json) (`version`, `url`, windows-zip
`hash`) are updated to the new checksums, then copied into the tap
(`Formula/flux.rb`) and bucket (`bucket/flux.json`) repos.
