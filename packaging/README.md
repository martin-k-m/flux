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
| **crates.io** | ready | the crate publishes as **`flux-platform`** (the short `flux` is taken by an unrelated InfluxDB client); the installed binary is still `flux`. `cargo install flux-platform` works once the first version is published — see below |

## crates.io

The crate is named `flux-platform` on crates.io (`[package] name`), while the
binary stays `flux` (`[[bin]] name`). The release workflow's `publish-crate` job
runs `cargo publish` automatically on each version tag **when the `CRATES_IO_TOKEN`
repository secret is set** — it's a no-op otherwise, so nothing publishes by
accident. To enable it: create a crates.io API token, add it as the
`CRATES_IO_TOKEN` Actions secret, then push a version tag (or run the first
`cargo publish` manually). After that, users install with `cargo install flux-platform`.

## Keeping the manifests in sync

On each release, [`homebrew/flux.rb`](homebrew/flux.rb) (source-tarball `url` +
`sha256`) and [`scoop/flux.json`](scoop/flux.json) (`version`, `url`, windows-zip
`hash`) are updated to the new checksums, then copied into the tap
(`Formula/flux.rb`) and bucket (`bucket/flux.json`) repos.
