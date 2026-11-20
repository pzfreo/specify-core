use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use serde_json::{Map, Value, json};
use specify_core::{process, resume};
use specify_core_wasm::session::Session;

fn write(session: &Session, answers: &Value) -> Vec<u8> {
    let written = session
        .write_pmi(answers.as_object().unwrap(), None, None)
        .unwrap();
    serde_json::from_value(written["bytes"].clone()).unwrap()
}

fn open(bytes: &[u8]) -> Session {
    Session::open("plate.step", bytes).unwrap()
}

fn item(name: &str, value: &str) -> String {
    format!("DESCRIPTIVE_REPRESENTATION_ITEM('{name}','{value}')")
}

fn marked_process(replacement: Option<(&str, &str)>) -> Document {
    let plate = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let bytes = write(&open(plate), &json!({process::QUESTION: process::MACHINED}));
    let original = String::from_utf8(bytes).unwrap();
    let before = item(process::ITEM, process::MACHINED);
    assert_eq!(original.matches(&before).count(), 1);
    let text = replacement.map_or_else(
        || original.clone(),
        |(name, value)| original.replace(&before, &item(name, value)),
    );
    Document::parse(text.into_bytes()).unwrap()
}

/// Write only the process, so its removal exercises files with no other marked requirement.
fn edited_process(replacement: Option<(&str, &str)>) -> Vec<u8> {
    let doc = marked_process(replacement);
    let property = doc
        .graph()
        .entities
        .iter()
        .find_map(|(&id, entity)| {
            matches!(entity, RawEntity::Simple {name, attributes, ..}
            if name == "PROPERTY_DEFINITION"
                && attributes.first() == Some(&Attribute::String(process::PROPERTY.into())))
            .then_some(id)
        })
        .unwrap();
    let link = *doc
        .referrers(property)
        .iter()
        .find(|&&id| {
            matches!(doc.get(id), Some(RawEntity::Simple {name, ..})
            if name == "PROPERTY_DEFINITION_REPRESENTATION")
        })
        .unwrap();
    let marker = *doc
        .referrers(link)
        .iter()
        .find(|&&id| {
            matches!(doc.get(id), Some(RawEntity::Simple {name, attributes, ..})
            if name == "DESCRIPTION_ATTRIBUTE"
                && matches!(attributes.first(), Some(Attribute::String(s))
                    if s == specify_core::writer::PMI_PROVENANCE))
        })
        .unwrap();
    let mut edit = Edit::new();
    edit.remove(marker);
    if replacement.is_none() {
        edit.remove(link);
    }
    doc.apply(&edit).unwrap().bytes
}

/// ADR P11: reopening reads the file's process, with no conflict.
fn no_conflict(analysis: &Value) {
    assert!(analysis["resume"].get("conflicts").is_none(), "{analysis}");
}

fn assert_only_process(bytes: &[u8], name: &str, value: &str) {
    let doc = Document::parse(bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(bytes).unwrap();
    let read = haecceity::pmi::read(&doc, &defs).unwrap();
    let values: Vec<_> = read.parts[0]
        .attributes
        .iter()
        .filter(|s| s.name == process::PROPERTY)
        .flat_map(|s| s.items.iter())
        .collect();
    assert_eq!(
        values,
        [&(
            name.into(),
            haecceity::pmi::AttributeValue::Text(value.into())
        )]
    );
}

fn stale_answer_after_the_first_save(name: &str) {
    let edited = edited_process(Some((name, process::CAST)));
    let opened = open(&edited);
    no_conflict(&opened.snapshot()["analysis"]);
    assert_eq!(
        opened.snapshot()["analysis"]["part"]["process"],
        process::CAST
    );
    let stale = json!({process::QUESTION: process::MACHINED});
    let first = write(&opened, &stale);
    let reopened = open(&first);
    assert!(
        reopened.snapshot()["analysis"]["resume"]
            .get("conflicts")
            .is_none()
    );
    let second = write(&reopened, &stale);
    assert_only_process(&second, name, process::CAST);
    assert!(
        resume::stored(&second)[&0]["answers"]
            .get(process::QUESTION)
            .is_none()
    );
    assert_eq!(
        open(&second).snapshot()["analysis"]["part"]["process"],
        process::CAST
    );
}

#[test]
fn a_stale_answer_is_filtered_on_every_save_with_the_canonical_item_name() {
    stale_answer_after_the_first_save(process::ITEM);
}

#[test]
fn a_stale_answer_is_filtered_on_every_save_with_a_normalised_item_name() {
    stale_answer_after_the_first_save("Manufacturing Process");
}

#[test]
fn a_foreign_process_outside_the_options_survives_rewrite() {
    let edited = edited_process(Some((process::ITEM, "laser cut")));
    let opened = open(&edited);
    let rewritten = write(&opened, &json!({}));
    assert_only_process(&rewritten, process::ITEM, "laser cut");
    let stale = write(
        &open(&rewritten),
        &json!({process::QUESTION: process::MACHINED}),
    );
    assert_only_process(&stale, process::ITEM, "laser cut");
}

/// ADR P11: the deletion stands. The removed process is a gap again, with no conflict, and a
/// save does not restore it.
#[test]
fn removing_the_only_marked_requirement_leaves_a_gap() {
    let edited = edited_process(None);
    let opened = open(&edited);
    let snapshot = opened.snapshot();
    let analysis = &snapshot["analysis"];
    no_conflict(analysis);
    assert!(
        analysis["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == process::QUESTION && r["state"] == "gap")
    );
    let resumed = analysis["resume"]["answers"].as_object().unwrap();
    assert_eq!(resumed, &Map::new());
    let rewritten = write(&opened, &Value::Object(resumed.clone()));
    assert!(
        open(&rewritten).snapshot()["analysis"]
            .get("process")
            .is_none()
    );
}

/// The process edit has already lost its marker. Strip Specify's remaining description
/// markers, including the one on the saved answers, without changing the payload.
fn strip_description_markers(bytes: Vec<u8>) -> Vec<u8> {
    let stored = resume::stored(&bytes);
    assert!(
        stored[&0].get("pmi_scope").is_none(),
        "replace-written fixture"
    );
    let doc = Document::parse(bytes).unwrap();
    let mut edit = Edit::new();
    let mut removed = 0;
    for (&id, record) in &doc.graph().entities {
        if matches!(record, RawEntity::Simple { name, attributes, .. }
            if name == "DESCRIPTION_ATTRIBUTE"
                && matches!(attributes.first(), Some(Attribute::String(s))
                    if s == specify_core::writer::PMI_PROVENANCE))
        {
            edit.remove(id);
            removed += 1;
        }
    }
    assert!(removed > 0, "the answers payload had a description marker");
    let stripped = doc.apply(&edit).unwrap().bytes;
    assert_eq!(resume::stored(&stripped), stored);
    stripped
}

#[test]
fn a_replace_written_file_keeps_the_process_when_its_markers_are_stripped() {
    let edited = strip_description_markers(edited_process(Some((process::ITEM, process::CAST))));
    let reopened = open(&edited);
    let rewritten = write(&reopened, &json!({}));
    assert_only_process(&rewritten, process::ITEM, process::CAST);
    no_conflict(&reopened.snapshot()["analysis"]);
    let answered = write(
        &open(&rewritten),
        &json!({process::QUESTION: process::MOULDED}),
    );
    assert_only_process(&answered, process::ITEM, process::CAST);
}

#[test]
fn a_replace_written_file_does_not_restore_a_process_removed_along_with_its_markers() {
    let edited = strip_description_markers(edited_process(None));
    let reopened = open(&edited);
    let snapshot = reopened.snapshot();
    let analysis = &snapshot["analysis"];
    let rewritten = write(&reopened, &analysis["resume"]["answers"]);
    assert!(
        open(&rewritten).snapshot()["analysis"]
            .get("process")
            .is_none()
    );
    no_conflict(analysis);
}

#[test]
fn process_evidence_also_preserves_edited_notes_and_tolerances_after_marker_loss() {
    let plate = include_bytes!("../../../tests/fixtures/parts/plate.step");
    let bytes = write(
        &open(plate),
        &json!({
            process::QUESTION: process::MACHINED,
            "part.notes": ["original note"],
            "part.general_tolerance": "ISO 2768-m",
            "datum.A": [2],
            "datum.A.flatness": "0.03",
        }),
    );
    let text = String::from_utf8(strip_description_markers(bytes)).unwrap();
    let edits = [
        ("original note", "edited note"),
        ("ISO 2768-m", "ISO 2768-f"),
        ("LENGTH_MEASURE(0.03)", "LENGTH_MEASURE(0.058)"),
    ];
    let mut text = text.replace("'specify-core:written:v1'", "''");
    for (old, new) in edits {
        assert!(text.contains(old), "fixture has {old}");
        text = text.replace(old, new);
    }
    let reopened = open(text.as_bytes());
    let analysed = reopened.snapshot();
    let written = write(&reopened, &analysed["analysis"]["resume"]["answers"]);
    let written = String::from_utf8(written).unwrap();
    for (old, new) in edits {
        assert!(written.contains(new), "rewrite keeps {new}");
        assert!(!written.contains(old), "rewrite doesn't restore {old}");
    }
}
