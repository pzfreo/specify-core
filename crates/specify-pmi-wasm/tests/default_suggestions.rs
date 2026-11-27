//! Exercise the native implementation of IntentEngine, including verified PMI readback. Saves
//! go through the engine's `edit` (Specify 0.3.0): the answers given, as a 0.2 answers map read
//! as fills; a default is written only when it is named.
use serde_json::{Map, Value, json};
use specify_pmi_wasm::session::Session;

/// The engine's `edit` of `answers` (read as fills): the result, written or refused.
fn edit(session: &Session, answers: &Map<String, Value>) -> Value {
    session.edit(&Value::Object(answers.clone()).to_string())
}

/// [`edit`], written: the file's bytes.
fn written(session: &Session, answers: &Map<String, Value>) -> Vec<u8> {
    let out = edit(session, answers);
    assert_eq!(out["written"], json!(true), "{out}");
    serde_json::from_value(out["bytes"].clone()).unwrap()
}

/// The free `acceptDefaults` on the session's analysis (the engine's own method is gone).
fn accept_defaults(
    session: &Session,
    answers: &Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    specify_pmi_wasm::accept_defaults(session.analysis(), answers)
}

/// The answers a file stores, `{}` when it stores none.
fn stored(snapshot: &Value) -> Value {
    json!(
        snapshot["analysis"]["resume"]["answers"]
            .as_object()
            .cloned()
            .unwrap_or_default()
    )
}

#[test]
fn alpha7_answered_tap_keeps_computed_depth_in_write_pmi_and_resave() {
    use haecceity::{p21::Document, pmi, step::read_part_definitions};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let baseline =
        specify_pmi::part::load(&root.join("thread-depth-alpha7.step.gz"), None).unwrap();
    // The answers the alpha.7 file stored (ADR P11: reopening resumes only those that leave no
    // PMI behind, so they are read from the payload).
    let given = specify_pmi::resume::answers_for(&baseline)
        .unwrap()
        .unwrap();
    for (bare, bytes) in [
        (
            true,
            std::fs::read(root.join("parts/GRM-02_shaft.step")).unwrap(),
        ),
        (false, baseline.bytes.to_vec()),
    ] {
        let session = Session::open("shaft.step", &bytes).unwrap();
        // ADR P11: the saved file states its thread, so it is not asked again; its resave
        // keeps the thread and its computed depth as they are.
        if bare {
            let review = session.review(&given).unwrap();
            let q = review["questions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|q| q["id"].as_str().unwrap().starts_with("hole.full_thread:"))
                .unwrap();
            assert_eq!(q["attention"], "refine");
            assert!(!review["open"].as_array().unwrap().contains(&q["id"]));
            assert!(!given.contains_key(q["id"].as_str().unwrap()));
            assert_eq!(
                review["intent"]["requirements"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["kind"] == "thread")
                    .unwrap()["full_thread"],
                2.0
            );
        }
        let bytes = written(&session, &given);
        let defs = read_part_definitions(&bytes).unwrap();
        let doc = Document::parse(bytes).unwrap();
        let read = pmi::read(&doc, &defs).unwrap();
        assert_eq!(read.parts[0].threads.len(), 1);
        assert!(read.parts[0].attributes.iter().flat_map(|a| &a.items).any(|(n,v)| n == "minimum full thread" && matches!(v, pmi::AttributeValue::Measure(m) if matches!(&m.quantity, pmi::Quantity::Length(l) if l.mm() == 2.0))));
    }
}

#[test]
fn full_thread_edit_is_optional_and_survives_a_session_write() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts/GRM-02_shaft.step");
    let session = Session::open("shaft.step", &std::fs::read(path).unwrap()).unwrap();
    let a = session.snapshot()["analysis"].clone();
    let review = session.review(&Map::new()).unwrap();
    let q = review["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"].as_str().unwrap().starts_with("hole.full_thread:"))
        .unwrap();
    let id = q["id"].as_str().unwrap();
    assert_eq!(q["kind"], "text");
    assert_eq!(q["default"], "2");
    assert_eq!(q["attention"], "refine");
    assert!(!review["open"].as_array().unwrap().contains(&json!(id)));
    let function = id.replace("hole.full_thread:", "hole.function:");
    let given = json!({"part.material":"steel", function.as_str():"tapped:M2", id:"1.8"})
        .as_object()
        .unwrap()
        .clone();
    let accepted = accept_defaults(&session, &given).unwrap();
    assert_eq!(accepted[id], "1.8");
    assert_eq!(
        accepted,
        specify_pmi_wasm::accept_defaults(&a, &given).unwrap()
    );
    let review = session.review(&given).unwrap();
    assert_eq!(
        review["intent"]["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == "thread")
            .unwrap()["full_thread"],
        1.8
    );
    let bytes = written(&session, &given);
    // ADR P11: the depth is the file's thread PMI, not a stored answer.
    let reopened = Session::open("edited.step", &bytes).unwrap();
    assert!(
        reopened.snapshot()["analysis"]["resume"]["answers"]
            .get(id)
            .is_none()
    );
    let defs = haecceity::step::read_part_definitions(&bytes).unwrap();
    let read = haecceity::pmi::read(
        &haecceity::p21::Document::parse(bytes.clone()).unwrap(),
        &defs,
    )
    .unwrap();
    assert!(read.parts[0].attributes.iter().flat_map(|a| &a.items).any(|(n, v)| n == "minimum full thread"
        && matches!(v, haecceity::pmi::AttributeValue::Measure(m) if matches!(&m.quantity, haecceity::pmi::Quantity::Length(l) if l.mm() == 1.8))));
    let bad = json!({function:"tapped:M2", id:"3"})
        .as_object()
        .unwrap()
        .clone();
    assert!(session.review(&bad).is_err());
    assert!(accept_defaults(&session, &bad).is_err());
}

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

        let state = specify_pmi::tolerancing::State {
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

        let bytes = written(&session, &answers);
        let reopened = Session::open("shaft-written.step", &bytes)
            .unwrap()
            .snapshot();
        // ADR P11: only answers that leave no PMI behind are stored.
        assert_eq!(
            stored(&reopened),
            json!(specify_pmi::feature_table::storable(&answers))
        );
        // A resumed analysis hides its PMI in favour of stored answers; inspect the actual
        // semantic records independently to prove which defaults were (not) written.
        let loaded = specify_pmi::part::load_bytes("shaft-written.step", bytes).unwrap();
        let existing = specify_pmi::existing::read_existing(&loaded).unwrap();
        assert!(
            existing
                .iter()
                .all(|p| p.kind != "datum" && p.kind != "geometric_tolerance")
        );
        let settings = specify_pmi::existing::part_settings(&loaded).unwrap();
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
    let bytes = written(&session, &given);
    let loaded = specify_pmi::part::load_bytes("plate-written.step", bytes).unwrap();
    let existing = specify_pmi::existing::read_existing(&loaded).unwrap();
    let datums: Vec<_> = existing.iter().filter(|p| p.kind == "datum").collect();
    assert_eq!(datums.len(), 1);
    assert_eq!(datums[0].r#type, "A");
}

#[test]
fn session_reports_an_answered_control_until_its_frame_is_accepted() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts/plate.step");
    let session = Session::open("plate.step", &std::fs::read(path).unwrap()).unwrap();
    let initial = session.review(&Map::new()).unwrap();
    let questions = initial["questions"].as_array().unwrap();
    let position = questions
        .iter()
        .find(|q| {
            q["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("hole.position:"))
        })
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let mut given = json!({position: "0.2"}).as_object().unwrap().clone();
    let review = session.review(&given).unwrap();
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["code"] == "awaits_datum_frame" && p["about"] == position)
    );
    // Was: written without its position (the frame is not accepted). An edit list refuses the
    // position, naming the datums to fill in the same list, and writes nothing.
    let pending = edit(&session, &given);
    assert_eq!(pending["written"], json!(false), "{pending}");
    assert!(
        pending["message"].as_str().unwrap().contains("datum.A"),
        "{pending}"
    );
    for q in questions {
        if q["id"]
            .as_str()
            .is_some_and(|id| ["datum.A", "datum.B", "datum.C"].contains(&id))
        {
            given.insert(q["id"].as_str().unwrap().into(), q["default"].clone());
        }
    }
    let review = session.review(&given).unwrap();
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["code"] != "awaits_datum_frame")
    );
    let bytes = written(&session, &given);
    let loaded = specify_pmi::part::load_bytes("accepted.step", bytes).unwrap();
    assert!(
        specify_pmi::existing::read_existing(&loaded)
            .unwrap()
            .iter()
            .any(|p| p.kind == "geometric_tolerance" && p.r#type == "Position")
    );
}

fn check_bulk_acceptance(path: &std::path::Path, given: Map<String, Value>, no_pmi: Value) {
    let bytes = std::fs::read(path).unwrap();
    let session = Session::open("accepted.step", &bytes).unwrap();
    let analysis = session.snapshot()["analysis"].clone();
    let accepted = accept_defaults(&session, &given).unwrap();
    assert_eq!(
        accepted,
        specify_pmi_wasm::accept_defaults(&analysis, &given).unwrap()
    );
    assert_eq!(accepted, accept_defaults(&session, &accepted).unwrap());
    for (id, value) in &given {
        assert_eq!(accepted.get(id), Some(value));
    }
    let review = session.review(&accepted).unwrap();
    assert!(review["open"].as_array().unwrap().is_empty());
    assert_eq!(review["intent"]["part"]["general_tolerance"], "ISO 2768-m");
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["code"] != "awaits_datum_frame")
    );
    let state = specify_pmi::tolerancing::State {
        answers: accepted.clone(),
        auto: Some(vec![]),
        gdt: None,
    };
    let settled = session.settle(&state).unwrap();
    assert_eq!(settled["state"]["answers"], json!(accepted));
    assert_eq!(settled["review"]["intent"], review["intent"]);

    let loaded = specify_pmi::part::load_bytes("accepted.step", bytes).unwrap();
    let expected = specify_pmi::api::apply(&analysis, &given, true).unwrap();
    let mut actual = review["intent"].clone();
    actual.as_object_mut().unwrap().remove("anchors");
    for key in ["datums", "requirements"] {
        for item in actual[key].as_array_mut().unwrap() {
            if item.get("not_written_reason").is_some() {
                // Rejected planner proposals have no interview/default status.
                assert!(item.get("question").is_none());
                assert!(item.get("default").is_none());
            } else {
                assert_eq!(
                    item.as_object_mut().unwrap().remove("default"),
                    Some(json!(false))
                );
            }
        }
    }
    assert_eq!(actual, expected.to_dict());
    // The engine's edit of every accepted answer is the library's edit writer's, byte for byte
    // (was: the whole-intent write's, `writer::write_bytes`).
    let bytes = written(&session, &accepted);
    let list = specify_pmi::edits::read(&Value::Object(accepted.clone()).to_string()).unwrap();
    let expected_bytes = specify_pmi::writer::apply::apply(&loaded, &list)
        .unwrap()
        .bytes;
    assert_eq!(bytes, expected_bytes);
    // ADR P11: only answers that leave no PMI behind are stored; rule 2: with the approval
    // the planner wrote nothing for (GRM-03's ⌀ on a datum's own faces), listed as such.
    let reopened = Session::open("accepted.step", &bytes).unwrap();
    let resumed = reopened.snapshot()["analysis"]["resume"]["answers"].clone();
    assert_eq!(resumed["_specify"]["no_pmi"], no_pmi);
    let mut accepted = accepted.clone();
    if !no_pmi.is_null() {
        accepted.insert("_specify".into(), resumed["_specify"].clone());
    }
    assert_eq!(
        resumed,
        json!(specify_pmi::feature_table::storable(&accepted))
    );
    let semantic = specify_pmi::part::load_bytes("accepted.step", bytes).unwrap();
    let pmi = specify_pmi::existing::read_existing(&semantic).unwrap();
    assert_eq!(
        pmi.iter().filter(|p| p.kind == "datum").count(),
        expected.datums.len()
    );
    assert!(
        pmi.iter()
            .any(|p| p.kind == "geometric_tolerance" && p.r#type == "Position")
    );
    assert_eq!(
        specify_pmi::existing::part_settings(&semantic).unwrap()["general_tolerance"],
        "ISO 2768-m"
    );

    // general dimensions are rows (Paul, 2026-11-21). A changed class needs fresh
    // approvals for its measurements. The class itself remains an editable answer.
    let mut edited = accepted;
    edited.retain(|id, _| !id.starts_with("length:planes:") && !id.starts_with("location:holes:"));
    edited.insert("part.general_tolerance".into(), json!("ISO 2768-f"));
    assert_eq!(
        accept_defaults(&session, &edited).unwrap()["part.general_tolerance"],
        "ISO 2768-f"
    );
    assert_eq!(
        session.review(&edited).unwrap()["intent"]["part"]["general_tolerance"],
        "ISO 2768-f"
    );
}

#[test]
fn bulk_acceptance_round_trips_the_grm02_shaft() {
    check_bulk_acceptance(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/parts/GRM-02_shaft.step"),
        json!({"part.material": "steel", "part.surface_finish": "Ra 1.6"})
            .as_object()
            .unwrap()
            .clone(),
        Value::Null,
    );
}

#[test]
fn bulk_acceptance_round_trips_grm03() {
    let Some(root) = ["QUIDDITY_CORPUS", "SPECIFY_CORPUS"]
        .iter()
        .filter_map(std::env::var_os)
        .map(std::path::PathBuf::from)
        .find(|p| p.is_dir())
    else {
        assert!(
            std::env::var_os("QUIDDITY_CORPUS_REQUIRED").is_none(),
            "QUIDDITY_CORPUS_REQUIRED: set QUIDDITY_CORPUS or SPECIFY_CORPUS to the corpus directory"
        );
        return;
    };
    // #216 adds planar floor 13 to this hole ID. Keep the same M2 answer and
    // every complete-intent, accepted-default and exact written-byte comparison.
    check_bulk_acceptance(
        &root.join("gramel/GRM-03_thumbwheel_drive_screw.step"),
        json!({"part.material": "steel", "hole.function:holes:12.13": "tapped:M2", "diameter.fit:turned_steps:6": "thread:M3", "diameter.fit:turned_steps:3": "knurl:straight"}).as_object().unwrap().clone(),
        json!(["diameter.fit:turned_steps:2"]),
    );
}

#[test]
fn accepting_defaults_leaves_questions_without_defaults_open_and_does_not_mutate_the_session() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts/plate.step");
    let session = Session::open("plate.step", &std::fs::read(path).unwrap()).unwrap();
    let before = session.review(&Map::new()).unwrap();
    let accepted = accept_defaults(&session, &Map::new()).unwrap();
    assert!(!accepted.contains_key("part.material"));
    let after = session.review(&accepted).unwrap();
    assert_eq!(after["open"], json!(["part.material"]));
    assert_eq!(session.review(&Map::new()).unwrap(), before);
}

#[test]
fn accepting_defaults_preserves_explicit_rejections_and_empty_answers() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/parts/plate.step");
    let session = Session::open("plate.step", &std::fs::read(path).unwrap()).unwrap();
    let given = json!({"datum.A": [], "part.notes": [], "part.material": "steel", "part.general_tolerance": "ISO 2768-f"})
        .as_object().unwrap().clone();
    let accepted = accept_defaults(&session, &given).unwrap();
    for (id, value) in &given {
        assert_eq!(accepted.get(id), Some(value));
    }
    assert_eq!(accept_defaults(&session, &accepted).unwrap(), accepted);
    let review = session.review(&accepted).unwrap();
    assert!(
        review["intent"]["datums"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["letter"] != "A")
    );
    assert_eq!(review["intent"]["part"]["general_tolerance"], "ISO 2768-f");
}

#[test]
fn a_partial_write_is_written_and_says_what_is_open() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/parts");
    let session = Session::open(
        "shaft.step",
        &std::fs::read(root.join("GRM-02_shaft.step")).unwrap(),
    )
    .unwrap();
    let analysis = session.snapshot()["analysis"].clone();
    let answers = json!({"part.material": "steel"})
        .as_object()
        .unwrap()
        .clone();
    // The same ids `write` refuses as incomplete, and the file is written all the same; what is
    // still open is what the file's feature table leaves open (was: the write's `complete` and
    // `missing`; the engine's `edit` reports the file, and the open questions are its table's).
    let bytes = written(&session, &answers);
    let Err(specify_pmi::api::Error::Rules(specify_pmi::rules::Error::Incomplete(e))) =
        specify_pmi::api::apply(&analysis, &answers, false)
    else {
        panic!("the shaft with only a material must be incomplete");
    };
    assert!(!e.missing.is_empty());
    assert!(!bytes.is_empty());
    let open_gaps = |bytes: &[u8]| -> Vec<String> {
        Session::open("w.step", bytes).unwrap().table()["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["state"] == "gap" && r["open"] == json!(true))
            .map(|r| r["id"].as_str().unwrap().to_string())
            .collect()
    };
    // A question with no table row (how holes are located) is open while nothing is stored
    // for it; every other missing id is an open gap of the table.
    let rows = |bytes: &[u8]| -> Vec<String> {
        Session::open("w.step", bytes).unwrap().table()["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_str().unwrap().to_string())
            .collect()
    };
    let open = open_gaps(&bytes);
    let reopened = Session::open("w.step", &bytes).unwrap().snapshot();
    for id in &e.missing {
        if rows(&bytes).contains(id) {
            assert!(open.contains(id), "{id} still open: {open:?}");
        } else {
            assert!(stored(&reopened).get(id).is_none(), "{id} unanswered");
        }
    }

    // Every default taken: nothing open.
    let all = accept_defaults(&session, &answers).unwrap();
    let complete = specify_pmi::api::apply(&analysis, &all, false).is_ok();
    let bytes = written(&session, &all);
    if complete {
        let open = open_gaps(&bytes);
        assert!(e.missing.iter().all(|id| !open.contains(id)), "{open:?}");
    }
    let plate = Session::open(
        "plate.step",
        &std::fs::read(root.join("plate.step")).unwrap(),
    )
    .unwrap();
    let plate_answers = accept_defaults(
        &plate,
        json!({"part.material": "6061-T6"}).as_object().unwrap(),
    )
    .unwrap();
    let plate_analysis = plate.snapshot()["analysis"].clone();
    if let Err(e) = specify_pmi::api::apply(&plate_analysis, &plate_answers, false) {
        panic!("the plate's answers must be complete: {e}");
    }
    let bytes = written(&plate, &plate_answers);
    let reopened = Session::open("plate.step", &bytes).unwrap();
    let reanalysed = reopened.snapshot()["analysis"].clone();
    let resumed = reanalysed["resume"]["answers"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    assert!(
        specify_pmi::api::apply(&reanalysed, &resumed, false).is_ok(),
        "the plate saved with every answer is complete"
    );
}
