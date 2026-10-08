//! Choosing by purpose or by code, and typed specs checked against the part: specify-core's
//! `tests/test_choices.py` (the parts that need no analysed part), the choices parts of
//! `test_threads.py`, and a comparison with Python over a generated grid.
//!
//! The tests of `test_choices.py` that analyse a part (`test_a_typed_answer_reaches_the_intent_
//! and_a_wrong_one_is_refused`, `test_the_serve_child_interprets_what_was_typed`,
//! `test_a_choice_answer_that_is_not_text_is_refused`) go through `api`, `rules` and `serve`
//! and are ported with them; the choices they rely on are checked here and in the grid.
//! `test_call_out.py` exercises added requirements (`extras`), not `choices`: nothing of it is
//! in this module's reach.

use std::io::Read;

use serde_json::{Value, json};
use specify_core::choices::{SpecError, callout, check, code, describe, interpret, split};

const HOLE: &str = "hole.function:x";
const SHAFT: &str = "diameter.fit:x";
const HOLE_OPTIONS: [&str; 4] = ["general", "clearance:M6", "fit:H7", "fit:H8"];
const NO_OPTIONS: [&str; 0] = [];

/// The refusal of `result`, which must be one.
fn refusal(result: Result<String, SpecError>) -> String {
    match result {
        Ok(value) => panic!("read as {value:?}, not refused"),
        Err(SpecError(message)) => message,
    }
}

// test_choices.py::test_each_option_has_a_purpose_a_code_and_a_callout
#[test]
fn each_option_has_a_purpose_a_code_and_a_callout() {
    let described = describe(HOLE, &HOLE_OPTIONS, Some(6.6)).unwrap();
    let by_value = |v: &str| described.iter().find(|c| c["value"] == v).cloned().unwrap();
    assert_eq!(
        by_value("clearance:M6"),
        json!({
            "value": "clearance:M6",
            "purpose": "clearance",
            "label": "A bolt passes through",
            "code": "M6 clearance",
            "callout": "⌀6.6 H11 (6.600–6.690) for M6",
        })
    );
    assert_eq!(by_value("fit:H7")["purpose"], "fit");
    assert_eq!(by_value("fit:H8")["purpose"], "fit");
    assert_eq!(by_value("general")["label"], "Nothing in particular");
    assert_eq!(callout("fit:H7", 8.0, true).unwrap(), "⌀8 H7 (8.000–8.015)");
    assert_eq!(
        callout("tapped:M2", 1.6, true).unwrap(),
        "M2×0.4-6H, ⌀1.6 tap drill"
    );
    assert_eq!(callout("g6", 10.0, false).unwrap(), "⌀10 g6 (9.986–9.995)");
    assert_eq!(
        describe("hole.position:x", &["0.1"], Some(6.6)).unwrap(),
        Vec::<Value>::new()
    );
}

// test_choices.py::test_typed_specs_are_read_as_answers
#[test]
fn typed_specs_are_read_as_answers() {
    let cases: [(&str, &str, Option<f64>, &str); 15] = [
        (HOLE, "H7", Some(6.6), "fit:H7"),
        (HOLE, "h6", Some(6.6), "fit:H6"),
        (HOLE, "JS7", Some(6.6), "fit:JS7"),
        (HOLE, "M6", Some(6.6), "clearance:M6"),
        (HOLE, "clearance M6", Some(6.6), "clearance:M6"),
        (HOLE, "M2", Some(1.6), "tapped:M2"),
        (HOLE, "tapped M2x0.4", Some(1.6), "tapped:M2"),
        (HOLE, "none", Some(6.6), "general"),
        (SHAFT, "G6", Some(10.0), "g6"),
        (SHAFT, "m6", Some(10.0), "m6"),
        (SHAFT, "M10", Some(10.0), "thread:M10"),
        (SHAFT, "diamond knurl", Some(10.0), "knurl:diamond"),
        ("hole.position:x", "⌀0.150", None, "0.15"),
        ("hole.position:x", "none", None, "none"),
        ("part.general_tolerance", "f", None, "ISO 2768-f"),
    ];
    for (qid, text, d, value) in cases {
        let options: &[&str] = if qid.starts_with("part.") {
            &["ISO 2768-f", "ISO 2768-m"]
        } else {
            &[]
        };
        assert_eq!(
            interpret(qid, text, options, d, None).unwrap(),
            value,
            "{qid} {text}"
        );
    }
}

// test_choices.py::test_on_a_shaft_capital_m_is_a_thread_and_lower_case_a_fit
#[test]
fn on_a_shaft_capital_m_is_a_thread_and_lower_case_a_fit() {
    let options = ["general", "h6", "m6", "thread:M6", "knurl:diamond"];
    assert_eq!(
        interpret(SHAFT, "M6", &options, Some(6.0), None).unwrap(),
        "thread:M6"
    );
    assert_eq!(
        interpret(SHAFT, "m6", &options, Some(6.0), None).unwrap(),
        "m6"
    );
    assert_eq!(
        interpret(SHAFT, "M6 thread", &options, Some(6.0), None).unwrap(),
        "thread:M6"
    );
    assert_eq!(
        interpret(HOLE, "h7", &["fit:H7"], Some(8.0), None).unwrap(),
        "fit:H7"
    );
}

// test_choices.py::test_specs_that_do_not_fit_the_part_are_refused
#[test]
fn specs_that_do_not_fit_the_part_are_refused() {
    let cases: [(&str, &str, Option<f64>, &str); 9] = [
        (HOLE, "M8 tapped", Some(1.6), "M8 tap needs a ⌀6.8 drill"),
        (HOLE, "clearance M8", Some(6.6), "M8 bolt will not pass"),
        (HOLE, "M6 clearance", Some(12.0), "too loose"),
        (HOLE, "g6", Some(6.6), "Holes take an H"),
        (HOLE, "M6x0.75", Some(5.0), "Only coarse pitch"),
        (HOLE, "M7", Some(6.0), "not a tabulated metric thread"),
        (SHAFT, "M6", Some(10.0), "M6 thread is ⌀6"),
        (SHAFT, "knurl", Some(10.0), "straight or a diamond"),
        ("hole.position:x", "-1", None, "more than 0"),
    ];
    for (qid, text, d, reason) in cases {
        let message = refusal(interpret(qid, text, &NO_OPTIONS, d, None));
        assert!(message.contains(reason), "{text}: {message}");
    }
}

// test_choices.py::test_a_tapped_hole_keeps_a_typed_thread_depth
#[test]
fn a_tapped_hole_keeps_a_typed_thread_depth() {
    for (text, value) in [
        ("M2 x 0.4, usable thread depth 2.3 mm", "tapped:M2@2.3"),
        ("M2 tapped, 2 deep", "tapped:M2@2"),
        ("M2x0.4-6H 1.5mm min full thread", "tapped:M2@1.5"),
        ("M2 tapped .5 deep", "tapped:M2@0.5"),
        ("M2 tapped 2deep", "tapped:M2@2"),
        ("M2 tapped 2mmdeep", "tapped:M2@2"),
        ("M2-6H", "tapped:M2"),
        ("tapped:M2@2.3", "tapped:M2@2.3"),
    ] {
        assert_eq!(
            interpret(HOLE, text, &NO_OPTIONS, Some(1.6), Some(3.5)).unwrap(),
            value
        );
    }
}

// test_choices.py::test_a_number_that_cannot_be_kept_is_refused_not_dropped
#[test]
fn a_number_that_cannot_be_kept_is_refused_not_dropped() {
    for (text, depth, reason) in [
        // Issue 7: 3.15 mm of a 3.5 mm hole was read as plain M2, the depth lost.
        (
            "M2 x 0.4, usable thread depth 3.15 mm",
            Some(3.5),
            "at most 2.3 mm",
        ),
        (
            "M2 tapped, 2 deep",
            None,
            "through or its depth is not known",
        ),
        ("M2 tapped -2 deep", Some(3.5), "more than 0 mm"),
        ("M2 tapped 1e1 deep", Some(3.5), "1e1 is not read"),
        ("M2 3.15", Some(3.5), "3.15 mm is not read"),
        ("M2 tapped 2 deep 3 long", Some(3.5), "one thread depth"),
        ("M2 2 in deep", Some(3.5), "Lengths are in mm"),
        ("M2-6G", Some(3.5), "Only the 6H class"),
    ] {
        let message = refusal(interpret(HOLE, text, &NO_OPTIONS, Some(1.6), depth));
        assert!(message.contains(reason), "{text}: {message}");
    }
}

// test_choices.py::test_a_clearance_hole_takes_no_depth_but_may_name_its_fit_and_standard
#[test]
fn a_clearance_hole_takes_no_depth_but_may_name_its_fit_and_standard() {
    let message = refusal(interpret(
        HOLE,
        "M6 clearance, 10 deep",
        &NO_OPTIONS,
        Some(6.6),
        Some(10.0),
    ));
    assert!(
        message.contains("Only a tapped hole takes a depth"),
        "{message}"
    );
    let read = |text| interpret(HOLE, text, &NO_OPTIONS, Some(6.6), None).unwrap();
    assert_eq!(read("M6 clearance H11"), "clearance:M6");
    assert_eq!(read("M6 clearance ISO 273"), "clearance:M6");
}

// test_choices.py::test_a_stored_depth_written_otherwise_is_refused_with_what_it_reads_as
#[test]
fn a_stored_depth_written_otherwise_is_refused_with_what_it_reads_as() {
    let refused = |value: &str| {
        check(HOLE, &json!(value), &NO_OPTIONS, Some(1.6), Some(3.5))
            .unwrap_err()
            .to_string()
    };
    assert!(refused("tapped:M2@2.30").contains("reads as 'tapped:M2@2.3'"));
    assert!(refused("tapped:M2@x").contains("reads as 'tapped:M2'"));
}

// test_choices.py::test_a_thread_depth_is_named_in_the_code_and_callout
#[test]
fn a_thread_depth_is_named_in_the_code_and_callout() {
    assert_eq!(code("tapped:M2@2.3"), "M2 tapped, 2.3 mm full thread");
    assert_eq!(
        callout("tapped:M2@2.3", 1.6, true).unwrap(),
        "M2×0.4-6H, 2.3 mm min full thread, ⌀1.6 tap drill"
    );
}

// test_choices.py::test_a_choice_answer_that_is_not_text_is_refused, for `check` itself (the
// Python test goes through `questions`, which names the question in the refusal by calling it).
#[test]
fn a_choice_answer_that_is_not_text_is_refused() {
    let qid = "hole.position:x";
    for value in [
        json!(0.1),
        json!(3),
        json!(true),
        json!(["general"]),
        json!({"value": "general"}),
        Value::Null,
    ] {
        let message = check(qid, &value, &NO_OPTIONS, None, None)
            .unwrap_err()
            .to_string();
        assert!(message.starts_with(qid), "{message}");
    }
    let message = check(qid, &json!(0.1), &NO_OPTIONS, None, None).unwrap_err();
    assert_eq!(
        message.to_string(),
        "hole.position:x: an answer is text, not float 0.1."
    );
}

// test_threads.py::test_a_tapped_hole_carries_its_thread_and_drill, test_threads.py::
// test_the_serve_child_reports_the_depth_it_kept: what choices gives them (the rest is rules).
#[test]
fn a_typed_thread_depth_is_kept_in_the_answer() {
    assert_eq!(split("tapped:M5@5.5"), ("tapped", "M5", Some(5.5)));
    assert_eq!(split("tapped:M5"), ("tapped", "M5", None));
    assert_eq!(split("tapped:M5@x"), ("tapped", "M5", None));
    assert_eq!(code("tapped:M5@5.5"), "M5 tapped, 5.5 mm full thread");
}

// --------------------------------------------------------------------------
// The grid against Python.

/// Python's outputs over the grid. Generated once with specify-core's venv, from the
/// repository root:
///
/// ```text
/// /Users/paul/repos/specify-core/.venv/bin/python tests/fixtures/choices_oracle.py \
///     | gzip -n -9 > tests/fixtures/choices_oracle.json.gz
/// ```
fn oracle() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/choices_oracle.json.gz"
    );
    let file = std::fs::File::open(path).expect("tests/fixtures/choices_oracle.json.gz");
    let mut text = String::new();
    flate2::read::GzDecoder::new(file)
        .read_to_string(&mut text)
        .expect("gzip");
    serde_json::from_str(&text).expect("valid JSON")
}

fn opt_f64(v: &Value) -> Option<f64> {
    v.as_f64()
}

/// A Rust result as the oracle records Python's: `{"ok": value}` or `{"err": message}` (the
/// exception's type is not compared: Rust's error types are not Python's).
fn record<T: Into<Value>, E: ToString>(result: Result<T, E>) -> Value {
    match result {
        Ok(value) => json!({"ok": value.into()}),
        Err(err) => json!({"err": err.to_string()}),
    }
}

/// Python's record without the exception type.
fn expected(python: &Value) -> Value {
    match python.get("err") {
        Some(message) => json!({"err": message}),
        None => json!({"ok": python["ok"]}),
    }
}

#[test]
fn choices_equal_pythons_over_the_grid() {
    let data = oracle();
    let options: Vec<Vec<String>> = data["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            o.as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap().to_string())
                .collect()
        })
        .collect();
    let cases = data["cases"].as_array().unwrap();
    assert!(cases.len() > 100_000);
    let mut wrong = Vec::new();
    for case in cases {
        let c = case.as_array().unwrap();
        let text = |i: usize| c[i].as_str().unwrap();
        let (got, python) = match text(0) {
            "describe" => {
                let opts = &options[c[2].as_u64().unwrap() as usize];
                (record(describe(text(1), opts, opt_f64(&c[3]))), &c[4])
            }
            "callout" => (
                record(callout(
                    text(1),
                    c[2].as_f64().unwrap(),
                    c[3].as_bool().unwrap(),
                )),
                &c[4],
            ),
            "check" => {
                let opts = &options[c[3].as_u64().unwrap() as usize];
                let result = check(text(1), &c[2], opts, opt_f64(&c[4]), opt_f64(&c[5]));
                (record(result.map(|()| Value::Null)), &c[6])
            }
            "interpret" => {
                let opts = &options[c[3].as_u64().unwrap() as usize];
                let result = interpret(text(1), text(2), opts, opt_f64(&c[4]), opt_f64(&c[5]));
                (record(result), &c[6])
            }
            "split" => {
                // The depth as its repr (inf, nan and -0.0 included).
                let (kind, spec, depth) = split(text(1));
                let depth = depth.map(specify_core::pyfmt::repr);
                (record(Ok::<_, String>(json!([kind, spec, depth]))), &c[2])
            }
            "code" => (record(Ok::<_, String>(code(text(1)))), &c[2]),
            other => panic!("unknown case {other}"),
        };
        if got != expected(python) {
            wrong.push(format!("{case}\n   rust: {got}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} differ from Python, e.g.:\n{}",
        wrong.len(),
        cases.len(),
        wrong
            .iter()
            .take(25)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
