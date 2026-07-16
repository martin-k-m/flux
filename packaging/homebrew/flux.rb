# Homebrew formula for Flux. Builds from the release source tarball with Cargo.
# To publish: place this in a tap (e.g. martin-k-m/homebrew-flux) so users can
#   brew install martin-k-m/flux/flux
# Bump `url`/`sha256` on each release (the release workflow can automate this).
class Flux < Formula
  desc "Local-first developer automation platform"
  homepage "https://github.com/martin-k-m/flux"
  url "https://github.com/martin-k-m/flux/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "078d17d357755e1ddcfdba7d2a4a7610d1a8b80debe9e6ac2932350a0e44e99a"
  license "Apache-2.0"
  head "https://github.com/martin-k-m/flux.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--locked", "--root", prefix, "--path", "."
  end

  test do
    assert_match "flux", shell_output("#{bin}/flux --version")
  end
end
