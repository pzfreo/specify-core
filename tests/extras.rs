//! Requirements a person adds where nothing asked: the added-requirement tests of
//! specify-core's `tests/test_locating_problems_extras.py` (its locating and problems tests
//! are in `tests/problems.rs`), on the oracle's captured analysis of the plate.
//!
//! `test_added_requirements_are_written_and_read_back`'s write is the writer's (stage 3); the
//! requirements it asserts are here.

mod common;

use serde_json::{Map, Value, json};
use specify_core::problems::problems;
use specify_core::rules::{Error, LOCATING, Question, apply_with_defaults, questions};

fn plate() -> Value {
    common::case("parts/plate")
        .artifact("analysis")
        .expect("a captured analysis")
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

fn planes(analysis: &Value) -> Vec<&Value> {
    analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "plane")
        .collect()
}

fn z(f: &Value) -> f64 {
    f["centroid"][2].as_f64().unwrap()
}

/// `_top`: the highest plane (the first of equals, as Python's `max`).
fn top(analysis: &Value) -> i64 {
    let planes = planes(analysis);
    let mut best = planes[0];
    for p in &planes {
        if z(p) > z(best) {
            best = p;
        }
    }
    best["id"].as_i64().unwrap()
}

/// `_bore8`: the Ø8 bore.
fn bore8(analysis: &Value) -> i64 {
    analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "cylinder" && (f["radius"].as_f64().unwrap() - 4.0).abs() < 1e-6)
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

fn plain_holes(qs: &[Question]) -> Map<String, Value> {
    qs.iter()
        .filter(|q| q.id.starts_with("hole.function"))
        .map(|q| (q.id.clone(), json!("general")))
        .collect()
}

fn ids(qs: &[Question]) -> Vec<&str> {
    qs.iter().map(|q| q.id.as_str()).collect()
}

// test_locating_problems_extras.py::test_added_requirements_are_written_and_read_back
// (the requirements; the write is stage 3's)
#[test]
fn added_requirements_are_written_and_read_back() {
    let plate = plate();
    let (top, bore) = (top(&plate), bore8(&plate));
    let given = answers(json!({
        "part.material": "steel",
        "extra.1": {"faces": [top], "kind": "flatness", "value": "0.05"},
        "extra.2": {"faces": [bore], "kind": "perpendicularity", "value": "0.02", "datums": ["A"]},
        "extra.3": {"faces": [top], "kind": "parallelism", "value": "0.1", "datums": ["A"]},
        "extra.4": {"faces": [bore], "kind": "fit", "value": "H6"},
    }));
    let intent = apply_with_defaults(&plate, &given, true).unwrap();
    let added = |key: &str| {
        intent
            .requirements
            .iter()
            .rfind(|r| r.get("feature") == Some(&json!("added")) && r["question"] == key)
            .unwrap()
            .clone()
    };
    assert_eq!(
        Value::Object(added("extra.1")),
        json!({
            "kind": "flatness",
            "feature": "added",
            "faces": [top],
            "question": "extra.1",
            "tolerance": 0.05,
        })
    );
    assert_eq!(added("extra.2")["diametral"], json!(true));
    assert_eq!(added("extra.2")["datums"], json!(["A"]));
    assert_eq!(added("extra.4")["fit"], "H6");
    assert!((added("extra.4")["upper"].as_f64().unwrap() - 0.009).abs() < 1e-9);
}

// test_locating_problems_extras.py::
//     test_an_added_orientation_brings_datums_where_nothing_else_needs_them
#[test]
fn an_added_orientation_keeps_the_default_measurement_frame() {
    let plate = plate();
    let mut plain = plain_holes(&questions(&plate, &Map::new()).unwrap());
    let qs = questions(&plate, &plain).unwrap();
    assert!(ids(&qs).contains(&"datum.A"));
    let extra = json!({"faces": [bore8(&plate)], "kind": "perpendicularity", "value": "0.02",
                       "datums": ["A"]});
    plain.insert("extra.1".into(), extra);
    assert!(ids(&questions(&plate, &plain).unwrap()).contains(&"datum.A"));
}

// test_locating_problems_extras.py::
//     test_an_added_requirement_citing_a_missing_datum_is_reported_not_written
#[test]
fn an_added_requirement_citing_a_missing_datum_is_reported_not_written() {
    let plate = plate();
    let extra = json!({"faces": [bore8(&plate)], "kind": "parallelism", "value": "0.1",
                       "datums": ["E"]});
    let given = answers(json!({"extra.1": extra}));
    let mut with_material = given.clone();
    with_material.insert("part.material".into(), json!("x"));
    let intent = apply_with_defaults(&plate, &with_material, true).unwrap();
    assert!(
        !intent
            .requirements
            .iter()
            .any(|r| r.get("question") == Some(&json!("extra.1")))
    );
    let found = problems(&plate, &given).unwrap();
    assert!(
        found
            .iter()
            .any(|p| p["about"] == "datum.E" && p["message"].as_str().unwrap().contains("datum E"))
    );
    // E is asked, empty, so it can be picked.
    assert!(ids(&questions(&plate, &given).unwrap()).contains(&"datum.E"));
}

// test_locating_problems_extras.py::
//     test_an_added_requirement_that_does_not_suit_its_faces_is_refused (every parameter)
#[test]
fn an_added_requirement_that_does_not_suit_its_faces_is_refused() {
    let plate = plate();
    let (top, bore) = (json!([top(&plate)]), json!([bore8(&plate)]));
    for (extra, reason) in [
        (
            json!({"faces": [], "kind": "flatness", "value": "0.1"}),
            "pick the faces",
        ),
        (
            json!({"faces": [99999], "kind": "flatness", "value": "0.1"}),
            "does not exist",
        ),
        (
            json!({"faces": bore, "kind": "flatness", "value": "0.1"}),
            "flat faces",
        ),
        (
            json!({"faces": top, "kind": "fit", "value": "H7"}),
            "cylinder",
        ),
        (
            json!({"faces": bore, "kind": "fit", "value": "Q7"}),
            "not a fit",
        ),
        (
            json!({"faces": top, "kind": "perpendicularity", "value": "0.1"}),
            "which datum",
        ),
        (
            json!({"faces": top, "kind": "flatness", "value": "none"}),
            "give a tolerance",
        ),
        (
            json!({"faces": top, "kind": "roundness", "value": "0.1"}),
            "not one of",
        ),
    ] {
        match questions(&plate, &answers(json!({"extra.1": extra}))) {
            Err(Error::Invalid(message)) => {
                assert!(message.contains(reason), "{message} lacks {reason}")
            }
            other => panic!("{extra} not refused: {other:?}"),
        }
    }
}

// test_locating_problems_extras.py::test_only_the_datums_added_orientations_cite_are_proposed
#[test]
fn added_orientations_keep_the_full_default_measurement_frame() {
    let plate = plate();
    let extra = json!({"faces": [bore8(&plate)], "kind": "perpendicularity", "value": "0.02",
                       "datums": ["B"]});
    let given = answers(json!({"part.locating": LOCATING[1], "extra.1": extra.clone()}));
    let asked = questions(&plate, &given).unwrap();
    let datums: Vec<&str> = ids(&asked)
        .into_iter()
        .filter(|i| i.matches('.').count() == 1 && i.starts_with("datum."))
        .collect();
    assert_eq!(datums, ["datum.A", "datum.B", "datum.C"]);
    let b = asked.iter().find(|q| q.id == "datum.B").unwrap();
    assert!(
        b.default
            .as_ref()
            .is_some_and(|d| d.as_array().is_some_and(|f| !f.is_empty()))
    );
    let mut both = extra;
    both["datums"] = json!(["A", "B"]);
    let given = answers(json!({"part.locating": LOCATING[1], "extra.1": both}));
    let asked = questions(&plate, &given).unwrap();
    let asked = ids(&asked);
    assert!(asked.contains(&"datum.A") && asked.contains(&"datum.B"));
    assert!(asked.contains(&"datum.C"));
}
