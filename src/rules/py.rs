//! Python's view of JSON values, where the rules lean on it: truth, `repr`, `str`, `int()` and
//! `float()` of whatever an analysis or an answer holds.
//!
//! specify-core works on parsed JSON as Python objects, so an answer may be any JSON value and
//! the rules' messages show it as Python would (`not None`, `['finish']`). These helpers give
//! the same text and the same refusals, so a message the oracle captured is byte-identical.

use serde_json::{Map, Value};

use crate::pyfmt;

/// Python's truth of a JSON value: `None`, `False`, `0`, `0.0`, `""`, `[]` and `{}` are false.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// Python's truth of an optional value (absent is `None`).
pub fn truthy_opt(value: Option<&Value>) -> bool {
    value.is_some_and(truthy)
}

/// Whether `value` is the string `text` (Python's `value == text`).
pub fn is_str(value: Option<&Value>, text: &str) -> bool {
    matches!(value, Some(Value::String(s)) if s == text)
}

/// Python's type name of a JSON value, as its error messages give it.
pub fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Python's `repr()` of a JSON value: `None`, `True`, `3`, `0.1`, `'text'`, `[1, 2]`,
/// `{'k': 'v'}`.
pub fn repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(n) => match n.as_f64() {
            Some(x) if n.is_f64() => pyfmt::repr(x),
            _ => n.to_string(),
        },
        Value::String(s) => pyfmt::repr_str(s),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(repr).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(map) => {
            let items: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", pyfmt::repr_str(k), repr(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}

/// Python's `repr()` of a list of strings: `['A|B|C', 'D|B|C']`.
pub fn repr_strs<S: AsRef<str>>(items: &[S]) -> String {
    let items: Vec<String> = items.iter().map(|s| pyfmt::repr_str(s.as_ref())).collect();
    format!("[{}]", items.join(", "))
}

/// Python's `str()` of a JSON value: a string as it is, anything else as its `repr`.
pub fn str_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => repr(other),
    }
}

/// Python's `float()` of a JSON value, or the message of the `ValueError` or `TypeError` it
/// raises.
pub fn float_of(value: &Value) -> Result<f64, String> {
    match value {
        Value::Number(n) => Ok(n.as_f64().unwrap_or(f64::NAN)),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => pyfmt::parse_float(s)
            .ok_or_else(|| format!("could not convert string to float: {}", pyfmt::repr_str(s))),
        other => Err(format!(
            "float() argument must be a string or a real number, not '{}'",
            type_name(other)
        )),
    }
}

/// Why Python's `int()` refused a value: a `ValueError` (a str that is not an integer, a float
/// nan) or a `TypeError` (None, a list, a dict).
#[derive(Debug, Clone, PartialEq)]
pub enum IntError {
    Value(String),
    Type(String),
}

impl IntError {
    pub fn message(&self) -> &str {
        match self {
            IntError::Value(m) | IntError::Type(m) => m,
        }
    }
}

/// Python's `int()` of a JSON value: an int as it is, a float truncated, a bool 0 or 1, a str
/// of decimal digits (surrounding whitespace, a sign and single underscores between digits
/// allowed). An integer too large for an `i64` is `i64::MAX`, which no face has.
pub fn int_of(value: &Value) -> Result<i64, IntError> {
    match value {
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i)
            } else if n.is_u64() {
                Ok(i64::MAX)
            } else {
                let x = n.as_f64().unwrap_or(f64::NAN);
                if x.is_nan() {
                    Err(IntError::Value(
                        "cannot convert float NaN to integer".to_string(),
                    ))
                } else if x.is_infinite() {
                    Err(IntError::Value(
                        "cannot convert float infinity to integer".to_string(),
                    ))
                } else {
                    // Truncation toward zero; beyond i64 saturates, still no face.
                    Ok(x.trunc() as i64)
                }
            }
        }
        Value::String(s) => int_of_str(s).ok_or_else(|| {
            IntError::Value(format!(
                "invalid literal for int() with base 10: {}",
                pyfmt::repr_str(s)
            ))
        }),
        other => Err(IntError::Type(format!(
            "int() argument must be a string, a bytes-like object or a real number, not '{}'",
            type_name(other)
        ))),
    }
}

/// Python's `int(text)` in base 10.
fn int_of_str(text: &str) -> Option<i64> {
    let text = pyfmt::strip(text);
    let (negative, digits) = match text.chars().next()? {
        '-' => (true, &text[1..]),
        '+' => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') {
        return None;
    }
    if digits.contains("__") {
        return None;
    }
    let mut value: i64 = 0;
    for c in digits.chars().filter(|&c| c != '_') {
        let d = pyfmt::decimal_digit(c)?;
        value = value.saturating_mul(10).saturating_add(i64::from(d));
    }
    Some(if negative { -value } else { value })
}

/// What Python's `for x in value` yields for a JSON value: a list's items, a str's characters,
/// a dict's keys; None where it raises `TypeError` (None, a number, a bool).
pub fn iter_of(value: &Value) -> Option<Vec<Value>> {
    match value {
        Value::Array(items) => Some(items.clone()),
        Value::String(s) => Some(s.chars().map(|c| Value::String(c.to_string())).collect()),
        Value::Object(map) => Some(map.keys().map(|k| Value::String(k.clone())).collect()),
        _ => None,
    }
}

/// Python's `round(x, digits)`, as specify-core rounds distances and keys.
pub fn round(x: f64, digits: i32) -> f64 {
    pyfmt::round_half_even(x, digits)
}

/// `rules._fmt`: a number in a prompt, basis or reason (`6.6`, `8`, `0.25`).
pub fn fmt(x: f64) -> String {
    pyfmt::fmt_rules(x)
}

/// `rules._sentence`: the words with a capital and a full stop.
pub fn sentence(words: &str) -> String {
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        // Python raises IndexError on an empty string; nothing calls it with one but a mate
        // with an empty reason, which says nothing either way.
        None => ".".to_string(),
    }
}

/// `a - b` of two JSON numbers as Python computes it: two ints give an int, else a float.
pub fn sub(a: &Value, b: &Value) -> Result<Value, String> {
    if let (Some(x), Some(y)) = (a.as_i64(), b.as_i64())
        && let Some(d) = x.checked_sub(y)
    {
        return Ok(Value::from(d));
    }
    match (a, b) {
        (Value::Number(_) | Value::Bool(_), Value::Number(_) | Value::Bool(_)) => {
            let x = float_of(a)?;
            let y = float_of(b)?;
            Ok(Value::from(x - y))
        }
        _ => Err(format!(
            "unsupported operand type(s) for -: '{}' and '{}'",
            type_name(a),
            type_name(b)
        )),
    }
}

/// A JSON object built in order (Python's dict literal), from `(key, value)` pairs.
pub fn object<const N: usize>(pairs: [(&str, Value); N]) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Sorts `items` by a float key as Python's `sorted(key=...)` does: stable, ascending, equal
/// keys (and -0.0 against 0.0) keeping their order.
pub fn sort_by_float<T>(items: &mut [T], key: impl Fn(&T) -> f64) {
    items.sort_by(|a, b| {
        key(a)
            .partial_cmp(&key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// `sum(x * y for x, y in zip(a, b))`, left to right as Python adds.
pub fn dot(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    0.0 + a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repr_is_pythons() {
        assert_eq!(repr(&json!(null)), "None");
        assert_eq!(repr(&json!(["finish"])), "['finish']");
        assert_eq!(repr(&json!({"value": "general"})), "{'value': 'general'}");
        assert_eq!(repr(&json!(0.1)), "0.1");
        assert_eq!(repr(&json!(3)), "3");
        assert_eq!(repr(&json!(true)), "True");
    }

    #[test]
    fn int_is_pythons() {
        assert_eq!(int_of(&json!(" 12 ")), Ok(12));
        assert_eq!(int_of(&json!("1_2")), Ok(12));
        assert_eq!(int_of(&json!(3.9)), Ok(3));
        assert!(matches!(int_of(&json!("x")), Err(IntError::Value(_))));
        assert!(matches!(int_of(&json!(null)), Err(IntError::Type(_))));
        assert!(matches!(int_of(&json!("1__2")), Err(IntError::Value(_))));
    }

    #[test]
    fn sub_keeps_ints() {
        assert_eq!(sub(&json!(12), &json!(0)), Ok(json!(12)));
        assert_eq!(sub(&json!(12.0), &json!(0)), Ok(json!(12.0)));
    }
}
