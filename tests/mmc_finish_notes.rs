//! Clearance holes positioned at MMC, the part's notes, and surface finish: the rules side of
//! specify-core's `tests/test_mmc_finish_notes.py`, on the oracle's captured analysis of the
//! plate. What each test asserts of the written file and its read-back is the writer's
//! (stage 3) and the existing-PMI reader's; the intent it asserts is here.

mod common;

use serde_json::{Map, Value, json};
use specify_core::rules::{Error, Intent, apply_with_defaults, questions};

fn plate() -> Value {
    common::case("parts/plate")
        .artifact("analysis")
        .expect("a captured analysis")
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

/// The `written` fixture's intent, and the top face its finish is on.
fn written() -> (Intent, i64) {
    let plate = plate();
    let bore = questions(&plate, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id.starts_with("hole.function") && q.prompt.contains("Ø8"))
        .unwrap()
        .id;
    let faces = plate["faces"].as_array().unwrap();
    let z = |f: &Value| f["centroid"][2].as_f64().unwrap();
    let planes: Vec<&Value> = faces.iter().filter(|f| f["kind"] == "plane").collect();
    let mut top = planes[0];
    for p in &planes {
        if z(p) > z(top) {
            top = p;
        }
    }
    let top = top["id"].as_i64().unwrap();
    let given = answers(json!({
        "part.material": "Aluminium 6082-T6",
        bore: "fit:H7",
        "part.coating": "Anodise black (MIL-A-8625 Type II)",
        "part.heat_treatment": "Solution treat and age to T6",
        "part.surface_finish": "Ra 1.6",
        "extra.1": {"faces": [top], "kind": "finish", "value": "0.8"},
    }));
    (apply_with_defaults(&plate, &given, true).unwrap(), top)
}

// test_mmc_finish_notes.py::test_clearance_holes_are_positioned_at_mmc_and_fitted_ones_are_not
#[test]
fn clearance_holes_are_positioned_at_mmc_and_fitted_ones_are_not() {
    let (intent, _) = written();
    let positions: Vec<&Map<String, Value>> = intent
        .requirements
        .iter()
        .filter(|r| r["kind"] == "position")
        .collect();
    let feature = |r: &Map<String, Value>| r["feature"].as_str().unwrap().to_string();
    let clearance: Vec<_> = positions
        .iter()
        .filter(|r| feature(r).contains("grid"))
        .collect();
    let fitted: Vec<_> = positions
        .iter()
        .filter(|r| feature(r).starts_with("holes:"))
        .collect();
    assert!(!clearance.is_empty());
    assert!(clearance.iter().all(|r| r.get("mmc") == Some(&json!(true))));
    assert!(!fitted.is_empty());
    assert!(!fitted.iter().any(|r| r.contains_key("mmc")));
}

// test_mmc_finish_notes.py::test_the_parts_notes_are_written_and_read_back (the intent)
#[test]
fn the_parts_notes_are_written_and_read_back() {
    let (intent, _) = written();
    assert!(
        intent.part["coating"]
            .as_str()
            .unwrap()
            .starts_with("Anodise black")
    );
    assert_eq!(
        Value::Object(intent.part.clone()),
        json!({
            "material": "Aluminium 6082-T6",
            "general_tolerance": "ISO 2768-m",
            "surface_finish": "Ra 1.6",
            "coating": "Anodise black (MIL-A-8625 Type II)",
            "heat_treatment": "Solution treat and age to T6",
            "edges": "Break sharp edges 0.2 max",
        })
    );
}

// test_mmc_finish_notes.py::test_nothing_is_written_for_no_coating_or_heat_treatment (the
// intent)
#[test]
fn nothing_is_written_for_no_coating_or_heat_treatment() {
    let intent =
        apply_with_defaults(&plate(), &answers(json!({"part.material": "steel"})), true).unwrap();
    assert!(!intent.part.contains_key("coating"));
    assert!(!intent.part.contains_key("heat_treatment"));
}

// test_mmc_finish_notes.py::test_a_face_finish_is_written_on_its_face (the intent)
#[test]
fn a_face_finish_is_written_on_its_face() {
    let (intent, top) = written();
    let finishes: Vec<_> = intent
        .requirements
        .iter()
        .filter(|r| r["kind"] == "finish")
        .collect();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0]["value"], "Ra 0.8");
    assert_eq!(finishes[0]["faces"], json!([top]));
}

// test_mmc_finish_notes.py::test_a_finish_that_is_not_a_roughness_is_refused (every parameter)
#[test]
fn a_finish_that_is_not_a_roughness_is_refused() {
    let plate = plate();
    let face = plate["faces"][0]["id"].clone();
    for value in ["rough", "0", "Ra 99"] {
        let given =
            answers(json!({"extra.1": {"faces": [face], "kind": "finish", "value": value}}));
        match questions(&plate, &given) {
            Err(Error::Invalid(message)) => assert!(message.contains("roughness"), "{message}"),
            other => panic!("{value} not refused: {other:?}"),
        }
    }
}
