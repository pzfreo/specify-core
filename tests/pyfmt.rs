//! Python's number formatting, `float()`, `repr()` of a str and the character classes, checked
//! against Python's own output.
//!
//! `tests/fixtures/pyfmt.json` was generated once with specify-core's venv (Python 3.14,
//! Unicode 16.0), from the repository root:
//!
//! ```text
//! /Users/paul/repos/specify-core/.venv/bin/python tests/fixtures/pyfmt.py > tests/fixtures/pyfmt.json
//! ```

use regex::Regex;
use serde_json::Value;
use specify_core::pyfmt;

fn fixture() -> Value {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/pyfmt.json"
    ))
    .expect("tests/fixtures/pyfmt.json");
    serde_json::from_str(&text).expect("valid JSON")
}

fn text(v: &Value) -> &str {
    v.as_str().expect("a string")
}

/// Python's repr of a float, read back (inf, -inf, nan and -0.0 included).
fn float(v: &Value) -> f64 {
    text(v).parse().expect("a float repr")
}

/// Equality of floats that counts -0.0 and 0.0 apart and nan as itself.
fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

#[test]
fn numbers_are_formatted_as_python_formats_them() {
    let data = fixture();
    let digits: Vec<i32> = data["round_digits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_i64().unwrap() as i32)
        .collect();
    let numbers = data["numbers"].as_array().unwrap();
    assert!(numbers.len() > 2000, "a few thousand values");
    let mut wrong = Vec::new();
    for case in numbers {
        let x = float(&case["x"]);
        let mut check = |what: &str, got: String, want: &str| {
            if got != want {
                wrong.push(format!(
                    "{what}({}) = {got:?}, Python {want:?}",
                    text(&case["x"])
                ));
            }
        };
        check("repr", pyfmt::repr(x), text(&case["x"]));
        check("g", pyfmt::g(x), text(&case["g"]));
        check("signed_g", pyfmt::signed_g(x), text(&case["signed_g"]));
        check("fixed3", pyfmt::fixed(x, 3), text(&case["fixed3"]));
        check("fmt3", pyfmt::fmt3(x), text(&case["fmt3"]));
        if let Some(want) = case["fmt_rules"].as_str() {
            check("fmt_rules", pyfmt::fmt_rules(x), want);
        }
        for (n, want) in digits.iter().zip(case["round"].as_array().unwrap()) {
            let want = if want.is_null() { x } else { float(want) };
            let got = pyfmt::round_half_even(x, *n);
            if !same(got, want) {
                wrong.push(format!(
                    "round({}, {n}) = {}, Python {}",
                    text(&case["x"]),
                    pyfmt::repr(got),
                    pyfmt::repr(want)
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} differences:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// At a power of two the gap to the next float down is half the gap up, so the nearest
/// decimal with the fewest digits may not read back; Python then writes the nearest one that
/// does (2^-44 is `5.684341886080802e-14`, not `...801e-14`).
#[test]
fn reprs_at_binade_boundaries_are_pythons() {
    let data = fixture();
    let cases = data["boundary_reprs"].as_array().unwrap();
    assert!(cases.len() > 6000, "every power of two and its neighbours");
    let mut wrong = Vec::new();
    for case in cases {
        let x = f64::from_bits(u64::from_str_radix(text(&case[0]), 16).unwrap());
        let want = text(&case[1]);
        for (x, want) in [(x, want.to_string()), (-x, format!("-{want}"))] {
            let got = pyfmt::repr(x);
            if got != want {
                wrong.push(format!("repr = {got:?}, Python {want:?}"));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} differences:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn rounding_is_on_the_exact_binary_value_ties_to_even() {
    // 2.675 is stored as 2.67499999999999982236431605997495353221893310546875.
    assert_eq!(pyfmt::round_half_even(2.675, 2), 2.67);
    assert_eq!(pyfmt::round_half_even(0.125, 2), 0.12);
    assert_eq!(pyfmt::round_half_even(0.375, 2), 0.38);
    assert_eq!(pyfmt::round_half_even(2.5, 0), 2.0);
    assert_eq!(pyfmt::round_half_even(-2.5, 0), -2.0);
    assert_eq!(pyfmt::round_half_even(25.0, -1), 20.0);
    assert_eq!(pyfmt::round_half_even(35.0, -1), 40.0);
    assert!(pyfmt::round_half_even(-0.0001, 3).is_sign_negative());
}

#[test]
fn text_is_read_as_python_float_reads_it() {
    let data = fixture();
    let mut wrong = Vec::new();
    for case in data["floats"].as_array().unwrap() {
        let input = text(&case[0]);
        let got = pyfmt::parse_float(input);
        let want = case[1].as_str().map(|r| r.parse::<f64>().unwrap());
        let agree = match (got, want) {
            (Some(a), Some(b)) => same(a, b),
            (None, None) => true,
            _ => false,
        };
        if !agree {
            wrong.push(format!("float({input:?}) = {got:?}, Python {want:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn strings_are_repr_as_python_writes_them() {
    let data = fixture();
    for case in data["reprs"].as_array().unwrap() {
        assert_eq!(pyfmt::repr_str(text(&case[0])), text(&case[1]));
    }
}

/// Every character in `ranges` (Python's set) agrees with `predicate`, and no other does.
fn agrees(name: &str, ranges: &Value, predicate: impl Fn(char) -> bool) {
    let mut inside = vec![false; 0x110000];
    for range in ranges.as_array().unwrap() {
        let (a, b) = (range[0].as_u64().unwrap(), range[1].as_u64().unwrap());
        for c in a..=b {
            inside[c as usize] = true;
        }
    }
    let wrong: Vec<String> = (0..0x110000u32)
        .filter_map(|c| char::from_u32(c).map(|ch| (c, ch)))
        .filter(|&(c, ch)| predicate(ch) != inside[c as usize])
        .take(20)
        .map(|(c, _)| format!("U+{c:04X}"))
        .collect();
    assert!(wrong.is_empty(), "{name} differs from Python at {wrong:?}");
}

/// The character classes the typed-spec readers use stand for Python's: the regex crate's
/// Unicode tables against Python 3.14's (a change of either's Unicode version shows here).
#[test]
fn character_classes_are_pythons() {
    let data = fixture();
    let is = |pattern: &str| {
        let re = Regex::new(&format!("^{pattern}$")).unwrap();
        move |c: char| re.is_match(c.encode_utf8(&mut [0; 4]))
    };
    agrees("str.isspace", &data["space"], pyfmt::is_space);
    agrees("re \\s", &data["re_space"], pyfmt::is_space);
    agrees("re \\d", &data["re_digit"], is(r"\p{Nd}"));
    agrees("decimal digits", &data["re_digit"], |c| {
        pyfmt::decimal_digit(c).is_some()
    });
    agrees("re \\w", &data["re_word"], is(r"[\p{L}\p{N}_]"));
    agrees("str.isprintable", &data["printable"], pyfmt::is_printable);
    for zero in data["decimal_zeros"].as_array().unwrap() {
        let zero = zero.as_u64().unwrap() as u32;
        for d in 0..10 {
            assert_eq!(
                pyfmt::decimal_digit(char::from_u32(zero + d).unwrap()),
                Some(d as u8)
            );
        }
    }
}

#[test]
fn whitespace_is_squashed_and_stripped_as_python_does() {
    assert_eq!(
        pyfmt::squash_spaces("  M6 \t clearance\u{a0}\u{1c}x "),
        "M6 clearance x"
    );
    assert_eq!(pyfmt::strip("\u{3000} h7\u{1f}"), "h7");
    assert_eq!(pyfmt::parse_digits("٠٧"), Some(7));
    assert_eq!(pyfmt::parse_digits("7a"), None);
}

/// Where Python raises, these panic with Python's message rather than write a number.
#[test]
#[should_panic(expected = "rounded value too large to represent")]
fn rounding_past_the_largest_float_panics_as_python_raises() {
    // Python: round(sys.float_info.max, -300) is 1.79769313e+308, -307 raises.
    assert_eq!(pyfmt::round_half_even(f64::MAX, -300), 1.79769313e308);
    pyfmt::round_half_even(f64::MAX, -307);
}

#[test]
#[should_panic(expected = "cannot convert float infinity to integer")]
fn rules_fmt_of_infinity_panics_as_python_raises() {
    pyfmt::fmt_rules(f64::INFINITY);
}

#[test]
#[should_panic(expected = "cannot convert float NaN to integer")]
fn rules_fmt_of_nan_panics_as_python_raises() {
    pyfmt::fmt_rules(f64::NAN);
}
