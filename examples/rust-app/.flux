project "rust-app"
language rust

pipeline {
    step build {
        command "cargo build --release"
        inputs [ "src/**", "Cargo.toml" ]
    }
    step test {
        needs build
        command "cargo test"
    }
}
