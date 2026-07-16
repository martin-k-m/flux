fn greeting() -> String {
    "Hello from a Flux-built Rust app!".to_string()
}

fn main() {
    println!("{}", greeting());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets() {
        assert!(greeting().contains("Flux"));
    }
}
