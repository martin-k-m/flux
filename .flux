project "flux"
language rust

# Flux dogfoods itself: `flux build` here compiles and tests the CLI.
pipeline {
    step build {
        command "cargo build --release"
        inputs [ "src/**", "Cargo.toml", "Cargo.lock" ]
    }
    step test {
        needs build
        command "cargo test"
    }
    step lint {
        needs build
        command "cargo clippy --all-targets -- -D warnings"
    }
}
