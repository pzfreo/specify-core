//! The intent the answers so far amount to, defaults standing in: specify-core's
//! `tests/test_preview.py`, on the oracle's captured analysis of the plate. Label anchors for
//! the preview are stage 4's (serve).

mod common;

use serde_json::{Map, Value, json};
use specify_core::rules::{Error, apply, preview, with_all_defaults};

fn plate() -> Value {
    common::case("parts/plate")
        .artifact("analysis")
        .expect("a captured analysis")
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

// test_preview.py::test_nothing_answered_shows_the_defaults
#[test]
fn nothing_answered_shows_the_defaults() {
    let intent = preview(&plate(), &Map::new()).unwrap();
    let letters: Vec<&Value> = intent.datums.iter().map(|d| &d["letter"]).collect();
    assert_eq!(letters, [&json!("A"), &json!("B"), &json!("C")]);
    assert!(!intent.requirements.is_empty());
    assert!(
        intent
            .requirements
            .iter()
            .all(|r| r["default"] == json!(true))
    );
    assert!(intent.datums.iter().all(|d| d["default"] == json!(true)));
    assert!(!intent.part.contains_key("material")); // no answer and no default
}

// test_preview.py::test_an_answer_is_shown_as_decided
#[test]
fn an_answer_is_shown_as_decided() {
    let plate = plate();
    let grid = plate["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .find(|id| id.starts_with("hole_patterns.grid"))
        .unwrap();
    let position = format!("hole.position:{grid}");
    let given = answers(json!({ position.clone(): "0.2", "part.material": "6082-T6" }));
    let intent = preview(&plate, &given).unwrap();
    let found: Vec<_> = intent
        .requirements
        .iter()
        .filter(|r| r.get("feature") == Some(&json!(grid)) && r["kind"] == "position")
        .collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["tolerance"].as_f64(), Some(0.2));
    assert_eq!(found[0]["default"], json!(false));
    assert_eq!(found[0]["question"], json!(position));
    assert_eq!(intent.part["material"], "6082-T6");
}

// test_preview.py::test_every_item_names_its_question
#[test]
fn every_item_names_its_question() {
    let plate = plate();
    let mut given = answers(json!({"part.material": "steel"}));
    for (k, v) in with_all_defaults(&plate, &Map::new()).unwrap() {
        given.insert(k, v);
    }
    let intent = apply(&plate, &given).unwrap();
    assert!(
        intent
            .datums
            .iter()
            .chain(&intent.requirements)
            .all(|item| item.contains_key("question"))
    );
    let mut questions: Vec<&Value> = intent.datums.iter().map(|d| &d["question"]).collect();
    questions.sort_by_key(|q| q.to_string());
    questions.dedup();
    assert_eq!(
        questions,
        [&json!("datum.A"), &json!("datum.B"), &json!("datum.C")]
    );
}

// test_preview.py::test_a_refused_answer_is_refused_in_preview_too
#[test]
fn a_refused_answer_is_refused_in_preview_too() {
    match preview(&plate(), &answers(json!({"datum.A": [99999]}))) {
        Err(Error::Invalid(message)) => assert!(message.contains("datum A"), "{message}"),
        other => panic!("not refused: {other:?}"),
    }
}
