//! ISO 286 fits and metric bolt sizes: specify-core's `tests/test_standards.py`,
//! `test_threads.py::test_thread_standards`, and the lookups compared with Python over a sweep
//! of diameters (`tests/fixtures/choices_oracle.json.gz`; see `tests/choices.rs` for how it is
//! generated).

mod common;

use std::io::Read;

use serde_json::{Value, json};
use specify_core::pyfmt;
use specify_core::standards::{
    self, CLEARANCE, PITCH, TAP_DRILL, clearance_bolts, fit_limits, full_thread,
    largest_bolt_below, match_size, thread_size,
};

fn approx(a: f64, b: f64) -> bool {
    // pytest.approx's default: relative 1e-6, absolute 1e-12.
    (a - b).abs() <= (1e-6 * b.abs()).max(1e-12)
}

// test_standards.py::test_fit_limits_match_iso_286
#[test]
fn fit_limits_match_iso_286() {
    for (fit, nominal, upper, lower) in [
        ("H7", 8.0, 0.015, 0.0),
        ("H7", 40.0, 0.025, 0.0),
        ("H7", 47.0, 0.025, 0.0),
        ("H8", 25.0, 0.033, 0.0),
        ("g6", 25.0, -0.007, -0.020),
        ("h6", 20.0, 0.0, -0.013),
        ("f7", 30.0, -0.020, -0.041),
        ("k6", 50.0, 0.018, 0.002),
        ("p6", 20.0, 0.035, 0.022),
        ("js6", 20.0, 0.0065, -0.0065),
        // Range boundaries: 10 is in "over 6 up to 10", 10.01 in the next.
        ("H7", 10.0, 0.015, 0.0),
        ("H7", 10.01, 0.018, 0.0),
    ] {
        let (u, l) = fit_limits(fit, nominal).unwrap();
        assert!(
            approx(u, upper) && approx(l, lower),
            "{fit} at {nominal}: {u}, {l}"
        );
    }
}

/// Above 500 mm (Q8): limits from ISO 286-2:2010 Table 17 (shafts) and IT7 of ISO 286-1:2010
/// Table 1 (H7), one size in each range to 3150 mm, and a range edge on each side.
#[test]
fn fit_limits_match_iso_286_to_3150_mm() {
    for (fit, nominal, upper, lower) in [
        ("g6", 600.0, -22, -66),
        ("h6", 700.0, 0, -50),
        ("k6", 900.0, 56, 0),
        ("m6", 1100.0, 106, 40),
        ("f7", 1300.0, -110, -235),
        ("g6", 1800.0, -32, -124),
        ("m6", 2200.0, 178, 68),
        ("f7", 3000.0, -145, -355),
        ("k6", 3150.0, 135, 0),
        ("H7", 500.0, 63, 0),
        ("H7", 500.01, 70, 0),
        ("H7", 1250.0, 105, 0),
        ("H7", 1250.01, 125, 0),
        ("p6", 2600.0, 375, 240),
        ("n6", 600.0, 88, 44),
        ("js6", 1100.0, 33, -33),
        ("H11", 2000.0, 920, 0),
        ("h5", 2600.0, 0, -96),
    ] {
        let (u, l) = fit_limits(fit, nominal).unwrap();
        let (upper, lower) = (f64::from(upper) / 1000.0, f64::from(lower) / 1000.0);
        assert!(
            approx(u, upper) && approx(l, lower),
            "{fit} at {nominal}: {u}, {l}"
        );
    }
    // k above 500 mm is 0 for every grade, but grades above 7 stay refused as below 500 mm.
    assert!(fit_limits("k8", 600.0).is_err());
    assert_eq!(
        fit_limits("H7", 3150.01).unwrap_err(),
        "nominal size 3150.01 outside 0-3150 mm"
    );
}

// test_standards.py::test_unsupported_fits_are_refused
#[test]
fn unsupported_fits_are_refused() {
    for fit in ["Z7", "H3", "k8"] {
        assert!(fit_limits(fit, 10.0).is_err(), "{fit}");
    }
}

// test_standards.py::test_size_matching
#[test]
fn size_matching() {
    assert_eq!(match_size(6.6, &CLEARANCE), Some("M6"));
    assert_eq!(match_size(5.0, &TAP_DRILL), Some("M6"));
    assert_eq!(match_size(7.3, &CLEARANCE), None);
}

// test_standards.py::test_clearance_holes_match_any_iso_273_series
#[test]
fn clearance_holes_match_any_iso_273_series() {
    assert_eq!(clearance_bolts(6.4), ["M6"]); // fine
    assert_eq!(clearance_bolts(6.6), ["M6"]); // medium
    assert_eq!(clearance_bolts(10.0), ["M8"]); // coarse
    assert!(clearance_bolts(8.0).is_empty()); // between M6 coarse (7.0) and M8 fine (8.4)
}

// test_standards.py::test_an_unstated_bolt_is_the_largest_a_hole_plausibly_clears
#[test]
fn an_unstated_bolt_is_the_largest_a_hole_plausibly_clears() {
    assert_eq!(largest_bolt_below(8.0), Some("M6"));
    assert_eq!(largest_bolt_below(62.0), None); // a bore, not a clearance hole
}

// test_standards.py::test_small_metric_threads_are_known
#[test]
fn small_metric_threads_are_known() {
    // An M2 tapped hole is drilled Ø1.6; below M3 nothing was offered.
    assert_eq!(match_size(1.6, &TAP_DRILL), Some("M2"));
    assert_eq!(match_size(2.05, &TAP_DRILL), Some("M2.5"));
    assert_eq!(match_size(1.25, &TAP_DRILL), Some("M1.6"));
    assert_eq!(clearance_bolts(2.4), ["M2"]);
    assert_eq!(thread_size(2.5), Some("M2.5"));
    assert_eq!(full_thread(4.0, "M2"), 2.5);
}

// test_threads.py::test_thread_standards
#[test]
fn thread_standards() {
    assert_eq!(full_thread(8.0, "M5"), 5.5); // 8 - 3 x 0.8, down to 0.5
    assert_eq!(thread_size(6.0), Some("M6"));
    assert_eq!(thread_size(62.0), None);
}

#[test]
fn tables_are_keyed_by_the_same_sizes_in_the_same_order() {
    let order: Vec<&str> = standards::sizes(&PITCH).collect();
    for table in [
        &standards::CLEARANCE_FINE,
        &standards::CLEARANCE_COARSE,
        &CLEARANCE,
        &TAP_DRILL,
    ] {
        assert_eq!(standards::sizes(table).collect::<Vec<_>>(), order);
    }
    assert_eq!(standards::lookup(&PITCH, "M2.5"), Some(0.45));
    assert_eq!(standards::lookup(&PITCH, "M7"), None);
    assert_eq!(standards::bolt_diameter("M1.6"), 1.6);
}

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

/// Python's standards over the grid: the lookups at every 0.05 mm to 65 mm, every fit at the
/// range edges (limits exactly equal, messages byte for byte), and full threads.
#[test]
fn standards_equal_pythons() {
    let data = oracle();
    let opt = |v: &Value| v.as_str().map(str::to_string);
    for row in data["standards"].as_array().unwrap() {
        let d = row[0].as_f64().unwrap();
        assert_eq!(
            match_size(d, &TAP_DRILL).map(str::to_string),
            opt(&row[1]),
            "{d}"
        );
        assert_eq!(
            match_size(d, &CLEARANCE).map(str::to_string),
            opt(&row[2]),
            "{d}"
        );
        let bolts: Vec<String> = row[3]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_str().unwrap().to_string())
            .collect();
        assert_eq!(clearance_bolts(d), bolts, "{d}");
        assert_eq!(
            largest_bolt_below(d).map(str::to_string),
            opt(&row[4]),
            "{d}"
        );
        assert_eq!(thread_size(d).map(str::to_string), opt(&row[5]), "{d}");
    }
    for row in data["fits"].as_array().unwrap() {
        let (fit, nominal, python) = (row[0].as_str().unwrap(), row[1].as_f64().unwrap(), &row[2]);
        // Python's 500 mm limit (Q8: Rust tabulates to 3150 mm); those rows are compared and
        // judged by `iso286_equals_pythons`.
        if python["err"]
            .as_str()
            .is_some_and(|m| m.ends_with(" outside 0-500 mm"))
        {
            continue;
        }
        match fit_limits(fit, nominal) {
            Ok((upper, lower)) => {
                let ok = &python["ok"];
                assert_eq!(
                    (upper, lower),
                    (ok[0].as_f64().unwrap(), ok[1].as_f64().unwrap()),
                    "{fit:?} at {nominal}"
                );
            }
            Err(message) => assert_eq!(
                Some(message.as_str()),
                python["err"].as_str(),
                "{fit:?} at {}",
                pyfmt::repr(nominal)
            ),
        }
    }
    for row in data["full_threads"].as_array().unwrap() {
        let (depth, size) = (row[0].as_f64().unwrap(), row[1].as_str().unwrap());
        assert_eq!(
            full_thread(depth, size),
            row[2].as_f64().unwrap(),
            "{size} {depth}"
        );
    }
}

/// Python's fit limits over `tests/fixtures/iso286_oracle.json.gz` (every class to 3150 mm at
/// each range edge, either side of it and mid-range; see `iso286_oracle.py`): limits exactly
/// equal, messages byte for byte. Differences are judged in `known_differences/iso286.json`.
///
/// A difference's path says what kind it is, so that an entry for one kind cannot hide
/// another. Where Python refuses the size (beyond its last range) the path is `limits where
/// Python refuses the size` when Rust answers at a size up to 3150 mm, `message for the grade
/// where Python refuses the size` when Rust refuses k above grade 7 there, and `message for
/// the size` when Rust also refuses the size, beyond 3150 mm. Every other difference, an
/// unequal pair of limits among them, has path `limits` or `message`.
#[test]
fn iso286_equals_pythons() {
    let data = common::read_json_gz(&common::fixtures().join("iso286_oracle.json.gz"));
    let rows = data["fits"].as_array().unwrap();
    assert!(rows.len() > 5000, "the whole grid");
    let size_refusal = |m: &str| m.starts_with("nominal size ") && m.contains(" outside 0-");
    let mut differences = Vec::new();
    for row in rows {
        let (fit, nominal, python) = (row[0].as_str().unwrap(), row[1].as_f64().unwrap(), &row[2]);
        let python_refuses_size = python["err"].as_str().is_some_and(size_refusal);
        let tabulated = nominal <= 3150.0;
        let (rust, path) = match fit_limits(fit, nominal) {
            Ok((upper, lower)) => (
                json!({"ok": [upper, lower]}),
                if python_refuses_size && tabulated {
                    "limits where Python refuses the size"
                } else {
                    "limits"
                },
            ),
            Err(message) => {
                let path = if !python_refuses_size {
                    "message"
                } else if size_refusal(&message) && !tabulated {
                    "message for the size"
                } else if tabulated
                    && message == "k deviations are tabulated here for grades 4 to 7 only"
                {
                    "message for the grade where Python refuses the size"
                } else {
                    "message"
                };
                (json!({"err": message}), path)
            }
        };
        let python = match python.get("err") {
            Some(message) => json!({"err": message}),
            None => json!({"ok": python["ok"]}),
        };
        if rust != python {
            differences.push(common::Difference {
                case: format!("{fit}@{}", pyfmt::repr(nominal)),
                path: path.to_string(),
                python: Some(python),
                rust: Some(rust),
            });
        }
    }
    common::check_known("iso286", &differences);
}

/// Q8 end to end: a part with a Ø600 hole is asked about it with fits offered (Python, with ISO
/// 286 to 500 mm only, fails the whole question list there), and each fit is described with its
/// ISO 286 limits. The plate's captured analysis, its Ø8 hole made Ø600.
#[test]
fn a_600_mm_hole_is_offered_fits_with_their_limits() {
    let mut analysis = common::case("parts/plate")
        .artifact("analysis")
        .expect("a captured analysis");
    let hole = analysis["features"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["id"] == "holes:9")
        .expect("the plate's Ø8 hole");
    assert_eq!(hole["record"]["diameter"], json!(8.0));
    hole["record"]["diameter"] = json!(600.0);
    let questions = specify_core::rules::questions(&analysis, &serde_json::Map::new()).unwrap();
    let question = questions
        .iter()
        .find(|q| q.feature.as_deref() == Some("holes:9") && q.prompt.contains("Ø600"))
        .expect("a question about the Ø600 hole");
    for fit in standards::HOLE_FITS {
        assert!(question.options.contains(&format!("fit:{fit}")), "{fit}");
    }
    let callout = |code: &str| {
        question
            .choices
            .iter()
            .find(|c| c["value"] == code)
            .unwrap_or_else(|| panic!("{code} described"))["callout"]
            .clone()
    };
    // ISO 286-1:2010 Table 1, over 500 up to 630 mm: IT7 70, IT8 110, IT9 175, IT11 440 µm.
    assert_eq!(callout("fit:H7"), json!("⌀600 H7 (600.000–600.070)"));
    assert_eq!(callout("fit:H8"), json!("⌀600 H8 (600.000–600.110)"));
    assert_eq!(callout("fit:H9"), json!("⌀600 H9 (600.000–600.175)"));
    assert_eq!(callout("fit:H11"), json!("⌀600 H11 (600.000–600.440)"));
}
