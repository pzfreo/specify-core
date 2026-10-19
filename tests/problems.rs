//! Locating holes by ± dimensions and the problems list: the locating and problems tests of
//! specify-core's `tests/test_locating_problems_extras.py` (its added-requirement tests are in
//! `tests/extras.rs`), on the oracle's captured analysis of the plate.

mod common;

use serde_json::{Map, Value, json};
use specify_core::problems::problems;
use specify_core::rules::{LOCATING, Question, apply_with_defaults, questions};

fn plate() -> Value {
    common::case("parts/plate")
        .artifact("analysis")
        .expect("a captured analysis")
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

/// The highest (`top`) or lowest plane, the first of equals as Python's `max` and `min`.
fn extreme(analysis: &Value, top: bool) -> i64 {
    let planes: Vec<&Value> = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "plane")
        .collect();
    let z = |f: &Value| f["centroid"][2].as_f64().unwrap();
    let mut best = planes[0];
    for p in &planes {
        if (top && z(p) > z(best)) || (!top && z(p) < z(best)) {
            best = p;
        }
    }
    best["id"].as_i64().unwrap()
}

fn plain_holes(qs: &[Question]) -> Map<String, Value> {
    qs.iter()
        .filter(|q| q.id.starts_with("hole.function"))
        .map(|q| (q.id.clone(), json!("general")))
        .collect()
}

fn text(p: &Value) -> &str {
    p["message"].as_str().unwrap()
}

// test_locating_problems_extras.py::test_holes_located_by_plus_minus_need_no_positions_or_datums
#[test]
fn holes_located_by_plus_minus_have_datums_without_positions() {
    let plate = plate();
    let qs = questions(&plate, &Map::new()).unwrap();
    assert!(qs.iter().any(|q| q.id == "part.locating"));
    assert!(qs.iter().any(|q| q.id.starts_with("hole.position")));
    let plus_minus = answers(json!({"part.locating": LOCATING[1]}));
    let qs = questions(&plate, &plus_minus).unwrap();
    assert!(
        !qs.iter()
            .any(|q| q.id.starts_with("hole.position") || q.id.starts_with("hole.frame"))
    );
    let mut given = plus_minus;
    given.insert("part.material".into(), json!("steel"));
    let intent = apply_with_defaults(&plate, &given, true).unwrap();
    assert_eq!(intent.datums.len(), 3);
    assert!(problems(&plate, &given).unwrap().is_empty());
    assert!(!intent.requirements.iter().any(|r| r["kind"] == "position"));
    assert!(intent.requirements.iter().any(|r| r["kind"] == "size")); // fits still apply
}

// test_locating_problems_extras.py::test_a_standard_part_has_no_problems
#[test]
fn a_standard_part_has_no_problems() {
    assert_eq!(
        problems(&plate(), &Map::new()).unwrap(),
        Vec::<Value>::new()
    );
}

// test_locating_problems_extras.py::test_holes_with_nothing_to_locate_them_are_reported
#[test]
fn holes_with_nothing_to_locate_them_are_reported() {
    let found = problems(&plate(), &answers(json!({"datum.A": []}))).unwrap();
    let unlocated: Vec<&Value> = found
        .iter()
        .filter(|p| text(p).contains("nothing locates"))
        .collect();
    assert!(!unlocated.is_empty());
    assert!(unlocated.iter().all(
        |p| p["severity"] == "warning" && p["faces"].as_array().is_some_and(|f| !f.is_empty())
    ));
    assert!(
        unlocated
            .iter()
            .any(|p| text(p).contains("6× Ø6.6 holes are for M6 clearance"))
    );
}

// test_locating_problems_extras.py::test_an_unused_and_a_redundant_datum_are_reported
#[test]
fn an_unused_and_a_redundant_datum_are_reported() {
    let plate = plate();
    let mut given = plain_holes(&questions(&plate, &Map::new()).unwrap());
    given.insert("datum.A".into(), json!([extreme(&plate, true)]));
    let found = problems(&plate, &given).unwrap();
    let about: Vec<&Value> = found.iter().map(|p| &p["about"]).collect();
    assert_eq!(
        about,
        [&json!("datum.A"), &json!("datum.B"), &json!("datum.C")]
    );
    assert_eq!(found[0]["severity"], "info");
    let redundant = problems(
        &plate,
        &answers(json!({"datum.B": [extreme(&plate, false)]})),
    )
    .unwrap();
    assert!(
        redundant
            .iter()
            .any(|p| p["about"] == "datum.B" && text(p).contains("parallel to A"))
    );
}

// test_locating_problems_extras.py::test_an_orientation_on_its_own_datum_is_reported
#[test]
fn an_orientation_on_its_own_datum_is_reported() {
    let plate = plate();
    let top = extreme(&plate, true);
    let extra = json!({"faces": [top], "kind": "parallelism", "value": "0.05", "datums": ["A"]});
    let found = problems(
        &plate,
        &answers(json!({"datum.A": [top], "extra.1": extra})),
    )
    .unwrap();
    assert!(
        found
            .iter()
            .any(|p| p["about"] == "extra.1" && text(p).contains("measured from itself"))
    );
}
