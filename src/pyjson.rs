//! JSON text as Python's `json.dumps` writes it, where the exact text matters: a hash is taken
//! of it (the resume fingerprint, `resume::fingerprint`), or a file is to read as Python's does
//! (the analysis file, written with `indent=1`).
//!
//! `serde_json` and Python agree on the parsed value but not on the text: Python separates with
//! `", "` and `": "` by default, writes floats by `float.__repr__` (`5.0`, `1e-05`), escapes
//! every character outside printable ASCII (`ensure_ascii`), and writes `NaN` and `Infinity`.
//! The three forms here are Python's:
//!
//! - `json.dumps(value)`, [`dumps`];
//! - `json.dumps(value, separators=(",", ":"))`, [`dumps_compact`];
//! - `json.dumps(value, indent=1)`, [`dumps_indent1`] (items on their own lines, `","` between
//!   them, `": "` after a key; an empty list or object stays `[]` or `{}`).
//!
//! Objects keep their order (`serde_json`'s `preserve_order`, as Python's dicts do). Integers
//! are written as integers and floats by [`pyfmt::repr`], so a value read from Python's JSON
//! is written back as Python wrote it.

use serde_json::{Number, Value};

use crate::pyfmt;

/// `json.dumps(value)`: Python's default separators `", "` and `": "`.
pub fn dumps(value: &Value) -> String {
    let mut out = String::new();
    write(
        &mut out,
        value,
        &Layout::Inline {
            item: ", ",
            key: ": ",
        },
        0,
    );
    out
}

/// `json.dumps(value, separators=(",", ":"))`.
pub fn dumps_compact(value: &Value) -> String {
    let mut out = String::new();
    write(
        &mut out,
        value,
        &Layout::Inline {
            item: ",",
            key: ":",
        },
        0,
    );
    out
}

/// `json.dumps(value, indent=1)`.
pub fn dumps_indent1(value: &Value) -> String {
    let mut out = String::new();
    write(&mut out, value, &Layout::Indented { step: 1 }, 0);
    out
}

/// How containers are laid out.
enum Layout {
    /// On one line, with these separators between items and after keys.
    Inline {
        item: &'static str,
        key: &'static str,
    },
    /// One item per line, indented by `step` spaces per level (Python then uses `","` and
    /// `": "`).
    Indented { step: usize },
}

fn write(out: &mut String, value: &Value, layout: &Layout, depth: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&number(n)),
        Value::String(s) => string(out, s),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                separate(out, layout, depth + 1, i == 0);
                write(out, item, layout, depth + 1);
            }
            close(out, layout, depth);
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (key, item)) in map.iter().enumerate() {
                separate(out, layout, depth + 1, i == 0);
                string(out, key);
                out.push_str(match layout {
                    Layout::Inline { key, .. } => key,
                    Layout::Indented { .. } => ": ",
                });
                write(out, item, layout, depth + 1);
            }
            close(out, layout, depth);
            out.push('}');
        }
    }
}

/// What goes before an item: its separator from the one before, and its line and indent.
fn separate(out: &mut String, layout: &Layout, depth: usize, first: bool) {
    match layout {
        Layout::Inline { item, .. } => {
            if !first {
                out.push_str(item);
            }
        }
        Layout::Indented { step } => {
            if !first {
                out.push(',');
            }
            out.push('\n');
            out.push_str(&" ".repeat(step * depth));
        }
    }
}

/// What goes before a container's closing bracket.
fn close(out: &mut String, layout: &Layout, depth: usize) {
    if let Layout::Indented { step } = layout {
        out.push('\n');
        out.push_str(&" ".repeat(step * depth));
    }
}

/// An integer as an integer; a float as Python's `float.__repr__`, with `json`'s `NaN`,
/// `Infinity` and `-Infinity`.
fn number(n: &Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    let x = n.as_f64().expect("a JSON number is an integer or a float");
    if x.is_nan() {
        "NaN".into()
    } else if x == f64::INFINITY {
        "Infinity".into()
    } else if x == f64::NEG_INFINITY {
        "-Infinity".into()
    } else {
        pyfmt::repr(x)
    }
}

/// A string as `json.dumps` with `ensure_ascii` writes it: `"` and `\` escaped, `\b \f \n \r
/// \t` by name, every other character outside space..`~` as `\uXXXX` (a pair of surrogates
/// above U+FFFF).
fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn separators_are_pythons() {
        let v = json!([["plane", [0.0, -0.0, 5.5]], {"a": 1, "b": [true, null]}]);
        assert_eq!(
            dumps(&v),
            r#"[["plane", [0.0, -0.0, 5.5]], {"a": 1, "b": [true, null]}]"#
        );
        assert_eq!(
            dumps_compact(&v),
            r#"[["plane",[0.0,-0.0,5.5]],{"a":1,"b":[true,null]}]"#
        );
    }

    #[test]
    fn floats_are_reprs() {
        assert_eq!(
            dumps(&json!([1e-5, 1e16, 0.1, 100.0])),
            "[1e-05, 1e+16, 0.1, 100.0]"
        );
    }

    #[test]
    fn non_ascii_is_escaped() {
        // Python: json.dumps("\u00d86 \u00b5m\x7f\U0001f600\"\\\n")
        let expected = [
            "\"",
            "\\u00d86 \\u00b5m",
            "\\u007f",
            "\\ud83d\\ude00",
            "\\\"",
            "\\\\",
            "\\n",
            "\"",
        ];
        assert_eq!(
            dumps(&json!("\u{d8}6 \u{b5}m\u{7f}\u{1F600}\"\\\n")),
            expected.concat()
        );
    }

    #[test]
    fn indent_one_is_pythons() {
        let v = json!({"a": [1, 2], "b": {}, "c": []});
        assert_eq!(
            dumps_indent1(&v),
            "{\n \"a\": [\n  1,\n  2\n ],\n \"b\": {},\n \"c\": []\n}"
        );
    }
}
