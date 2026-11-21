//! Pin the binding's compatibility to the independent Python-compatible core rules.
use serde_json::{Map, Value, json};
use specify_core::{api, edits, part, rules, tolerancing};
use specify_core_wasm::session::Session;

#[test]
fn text_answers_keep_core_truthiness_and_json_values() {
    let bytes = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let loaded = part::load_bytes("plate.step", bytes.to_vec()).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let session = Session::open("plate.step", bytes).unwrap();
    for id in ["part.material", "part.coating", "part.heat_treatment"] {
        for value in [
            json!(null),
            json!(""),
            json!(42),
            json!(false),
            json!(0),
            json!([]),
            json!({}),
            json!(["legacy"]),
        ] {
            let answers = Map::from_iter([(id.to_string(), value)]);
            let expected = api::review(&analysis, &answers).unwrap();
            let mut actual = session.review(&answers).unwrap();
            actual["intent"]
                .as_object_mut()
                .unwrap()
                .shift_remove("anchors");
            assert_eq!(actual, expected, "{answers:?}");
            assert_eq!(
                specify_core_wasm::accept_defaults(session.analysis(), &answers).unwrap(),
                rules::with_all_defaults(&analysis, &answers).unwrap()
            );
            // The engine's `edit` of these answers (a 0.2 answers map, read as fills) is the
            // library's edit writer's: the same bytes and report, or the same refusal. (Was:
            // the whole-intent `writePmi` against `writer::write_bytes`.)
            let text = Value::Object(answers.clone()).to_string();
            let written = session.edit(&text);
            match edits::read(&text).and_then(|l| specify_core::writer::apply::apply(&loaded, &l)) {
                Ok(applied) => {
                    assert_eq!(written["written"], json!(true), "{answers:?}: {written}");
                    assert_eq!(written["bytes"], json!(applied.bytes));
                    assert_eq!(written["report"], applied.to_dict());
                }
                Err(failures) => {
                    assert_eq!(written["written"], json!(false), "{answers:?}: {written}");
                    assert_eq!(written["failures"], json!(failures));
                }
            }
        }
    }
}

#[test]
fn stale_and_repeated_auto_entries_keep_core_settling_semantics() {
    let bytes = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let loaded = part::load_bytes("plate.step", bytes.to_vec()).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let session = Session::open("plate.step", bytes).unwrap();
    for gdt in [None, Some(tolerancing::Mode::Geometric)] {
        let state = tolerancing::State {
            answers: Map::from_iter([("part.material".into(), json!("steel"))]),
            auto: Some(vec![
                "missing".into(),
                "missing".into(),
                "part.material".into(),
            ]),
            gdt,
        };
        let (expected_review, expected_state) = tolerancing::settle(&analysis, &state).unwrap();
        let mut actual = session.settle(&state).unwrap();
        assert_eq!(actual["state"], json!(expected_state));
        assert_eq!(
            actual["review"],
            session.review(&expected_state.answers).unwrap()
        );
        actual["review"]["intent"]
            .as_object_mut()
            .unwrap()
            .shift_remove("anchors");
        assert_eq!(actual["review"], expected_review);
    }
}

#[test]
fn carried_refusals_survive_wasm_defaults_review_and_write() {
    let bytes = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let session = Session::open("plate.step", bytes).unwrap();
    let loaded = part::load_bytes("plate.step", bytes.to_vec()).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let mut answers = json!({"part.material":"steel", "_specify":{
        "unresolved":{"datum.A":"face 4 has no match in the new part"}
    }})
    .as_object()
    .unwrap()
    .clone();
    assert!(
        specify_core_wasm::accept_defaults(session.analysis(), &answers)
            .unwrap_err()
            .contains("carried answer could not be applied")
    );
    assert!(specify_core_wasm::accept_defaults(&analysis, &answers).is_err());
    assert!(
        !session.review(&answers).unwrap()["open"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let edit =
        |answers: &Map<String, Value>| session.edit(&Value::Object(answers.clone()).to_string());
    // Was: the whole-intent write refused the blocked set. An edit list writes only what it
    // fills (the material): the blocked datum A is never written.
    let blocked = edit(&answers);
    let bytes: Vec<u8> = serde_json::from_value(blocked["bytes"].clone()).unwrap();
    let existing =
        specify_core::existing::read_existing(&part::load_bytes("b.step", bytes).unwrap()).unwrap();
    assert!(existing.iter().all(|e| e.kind != "datum"), "{existing:?}");
    answers.insert("datum.A".into(), json!([4]));
    let accepted = specify_core_wasm::accept_defaults(session.analysis(), &answers).unwrap();
    assert_eq!(accepted["datum.A"], json!([4]));
    // The answers with the blocked datum answered save (an edit of them as fills).
    assert_eq!(edit(&answers)["written"], json!(true));
    answers["_specify"]["evidence"] = session.evidence().unwrap();
    assert_eq!(edit(&answers)["written"], json!(true));
    answers["_specify"]["evidence"]["face_count"] = json!(999);
    let stale = edit(&answers);
    assert_eq!(stale["written"], json!(false));
    assert!(
        stale["message"].as_str().unwrap().contains("stale answers"),
        "{stale}"
    );
}

/// ADR P9: the session's export takes exactly the defaults approved for its part, with the
/// values the person saw, as `api::export_answers` does; none without approvals.
#[test]
fn export_takes_only_the_defaults_approved_for_its_part() {
    let bytes = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let loaded = part::load_bytes("plate.step", bytes.to_vec()).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let session = Session::open("plate.step", bytes).unwrap();
    let given = Map::from_iter([("part.material".to_string(), json!("steel"))]);
    let default = api::questions(&analysis, &given)
        .unwrap()
        .into_iter()
        .find(|q| q.id == "part.general_tolerance")
        .and_then(|q| q.default)
        .unwrap();
    let approve = |part: usize, value: serde_json::Value| {
        api::Approved::from([(
            part,
            Map::from_iter([("part.general_tolerance".to_string(), value)]),
        )])
    };
    let without = |mut m: Map<String, serde_json::Value>| {
        m.remove("_specify");
        m
    };
    let none = session
        .export_answers(&given, &api::Approved::new())
        .unwrap();
    assert_eq!(without(none.clone()), given);
    let one = session
        .export_answers(&given, &approve(0, default.clone()))
        .unwrap();
    let mut expected = given.clone();
    expected.insert("part.general_tolerance".into(), default.clone());
    assert_eq!(without(one.clone()), expected);
    assert_eq!(
        one,
        api::export_answers(&loaded, &analysis, &given, &approve(0, default.clone()))
            .unwrap()
            .answers
    );
    let refused = session
        .export_answers(&given, &approve(0, json!("changed")))
        .unwrap_err();
    assert!(
        refused.contains("part 0: part.general_tolerance's default is now"),
        "{refused}"
    );
    assert_eq!(
        session
            .export_answers(&given, &approve(1, default))
            .unwrap(),
        none,
        "another part's approval does not apply"
    );
}
