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

/// What the editor does with 0.2's engine through `edit`: an added requirement (`extra.*`) is a
/// fill that adds it; `clear` takes a stored decision back (a decided none, how holes are
/// located), never PMI; a 0.2 answers file saved on another revision is refused as stale.
#[test]
fn edit_adds_requirements_clears_decisions_and_refuses_stale_answers() {
    let session = plate();
    let added = session.edit(
        r#"{"fill": {"part.material": "steel", "part.locating": "± dimensions",
                     "extra.1": {"kind": "size", "faces": [9], "value": "7.8/8.2"}},
            "none": ["part.coating"]}"#,
    );
    assert_eq!(added["written"], true, "{added}");
    let written = Session::open("plate.step", &bytes(&added)).unwrap();
    let scene = written.pmi_scene(&Default::default()).unwrap().to_string();
    assert!(scene.contains("7.8"), "the added size is in the file");
    assert_eq!(
        row(&written.table(), "part.coating")["state"],
        "decided_none"
    );
    let cleared = written.edit(r#"{"clear": ["part.coating", "part.locating"]}"#);
    assert_eq!(cleared["written"], true, "{cleared}");
    let back = Session::open("plate.step", &bytes(&cleared)).unwrap();
    assert_eq!(row(&back.table(), "part.coating")["state"], "gap");
    assert_eq!(row(&back.table(), "part.material")["state"], "stated");
    assert!(
        back.snapshot()["analysis"]["resume"]["answers"]
            .get("part.locating")
            .is_none()
    );
    let pmi = back.edit(r#"{"clear": ["part.material"]}"#);
    assert_eq!(pmi["written"], false, "a clear never touches PMI: {pmi}");
    let bad =
        session.edit(r#"{"fill": {"extra.1": {"kind": "size", "faces": [9999], "value": "1"}}}"#);
    assert_eq!(bad["written"], false, "{bad}");
    // A 0.2 answers file with face evidence from another revision.
    let old = Session::open("old.step", &fixture("revisions/plate_split.old.step.gz")).unwrap();
    let new = Session::open("new.step", &fixture("revisions/plate_split.new.step.gz")).unwrap();
    // An answers file saved on OLD with its face evidence (as 0.2 exported one; the export is
    // removed, ADR P11, but such files are still read).
    let mut answers = json!({"part.material": "6061-T6"})
        .as_object()
        .unwrap()
        .clone();
    let loaded =
        specify_core::part::load_bytes("old.step", fixture("revisions/plate_split.old.step.gz"))
            .unwrap();
    specify_core::answer_file::attach(&loaded, &mut answers, &Default::default(), []).unwrap();
    let saved = Value::Object(answers).to_string();
    for result in [new.edit(&saved), new.check_edits(&saved)] {
        assert!(result.to_string().contains("stale answers"), "{result}");
        assert_ne!(result["written"], true);
    }
    assert_eq!(old.edit(&saved)["written"], true);
}

/// A gzipped fixture's STEP text (the engine takes it decompressed).
fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    specify_core::part::load(&path, None)
        .unwrap()
        .bytes
        .to_vec()
}

/// Through `edit`: an added requirement the file states is a byte-identical no-op when filled
/// with the same spec, refused (nothing written) with another value, and replaced when the list
/// also removes it by number.
#[test]
fn edit_takes_an_added_requirements_state_from_the_file() {
    let size = |value: &str| json!({"kind": "size", "faces": [9], "value": value});
    let first = plate().edit(&json!({"fill": {"extra.1": size("7.8/8.2")}}).to_string());
    assert_eq!(first["written"], true, "{first}");
    let written = Session::open("plate.step", &bytes(&first)).unwrap();
    let same = written.edit(&json!({"fill": {"extra.1": size("7.8/8.2")}}).to_string());
    assert_eq!(same["report"]["unchanged"], true, "{same}");
    assert_eq!(bytes(&same), bytes(&first));
    let differing = written.edit(&json!({"fill": {"extra.1": size("7.9/8.1")}}).to_string());
    assert_eq!(differing["written"], false, "{differing}");
    assert!(
        differing["failures"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("#1")
    );
    let replaced = written.edit(
        &json!({"items": {"remove": ["#1"]}, "fill": {"extra.1": size("7.9/8.1")}}).to_string(),
    );
    assert_eq!(replaced["written"], true, "{replaced}");
    let back = Session::open("plate.step", &bytes(&replaced)).unwrap();
    assert!(
        row(&back.table(), "hole.function:holes:9")["value"]
            .as_str()
            .unwrap()
            .contains("⌀7.9 - ⌀8.1")
    );
}

/// A part of an assembly (opened with `open_part`) is edited in place through `edit`, and its
/// BOM written with it (the editor's route: a part's edits, then the BOM): `bom.*` fills and
/// accepted BOM suggestions, every part of the file numbered; the other part keeps its PMI.
#[test]
fn a_part_of_an_assembly_is_edited_with_its_bom() {
    let bytes = include_bytes!("../../../tests/fixtures/parts/assembly.step");
    let before = specify_core::bom::read_bytes(bytes.to_vec()).unwrap();
    assert!(
        before.iter().all(|r| r.item_number.value.is_null()),
        "no BOM yet"
    );
    let pin = Session::open_part("assembly.step", bytes, 1).unwrap();
    let list = json!({
        "fill": {"part.material": "brass", "bom.description": "Locating pin"},
        "accept_suggestions": ["bom.make_buy"],
    });
    assert_eq!(pin.check_edits(&list.to_string())["applicable"], true);
    let out = pin.edit(&list.to_string());
    assert_eq!(out["written"], true, "{out}");
    let written = bytes_of(&out);
    let rows = specify_core::bom::read_bytes(written.clone()).unwrap();
    assert_eq!(rows[1].description.value, "Locating pin");
    assert!(!rows[1].make_buy.value.is_null(), "{:?}", rows[1].make_buy);
    assert!(
        rows.iter().all(|r| r.item_number.value.is_u64()),
        "every part numbered: {:?}",
        rows.iter().map(|r| &r.item_number).collect::<Vec<_>>()
    );
    // The pin states its material; the plate is as it was.
    let reopened = Session::open_part("written.step", &written, 1).unwrap();
    assert_eq!(row(&reopened.table(), "part.material")["state"], "stated");
    let plate = |b: &[u8]| {
        Session::open_part("a.step", b, 0).unwrap().snapshot()["analysis"]["existing"].clone()
    };
    assert_eq!(plate(&written), plate(bytes));
    // A BOM value the BOM does not take, or a file of one part, is refused; nothing written.
    let bad = pin.edit(r#"{"fill": {"bom.optional": "perhaps"}}"#);
    assert_eq!(bad["written"], false, "{bad}");
    let single = plate_session().edit(r#"{"fill": {"bom.description": "Plate"}}"#);
    assert_eq!(single["written"], false, "{single}");
    assert!(single.to_string().contains("bom_no_assembly"), "{single}");
}

fn bytes_of(result: &Value) -> Vec<u8> {
    bytes(result)
}

fn plate_session() -> Session {
    plate()
}
