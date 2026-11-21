//! The engine's edit-list API (Specify 0.3.0, piece 6): `edit` applies a list in memory, all
//! or nothing; `checkEdits` checks one against the file; `table` is the feature table.
use serde_json::{Value, json};
use specify_core_wasm::session::Session;

fn plate() -> Session {
    let bytes = include_bytes!("../../../tests/fixtures/parts/plate.step");
    Session::open("plate.step", bytes).unwrap()
}

fn bytes(result: &Value) -> Vec<u8> {
    serde_json::from_value(result["bytes"].clone()).unwrap()
}

fn row<'a>(table: &'a Value, id: &str) -> &'a Value {
    table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no row {id}"))
}

#[test]
fn table_is_the_feature_table_with_the_ids_the_questions_use() {
    let session = plate();
    let table = session.table();
    assert!(table["rows"].as_array().is_some_and(|r| !r.is_empty()));
    assert!(table["file_pmi"].is_array());
    assert!(table["decided_none"].is_array());
    assert_eq!(table, session.snapshot()["analysis"]["table"]);
    assert_eq!(row(&table, "part.material")["state"], "gap");
    let questions = session.review(&Default::default()).unwrap()["questions"].clone();
    let asked: Vec<&str> = questions
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|q| q["id"].as_str())
        .collect();
    for id in ["part.material", "datum.A", "datum.A.flatness"] {
        assert!(asked.contains(&id), "{id} is asked");
        row(&table, id);
    }
}

#[test]
fn edit_applies_a_list_and_the_bytes_reopen_with_it_stated() {
    let session = plate();
    let list = json!({
        "fill": {"part.material": "6082-T6", "datum.A.flatness": "0.05"},
        "accept_suggestions": ["datum.A"],
    });
    let out = session.edit(&list.to_string());
    assert_eq!(out["written"], true, "{out}");
    assert_eq!(out["report"]["unchanged"], false);
    assert_eq!(
        out["source_sha256"],
        session.snapshot()["provenance"]["source_sha256"]
    );
    let reopened = Session::open("plate.step", &bytes(&out)).unwrap();
    let table = reopened.table();
    for id in ["part.material", "datum.A", "datum.A.flatness"] {
        assert_eq!(row(&table, id)["state"], "stated", "{id}");
    }
    // The engine is not changed by an edit: it still reads the file it was opened with.
    assert_eq!(row(&session.table(), "part.material")["state"], "gap");
}

#[test]
fn edit_with_a_bad_entry_writes_nothing_and_names_every_bad_entry() {
    let session = plate();
    let list = json!({
        "fill": {"part.material": "6082-T6", "hole.position:holes:9999": "0.2"},
        "items": {"remove": ["#999"]},
    });
    let out = session.edit(&list.to_string());
    assert_eq!(out["written"], false, "{out}");
    assert!(out.get("bytes").is_none());
    let entries: Vec<&str> = out["failures"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["entry"].as_str())
        .collect();
    assert!(
        entries.contains(&r#"fill["hole.position:holes:9999"]"#),
        "{entries:?}"
    );
    assert!(
        entries.contains(&r##"items.remove["#999"]"##),
        "{entries:?}"
    );
    assert!(!entries.iter().any(|e| e.contains("part.material")));
    let message = out["message"].as_str().unwrap();
    assert!(message.starts_with("Nothing was written"), "{message}");
    assert!(message.contains("hole.position:holes:9999") && message.contains("#999"));
    // Not JSON at all: refused the same way, not thrown.
    let out = session.edit("{");
    assert_eq!(out["written"], false);
    assert!(!out["failures"].as_array().unwrap().is_empty());
}

#[test]
fn an_empty_edit_gives_the_file_back_unchanged() {
    let original = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let out = plate().edit("{}");
    assert_eq!(out["written"], true);
    assert_eq!(out["report"]["unchanged"], true);
    assert_eq!(bytes(&out), original.to_vec());
}

#[test]
fn check_edits_checks_against_the_file_and_writes_nothing() {
    let session = plate();
    let good = json!({"fill": {"part.material": "6082-T6"}}).to_string();
    let checked = session.check_edits(&good);
    assert_eq!(checked["checked_against_file"], true, "{checked}");
    assert_eq!(checked["applicable"], true);
    assert_eq!(checked["written"], false);
    let bad = json!({"change": {"part.material": {"to": "steel", "expect": "brass"}}}).to_string();
    let checked = session.check_edits(&bad);
    assert_eq!(checked["applicable"], false);
    assert_eq!(
        checked["failures"][0]["entry"],
        r#"change["part.material"]"#
    );
    // What check passes, edit writes; what it refuses, edit refuses with the same reasons.
    assert_eq!(session.edit(&good)["written"], true);
    assert_eq!(session.edit(&bad)["failures"], checked["failures"]);
}

/// The editor keeps its project in the STEP (0.2's `writePmi(answers, project)`): an edit can
/// carry it, with the original part, and a later edit carrying it again replaces it rather than
/// adding a second; a plain edit keeps it as it is. Saved answers are still matched to the part
/// (`rematchAnswers`).
#[test]
fn an_edit_carries_the_editors_project_and_replaces_it() {
    const PROJECT: &str = "draftwright editor project";
    let session = plate();
    let original = include_bytes!("../../../tests/fixtures/parts/plate.step").to_vec();
    let first = session
        .edit_carrying(
            r#"{"fill": {"part.material": "steel"}}"#,
            Some("eyJ2IjoxfQ=="),
            None,
        )
        .unwrap();
    assert_eq!(first["written"], true, "{first}");
    let written = Session::open("plate.step", &bytes(&first)).unwrap();
    assert_eq!(written.payload(PROJECT), Some(json!({"v": 1})));
    assert_eq!(written.original(), Some(original.clone()));
    let again = written
        .edit_carrying("{}", Some("eyJ2IjoyfQ=="), None)
        .unwrap();
    let text = String::from_utf8(bytes(&again)).unwrap();
    assert_eq!(
        text.matches(PROJECT).count(),
        String::from_utf8(bytes(&first))
            .unwrap()
            .matches(PROJECT)
            .count()
    );
    let replaced = Session::open("plate.step", &bytes(&again)).unwrap();
    assert_eq!(replaced.payload(PROJECT), Some(json!({"v": 2})));
    assert_eq!(replaced.original(), Some(original));
    // A plain edit leaves the project as it is.
    let plain = replaced.edit(r#"{"none": ["part.coating"]}"#);
    assert_eq!(plain["written"], true, "{plain}");
    let kept = Session::open("plate.step", &bytes(&plain)).unwrap();
    assert_eq!(kept.payload(PROJECT), Some(json!({"v": 2})));
    assert!(
        session
            .edit_carrying("{}", Some("not base64!"), None)
            .is_err()
    );
    // Saved answers matched to the part by its evidence.
    let answers = json!({"part.material": "steel"})
        .as_object()
        .unwrap()
        .clone();
    let evidence = session.evidence().unwrap();
    let matched = written
        .rematch_answers(&answers, Some(&evidence), None)
        .unwrap();
    assert_eq!(matched["answers"], json!(answers));
}
