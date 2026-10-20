//! The intent the answers so far amount to, with defaults remaining suggestions (#20).
//! Adapted from `tests/test_preview.py`, on the oracle's captured analysis of the plate. Label anchors for
//! the preview are stage 4's (serve).

mod common;

use serde_json::{Map, Value, json};
use specify_core::rules::{Error, apply, preview, questions, with_all_defaults};

fn plate() -> Value {
    common::case("parts/plate")
        .artifact("analysis")
        .expect("a captured analysis")
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

// #20 intentionally departs from Python: suggestions must not become drawn/written PMI.
#[test]
fn nothing_answered_keeps_the_defaults_on_questions() {
    let intent = preview(&plate(), &Map::new()).unwrap();
    assert!(intent.datums.is_empty());
    assert!(intent.requirements.is_empty());
    assert!(intent.general.is_empty());
    assert!(!intent.part.contains_key("general_tolerance"));
    assert!(!intent.part.contains_key("material")); // no answer and no default
    let qs = questions(&plate(), &Map::new()).unwrap();
    for id in ["datum.A", "datum.B", "datum.C", "part.general_tolerance"] {
        let q = qs.iter().find(|q| q.id == id).unwrap();
        assert!(q.default.is_some());
        assert!(!q.basis.is_empty());
    }
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
    let mut given = answers(json!({ position.clone(): "0.2", "part.material": "6082-T6" }));
    let pending = preview(&plate, &given).unwrap();
    assert!(pending.datums.is_empty());
    assert!(
        pending.requirements.is_empty(),
        "an unanswered datum frame cannot be written"
    );
    for q in questions(&plate, &given).unwrap() {
        if ["datum.A", "datum.B", "datum.C"].contains(&q.id.as_str()) {
            given.insert(q.id, q.default.unwrap());
        }
    }
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

#[test]
fn datum_controls_wait_for_their_datum_answers() {
    let plate = plate();
    for id in ["datum.A.flatness", "datum.B.control", "datum.C.control"] {
        let mut given = answers(json!({id: "0.05"}));
        let pending = preview(&plate, &given).unwrap();
        assert!(pending.datums.is_empty());
        assert!(pending.requirements.is_empty(), "{id}: {pending:?}");
        for q in questions(&plate, &given).unwrap() {
            if ["datum.A", "datum.B", "datum.C"].contains(&q.id.as_str()) {
                given.insert(q.id, q.default.unwrap());
            }
        }
        let accepted = preview(&plate, &given).unwrap();
        assert_eq!(accepted.datums.len(), 3);
        assert_eq!(accepted.requirements.len(), 1);
        assert_eq!(accepted.requirements[0]["question"], id);
        assert_eq!(accepted.requirements[0]["tolerance"], 0.05);
    }
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
