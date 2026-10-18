//! Runout on turned diameters, and profile and runout added on any faces: the rules side of
//! specify-core's `tests/test_profile_runout.py`, on the oracle's captured analyses of the pin
//! and the plate. Writing each and reading it back is the writer's (stage 3).

mod common;

use std::collections::HashSet;

use serde_json::{Map, Value, json};
use specify_core::rules::{Error, Faces, Question, apply_with_defaults, default_datums, questions};

fn analysis(case: &str) -> Value {
    common::case(case)
        .artifact("analysis")
        .expect("a captured analysis")
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

/// `_runouts`: the runout questions.
fn runouts(analysis: &Value, given: &Map<String, Value>) -> Vec<Question> {
    questions(analysis, given)
        .unwrap()
        .into_iter()
        .filter(|q| q.id.starts_with("diameter.runout"))
        .collect()
}

// test_profile_runout.py::
//     test_turned_diameters_offer_runout_about_the_part_axis_but_not_the_axis_itself
#[test]
fn turned_diameters_offer_runout_about_the_part_axis_but_not_the_axis_itself() {
    let pin = analysis("parts/pin");
    let faces = Faces::from_analysis(&pin).unwrap();
    let turned = pin["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["family"] == "turned_steps");
    let (a, _, _) = default_datums(&faces, turned).unwrap();
    let axis: HashSet<i64> = a.into_iter().collect();
    let asked = runouts(&pin, &Map::new());
    assert!(!asked.is_empty());
    assert!(
        asked
            .iter()
            .all(|q| q.frame.as_deref() == Some("A") && q.default == Some(json!("none")))
    );
    assert!(
        asked
            .iter()
            .all(|q| q.attention == "refine" && q.options.iter().any(|o| o == "0.02"))
    );
    assert!(
        !asked
            .iter()
            .any(|q| q.faces.iter().any(|i| axis.contains(i)))
    );
}

// test_profile_runout.py::test_a_runout_is_written_about_the_frame_and_read_back (the intent)
#[test]
fn a_runout_is_written_about_the_frame_and_read_back() {
    let pin = analysis("parts/pin");
    let q = runouts(&pin, &Map::new()).remove(0);
    let given = answers(json!({"part.material": "brass", q.id: "0.02"}));
    let intent = apply_with_defaults(&pin, &given, true).unwrap();
    let runouts: Vec<_> = intent
        .requirements
        .iter()
        .filter(|r| r["kind"] == "runout")
        .collect();
    assert_eq!(runouts.len(), 1);
    assert_eq!(runouts[0]["tolerance"].as_f64(), Some(0.02));
    assert_eq!(runouts[0]["datums"], json!(["A"]));
}

// test_profile_runout.py::test_profile_and_total_runout_can_be_added_on_any_faces (the intent)
#[test]
fn profile_and_total_runout_can_be_added_on_any_faces() {
    let plate = analysis("parts/plate");
    let faces = plate["faces"].as_array().unwrap();
    let z = |f: &Value| f["centroid"][2].as_f64().unwrap();
    let planes: Vec<&Value> = faces.iter().filter(|f| f["kind"] == "plane").collect();
    let mut top = planes[0];
    for p in &planes {
        if z(p) > z(top) {
            top = p;
        }
    }
    let bore = faces
        .iter()
        .find(|f| f["kind"] == "cylinder" && (f["radius"].as_f64().unwrap() - 4.0).abs() < 1e-6)
        .unwrap();
    let given = answers(json!({
        "part.material": "steel",
        "extra.1": {"faces": [top["id"]], "kind": "profile", "value": "0.1"},
        "extra.2": {"faces": [top["id"]], "kind": "profile", "value": "0.2", "datums": ["A"]},
        "extra.3": {"faces": [bore["id"]], "kind": "total_runout", "value": "0.03",
                    "datums": ["A"]},
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
    assert_eq!(added("extra.1")["datums"], json!([]));
    assert_eq!(added("extra.1")["diametral"], json!(false));
    assert_eq!(added("extra.3")["kind"], "total_runout");
    assert_eq!(added("extra.3")["diametral"], json!(false));
}

// test_profile_runout.py::test_a_runout_needs_a_datum_but_a_profile_does_not
#[test]
fn a_runout_needs_a_datum_but_a_profile_does_not() {
    let plate = analysis("parts/plate");
    let face = plate["faces"][0]["id"].clone();
    let runout = answers(json!({"extra.1": {"faces": [face], "kind": "runout", "value": "0.05"}}));
    match questions(&plate, &runout) {
        Err(Error::Invalid(message)) => assert!(message.contains("which datum"), "{message}"),
        other => panic!("not refused: {other:?}"),
    }
    let profile =
        answers(json!({"extra.1": {"faces": [face], "kind": "profile", "value": "0.05"}}));
    questions(&plate, &profile).unwrap();
}
