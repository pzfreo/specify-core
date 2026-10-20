//! Issue #20: answering one question must not accept the other questions' defaults.
mod common;

use serde_json::{Map, Value, json};
use specify_core::{api, part, serve::Server, tolerancing};

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn check_suggestions(analysis: &Value, given: &Map<String, Value>) {
    let review = api::review(analysis, given).unwrap();
    assert!(review["intent"]["datums"].as_array().unwrap().is_empty());
    assert!(review["intent"]["part"].get("general_tolerance").is_none());
    assert!(review["intent"]["general"].as_array().unwrap().is_empty());
    assert!(
        review["intent"]["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| {
                !matches!(
                    r["kind"].as_str(),
                    Some("position" | "runout" | "flatness" | "perpendicularity")
                )
            })
    );
    for id in ["datum.A", "datum.B", "datum.C", "part.general_tolerance"] {
        assert!(
            review["open"].as_array().unwrap().contains(&json!(id)),
            "{id}"
        );
        let q = review["questions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["id"] == id)
            .unwrap();
        assert!(!q["default"].is_null(), "{q}");
        let reason = q.get("basis").unwrap_or(&q["why"]);
        assert!(!reason.as_str().unwrap().is_empty(), "{q}");
    }
    let served = Server::default()
        .reply(&json!({"id": 1, "op": "preview", "analysis": analysis, "answers": given}));
    assert_eq!(served["result"]["intent"], review["intent"]);
    assert!(api::apply(analysis, given, false).is_err());
    if given.contains_key("part.material") {
        let accepted = api::apply(analysis, given, true).unwrap();
        assert_eq!(accepted.part["general_tolerance"], "ISO 2768-m");
        assert!(!accepted.datums.is_empty());
    }

    // #16's automatic ±/plain answers must not accept datum or tolerance suggestions either.
    let state = tolerancing::State {
        answers: given.clone(),
        auto: Some(vec![]),
        gdt: None,
    };
    let (settled, state) = tolerancing::settle(analysis, &state).unwrap();
    assert!(settled["intent"]["datums"].as_array().unwrap().is_empty());
    assert!(settled["intent"]["part"].get("general_tolerance").is_none());
    assert!(!state.answers.contains_key("part.general_tolerance"));
    assert!(!state.answers.contains_key("datum.A"));
}

#[test]
fn grm02_material_finish_and_holes_leave_the_general_tolerance_unanswered() {
    // GRM-02 is absent from the supplied corpus; the repository carries the original STEP.
    let path = common::fixtures().join("parts/GRM-02_shaft.step");
    let analysis = api::analyse_file(&path, None).unwrap();
    let mut given = answers(json!({"part.material": "steel", "part.surface_finish": "Ra 1.6"}));
    for q in api::questions(&analysis, &given).unwrap() {
        if q.id.starts_with("hole.function:") {
            given.insert(q.id, json!("general"));
        }
    }
    check_suggestions(&analysis, &Map::new());
    check_suggestions(&analysis, &given);
}

#[test]
fn grm03_tap_thread_and_knurl_do_not_add_datums_or_position() {
    let Some(root) = common::corpus_dir() else {
        return;
    };
    let path = root.join("gramel/GRM-03_thumbwheel_drive_screw.step");
    let loaded = part::load(&path, None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let given = answers(json!({
        "hole.function:holes:14": "tapped:M2",
        "diameter.fit:turned_steps:6": "thread:M3",
        "diameter.fit:turned_steps:3": "knurl:straight",
    }));
    check_suggestions(&analysis, &Map::new());
    check_suggestions(&analysis, &given);
    let verbatim = api::preview(&analysis, &given).unwrap();
    assert_eq!(
        verbatim.requirements.len(),
        2,
        "the issue's stale hole id must not accept the tap suggestion"
    );
    let hole = api::questions(&analysis, &given)
        .unwrap()
        .into_iter()
        .find(|q| {
            q.id.starts_with("hole.function:") && q.diameter.is_some_and(|d| (d - 1.6).abs() < 1e-6)
        })
        .unwrap();
    let mut given = given;
    given.insert(hole.id, json!("tapped:M2"));
    check_suggestions(&analysis, &given);
    let intent = api::preview(&analysis, &given).unwrap();
    assert_eq!(intent.requirements.len(), 3, "{intent:?}");
    assert_eq!(
        intent
            .requirements
            .iter()
            .map(|r| r["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["thread", "knurl", "thread"]
    );
    let (bytes, _) = specify_core::writer::write_bytes(&loaded, &intent, Some(&given)).unwrap();
    let reopened = part::load_bytes("grm03.step", bytes).unwrap();
    let reread = api::analyse(&reopened).unwrap();
    assert_eq!(reread["resume"]["answers"], json!(given));
    // Analyse intentionally hides stored PMI when it resumes answers. Read semantic PMI
    // directly as well, so these absence assertions cannot pass just because it resumed.
    assert!(
        specify_core::existing::part_settings(&reopened)
            .unwrap()
            .get("general_tolerance")
            .is_none()
    );
    assert!(
        specify_core::existing::read_existing(&reopened)
            .unwrap()
            .iter()
            .all(|p| { p.kind != "datum" && p.kind != "geometric_tolerance" })
    );
}
