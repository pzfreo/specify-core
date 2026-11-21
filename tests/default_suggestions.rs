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
    // ADR P11: a gap whose suggestion is "none" is not open; datum.C's suggestion is none.
    let c = review["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"] == "datum.C")
        .unwrap();
    assert!(
        specify_core::feature_table::is_none_value(&c["default"]),
        "{c}"
    );
    assert!(
        !review["open"]
            .as_array()
            .unwrap()
            .contains(&json!("datum.C"))
    );
    for id in ["datum.A", "datum.B", "part.general_tolerance"] {
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
    // D11 (#126): the issue's stale hole id names nothing the part has, so the whole list is
    // refused, named with its reason, rather than the stale answer being ignored.
    let stale_error = "Invalid answers: hole.function:holes:14: unknown feature holes:14";
    assert_eq!(
        api::review(&analysis, &given).unwrap_err().to_string(),
        stale_error
    );
    assert_eq!(
        api::preview(&analysis, &given).unwrap_err().to_string(),
        stale_error
    );
    let mut given = given;
    let stale = given.shift_remove("hole.function:holes:14").unwrap();
    check_suggestions(&analysis, &given);
    let verbatim = api::preview(&analysis, &given).unwrap();
    assert_eq!(
        verbatim.requirements.len(),
        2,
        "without the stale id, only the thread and knurl are answered"
    );
    let hole = api::questions(&analysis, &given)
        .unwrap()
        .into_iter()
        .find(|q| {
            q.id.starts_with("hole.function:") && q.diameter.is_some_and(|d| (d - 1.6).abs() < 1e-6)
        })
        .unwrap();
    assert_eq!(
        hole.id, "hole.function:holes:12.13",
        "quiddity #216 includes planar floor 13 in the Ø1.6 hole identity"
    );
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
    // The writer refuses the stale id too, whatever intent it is given (D11).
    let mut with_stale = given.clone();
    with_stale.insert("hole.function:holes:14".into(), stale);
    let refused = specify_core::writer::write_bytes(&loaded, &intent, Some(&with_stale))
        .unwrap_err()
        .to_string();
    assert!(refused.contains(stale_error), "{refused}");
    let (bytes, _) = specify_core::writer::write_bytes(&loaded, &intent, Some(&given)).unwrap();
    let reopened = part::load_bytes("grm03.step", bytes).unwrap();
    let reread = api::analyse(&reopened).unwrap();
    // ADR P11 rule 2: every answer here writes PMI (two threads and a knurl), so none is
    // stored: the file states them, and nothing is left to resume or report.
    assert_eq!(reread["resume"]["answers"], json!({}));
    assert!(reread["resume"].get("unresolved").is_none());
    assert!(reread["resume"].get("dropped_answers").is_none());
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

#[test]
fn grm03_answered_position_reports_its_pending_frame_then_is_written() {
    let Some(root) = common::corpus_dir() else {
        return;
    };
    let loaded = part::load(
        &root.join("gramel/GRM-03_thumbwheel_drive_screw.step"),
        None,
    )
    .unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let qs = api::questions(&analysis, &Map::new()).unwrap();
    let position = qs
        .iter()
        .find(|q| q.id == "hole.position:holes:12.13")
        .cloned()
        .unwrap();
    let mut given = answers(json!({position.id.clone(): "0.2"}));
    let review = api::review(&analysis, &given).unwrap();
    let pending = review["problems"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["code"] == "awaits_datum_frame" && p["about"] == position.id)
        .unwrap();
    assert_eq!(pending["awaiting"], json!(["A"]));
    assert!(
        review["intent"]["requirements"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !review["open"]
            .as_array()
            .unwrap()
            .contains(&json!(position.id))
    );
    let served = Server::default()
        .reply(&json!({"id": 1, "op": "questions", "analysis": analysis, "answers": given}));
    assert_eq!(served["result"]["problems"], review["problems"]);
    for q in qs {
        if ["datum.A", "datum.B", "datum.C"].contains(&q.id.as_str()) {
            given.insert(q.id, q.default.unwrap());
        }
    }
    let review = api::review(&analysis, &given).unwrap();
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["code"] != "awaits_datum_frame")
    );
    let intent = api::preview(&analysis, &given).unwrap();
    assert!(
        intent
            .requirements
            .iter()
            .any(|r| r["question"] == position.id && r["kind"] == "position")
    );
    let (bytes, _) = specify_core::writer::write_bytes(&loaded, &intent, Some(&given)).unwrap();
    let reopened = part::load_bytes("grm03-position.step", bytes).unwrap();
    assert!(
        specify_core::existing::read_existing(&reopened)
            .unwrap()
            .iter()
            .any(|p| p.kind == "geometric_tolerance" && p.r#type.starts_with("Position"))
    );
}

fn check_accepted_defaults(path: &std::path::Path, given: Map<String, Value>, no_pmi: Value) {
    let loaded = part::load(path, None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let accepted = specify_core::rules::with_all_defaults(&analysis, &given).unwrap();
    assert_eq!(
        specify_core::rules::with_all_defaults(&analysis, &accepted).unwrap(),
        accepted
    );
    for (id, value) in &given {
        assert_eq!(accepted.get(id), Some(value));
    }
    let review = api::review(&analysis, &accepted).unwrap();
    assert!(review["open"].as_array().unwrap().is_empty());
    assert_eq!(review["intent"]["part"]["general_tolerance"], "ISO 2768-m");
    let expected = api::apply(&analysis, &given, true).unwrap();
    let mut preview = api::preview(&analysis, &accepted).unwrap();
    for item in preview
        .datums
        .iter_mut()
        .chain(preview.requirements.iter_mut())
    {
        if item.contains_key("not_written_reason") {
            // Planner diagnostics are reported proposals, not accepted interview
            // requirements. They have no question/default status (#75).
            assert!(!item.contains_key("question"));
            assert!(!item.contains_key("default"));
        } else {
            assert_eq!(item.remove("default"), Some(json!(false)));
        }
    }
    assert_eq!(preview.to_dict(), expected.to_dict());
    assert!(!expected.datums.is_empty());
    assert!(
        expected
            .requirements
            .iter()
            .any(|r| r["kind"] == "position")
    );
    let (actual_bytes, _) =
        specify_core::writer::write_bytes(&loaded, &preview, Some(&accepted)).unwrap();
    let (expected_bytes, _) =
        specify_core::writer::write_bytes(&loaded, &expected, Some(&accepted)).unwrap();
    assert_eq!(actual_bytes, expected_bytes);
    let reopened = part::load_bytes("accepted.step", actual_bytes).unwrap();
    let resumed = api::analyse(&reopened).unwrap()["resume"]["answers"].clone();
    // ADR P11 rule 2: the approval the planner wrote nothing for (a ⌀ on a datum's own faces)
    // is stored too, listed as such.
    assert_eq!(resumed["_specify"]["no_pmi"], no_pmi);
    let mut accepted = accepted;
    if !resumed["_specify"].is_null() {
        accepted.insert("_specify".into(), resumed["_specify"].clone());
    }
    assert_eq!(
        resumed,
        // ADR P11: stored are the answers that leave no PMI behind.
        json!(specify_core::feature_table::storable(&accepted))
    );
    assert_eq!(
        specify_core::existing::part_settings(&reopened).unwrap()["general_tolerance"],
        "ISO 2768-m"
    );
    assert!(
        specify_core::existing::read_existing(&reopened)
            .unwrap()
            .iter()
            .any(|p| p.kind == "geometric_tolerance" && p.r#type == "Position")
    );
}

#[test]
fn grm02_explicit_bulk_acceptance_matches_apply_with_defaults() {
    check_accepted_defaults(
        &common::fixtures().join("parts/GRM-02_shaft.step"),
        answers(json!({"part.material": "steel", "part.surface_finish": "Ra 1.6"})),
        Value::Null,
    );
}

#[test]
fn grm03_explicit_bulk_acceptance_matches_apply_with_defaults() {
    let Some(root) = common::corpus_dir() else {
        return;
    };
    check_accepted_defaults(
        &root.join("gramel/GRM-03_thumbwheel_drive_screw.step"),
        answers(json!({
            "part.material": "steel",
            "hole.function:holes:12.13": "tapped:M2",
            "diameter.fit:turned_steps:6": "thread:M3",
            "diameter.fit:turned_steps:3": "knurl:straight",
        })),
        json!(["diameter.fit:turned_steps:2"]),
    );
}
