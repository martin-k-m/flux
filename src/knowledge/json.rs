//! A tiny, dependency-free JSON writer.
//!
//! Flux avoids `serde_json` to keep the dependency tree `windows-sys`-free (see
//! CLAUDE.md). We only ever *write* JSON (the knowledge graph, the docs
//! manifest), never parse it, so a small value tree with correct escaping and
//! stable key ordering is all we need. Objects preserve insertion order so the
//! output is deterministic and diff-friendly.

use std::fmt::Write as _;

/// A JSON value.
#[derive(Debug, Clone)]
pub enum Json {
    Null,
    Bool(bool),
    Num(i64),
    Str(String),
    Array(Vec<Json>),
    /// Insertion-ordered key/value pairs.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Convenience constructor for a string value.
    pub fn s(value: impl Into<String>) -> Json {
        Json::Str(value.into())
    }

    /// Build an array from any iterator of `Json`.
    pub fn array(items: impl IntoIterator<Item = Json>) -> Json {
        Json::Array(items.into_iter().collect())
    }

    /// Pretty-print with two-space indentation and a trailing newline.
    pub fn pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out.push('\n');
        out
    }

    fn write(&self, out: &mut String, indent: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                let _ = write!(out, "{n}");
            }
            Json::Str(s) => write_string(out, s),
            Json::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    pad(out, indent + 1);
                    item.write(out, indent + 1);
                    if i + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                pad(out, indent);
                out.push(']');
            }
            Json::Object(pairs) => {
                if pairs.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                for (i, (key, value)) in pairs.iter().enumerate() {
                    pad(out, indent + 1);
                    write_string(out, key);
                    out.push_str(": ");
                    value.write(out, indent + 1);
                    if i + 1 < pairs.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                pad(out, indent);
                out.push('}');
            }
        }
    }
}

fn pad(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_orders() {
        let v = Json::Object(vec![
            ("name".into(), Json::s("a\"b")),
            ("n".into(), Json::Num(3)),
            ("list".into(), Json::array([Json::s("x"), Json::s("y")])),
            ("empty".into(), Json::Array(vec![])),
        ]);
        let out = v.pretty();
        assert!(out.contains("\"name\": \"a\\\"b\""));
        assert!(out.contains("\"n\": 3"));
        assert!(out.contains("\"empty\": []"));
        // Insertion order preserved.
        let name_pos = out.find("name").unwrap();
        let n_pos = out.find("\"n\"").unwrap();
        assert!(name_pos < n_pos);
    }
}
