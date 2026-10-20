//! Exercise the native implementation of IntentEngine, including verified PMI readback.
use serde_json::{Map, json};
use specify_core_wasm::session::Session;

#[test]
fn answering_material_does_not_write_unanswered_defaults() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts/GRM-02_shaft.step");
    let session = Session::open("shaft.step", &std::fs::read(path).unwrap()).unwrap();
    for answers in [
        Map::new(),
        json!({"part.material": "steel"})
            .as_object()
            .unwrap()
            .clone(),
    ] {
        let review = session.review(&answers).unwrap();
        assert!(review["intent"]["datums"].as_array().unwrap().is_empty());
        assert!(
            review["intent"]["requirements"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(review["intent"]["part"].get("general_tolerance").is_none());
        assert!(
            review["open"]
                .as_array()
                .unwrap()
                .contains(&json!("part.general_tolerance"))
        );

        let state = specify_core::tolerancing::State {
            answers: answers.clone(),
            auto: Some(vec![]),
            gdt: None,
        };
        let settled = session.settle(&state).unwrap();
        assert!(
            settled["review"]["intent"]["datums"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            settled["review"]["intent"]["part"]
                .get("general_tolerance")
                .is_none()
        );

        let written = session.write_pmi(&answers, None).unwrap();
        let bytes: Vec<u8> = serde_json::from_value(written["bytes"].clone()).unwrap();
        let reopened = Session::open("shaft-written.step", &bytes)
            .unwrap()
            .snapshot();
        assert_eq!(reopened["analysis"]["resume"]["answers"], json!(answers));
        // A resumed analysis hides its PMI in favour of stored answers; inspect the actual
        // semantic records independently to prove which defaults were (not) written.
        let loaded = specify_core::part::load_bytes("shaft-written.step", bytes).unwrap();
        let existing = specify_core::existing::read_existing(&loaded).unwrap();
        assert!(
            existing
                .iter()
                .all(|p| p.kind != "datum" && p.kind != "geometric_tolerance")
        );
        let settings = specify_core::existing::part_settings(&loaded).unwrap();
        assert!(settings.get("general_tolerance").is_none());
        assert_eq!(settings.get("material"), answers.get("part.material"));
    }
}

#[test]
fn accepting_one_default_writes_only_that_default() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts/plate.step");
    let session = Session::open("plate.step", &std::fs::read(path).unwrap()).unwrap();
    let initial = session.review(&Map::new()).unwrap();
    let datum = initial["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"] == "datum.A")
        .unwrap();
    let given = json!({"datum.A": datum["default"]})
        .as_object()
        .unwrap()
        .clone();
    let review = session.review(&given).unwrap();
    assert_eq!(review["intent"]["datums"].as_array().unwrap().len(), 1);
    assert!(
        review["intent"]["requirements"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        review["open"]
            .as_array()
            .unwrap()
            .contains(&json!("datum.B"))
    );
    let written = session.write_pmi(&given, None).unwrap();
    let bytes: Vec<u8> = serde_json::from_value(written["bytes"].clone()).unwrap();
    let loaded = specify_core::part::load_bytes("plate-written.step", bytes).unwrap();
    let existing = specify_core::existing::read_existing(&loaded).unwrap();
    let datums: Vec<_> = existing.iter().filter(|p| p.kind == "datum").collect();
    assert_eq!(datums.len(), 1);
    assert_eq!(datums[0].r#type, "A");
}
