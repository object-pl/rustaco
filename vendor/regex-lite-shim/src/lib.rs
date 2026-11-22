//! The part of the `regex` API tree-sitter calls (`bytes::Regex::new` and `is_match`),
//! backed by `regex-lite`. `regex-lite` has no Unicode classes (`\p{..}`), and `\w`, `\d`
//! and `\b` are ASCII; the highlight queries of the bundled grammars need neither.

pub mod bytes {
    use std::fmt;

    /// A compiled pattern, matched against UTF-8 bytes.
    #[derive(Clone)]
    pub struct Regex(regex_lite::Regex);

    impl Regex {
        pub fn new(pattern: &str) -> Result<Regex, regex_lite::Error> {
            regex_lite::Regex::new(pattern).map(Regex)
        }

        pub fn is_match(&self, text: &[u8]) -> bool {
            match std::str::from_utf8(text) {
                Ok(s) => self.0.is_match(s),
                Err(_) => self.0.is_match(&String::from_utf8_lossy(text)),
            }
        }

        pub fn as_str(&self) -> &str {
            self.0.as_str()
        }
    }

    impl fmt::Debug for Regex {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_tuple("Regex").field(&self.0.as_str()).finish()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::bytes::Regex;

    #[test]
    fn matches_like_regex() {
        let r = Regex::new(r"^[A-Z][A-Z\d_]*$").unwrap();
        assert!(r.is_match(b"MAX_SIZE2"));
        assert!(!r.is_match(b"MaxSize"));
        assert!(Regex::new("(").is_err());
    }
}
