//! ISO 286 fits and metric bolt sizes: specify-core's `tests/test_standards.py`,
//! `test_threads.py::test_thread_standards`, and the lookups compared with Python over a sweep
//! of diameters (`tests/fixtures/choices_oracle.json.gz`; see `tests/choices.rs` for how it is
//! generated).

use std::io::Read;

use serde_json::Value;
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
