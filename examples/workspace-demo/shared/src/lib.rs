pub fn banner() -> String {
    "shared library".to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn works() {
        assert_eq!(super::banner(), "shared library");
    }
}
