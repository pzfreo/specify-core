//! Every default specify-core chooses says why, so it can be shown as a choice made:
//! specify-core's `tests/test_basis.py`, on the oracle's captured analysis of the plate.

mod common;

use serde_json::{Map, json};
use specify_core::rules::{Question, questions};

fn plate_questions() -> Vec<Question> {
    let plate = common::case("parts/plate")
        .artifact("analysis")
        .expect("a captured analysis");
    questions(&plate, &Map::new()).unwrap()
}

fn by_id<'q>(qs: &'q [Question], id: &str) -> &'q Question {
    qs.iter()
        .find(|q| q.id == id)
        .unwrap_or_else(|| panic!("no question {id}"))
}

// test_basis.py::test_the_choices_made_for_a_person_say_why
#[test]
fn the_choices_made_for_a_person_say_why() {
    let qs = plate_questions();
    assert!(
        by_id(&qs, "datum.A")
            .basis
            .starts_with("The largest flat face")
    );
    assert_eq!(
        by_id(&qs, "datum.B").basis,
        "The largest flat face square to A."
    );
    assert!(
        by_id(&qs, "part.general_tolerance")
            .basis
            .starts_with("ISO 2768-m")
    );
    let grid = qs
        .iter()
        .find(|q| q.id.starts_with("hole.function:hole_patterns"))
        .unwrap();
    assert_eq!(grid.basis, "⌀6.6 is an ISO 273 clearance hole for M6.");
    assert!(
        by_id(&qs, &grid.id.replace("function", "position"))
            .basis
            .contains("H − F")
    );
    // Python's {prefix: q} keeps the last question of each prefix.
    let last_hole = qs
        .iter()
        .rfind(|q| q.id.split(':').next() == Some("hole.function"))
        .unwrap();
    assert!(last_hole.to_dict().get("basis").is_some());
}

// test_basis.py::test_a_hole_left_to_the_general_tolerance_has_no_choice_to_explain
#[test]
fn a_hole_left_to_the_general_tolerance_has_no_choice_to_explain() {
    let qs = plate_questions();
    let plain: Vec<&Question> = qs
        .iter()
        .filter(|q| q.id.starts_with("hole.function") && q.default == Some(json!("general")))
        .collect();
    assert!(!plain.is_empty());
    assert!(
        plain
            .iter()
            .all(|q| q.basis.is_empty() && q.to_dict().get("basis").is_none())
    );
}
