//! Caller attributes share the writer's verified PMI edit (issue #10), including the
//! editor's base64 project, resume, replace/add behaviour and gzipped destinations.

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use haecceity::p21::Document;
use haecceity::pmi::{self, AttributeSet, AttributeValue, PartPmi};
use haecceity::step::read_part_definitions;
use serde_json::{Map, Value, json};
use specify_core::part::{self, LoadedPart};
use specify_core::rules::Intent;
use specify_core::writer::{self, WriteError};
use specify_core::{api, resume};

const PROJECT: &str = "draftwright editor project";

fn fixture(stem: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parts")
        .join(format!("{stem}.step"))
}

fn plate() -> LoadedPart {
    part::load_bytes("plate.step", std::fs::read(fixture("plate")).unwrap()).unwrap()
}

fn answers(material: &str) -> Map<String, Value> {
    json!({"part.material": material, "hole.function:holes:9": "fit:H7"})
        .as_object()
        .unwrap()
        .clone()
}

fn preview_intent(loaded: &LoadedPart, answers: &Map<String, Value>) -> Intent {
    api::preview(&api::analyse(loaded).unwrap(), answers).unwrap()
}

fn project(revision: u32) -> AttributeSet {
    AttributeSet {
        name: PROJECT.into(),
        on: None,
        items: vec![(
            PROJECT.into(),
            AttributeValue::Text(STANDARD.encode(
                serde_json::to_vec(&json!({"revision": revision, "drawing": "front"})).unwrap(),
            )),
        )],
        description: None,
        general_property: None,
    }
}

fn pmi_of(bytes: &[u8]) -> PartPmi {
    let defs = read_part_definitions(bytes).unwrap();
    let doc = Document::parse(bytes.to_vec()).unwrap();
    pmi::read(&doc, &defs).unwrap().parts.remove(0)
}

fn assert_attribute(read: &PartPmi, expected: &AttributeSet) {
    let matching: Vec<_> = read
        .attributes
        .iter()
        .filter(|a| a.name == expected.name)
        .collect();
    assert_eq!(matching.len(), 1, "{read:?}");
    assert_eq!(matching[0].on, expected.on);
    assert_eq!(matching[0].items, expected.items);
    // description and general_property are read-only metadata in haecceity's model.
}

fn temp(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("writer-attributes-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn editor_project_and_other_extras_read_back_and_answers_resume() {
    let loaded = plate();
    let answers = answers("steel");
    let intent = preview_intent(&loaded, &answers);
    let extra = [
        project(1),
        AttributeSet {
            name: "caller revision".into(),
            items: vec![
                ("revision".into(), AttributeValue::Integer(42)),
                (
                    "comment".into(),
                    AttributeValue::Text("O'Brien's part".into()),
                ),
            ],
            ..project(1)
        },
    ];
    let (bytes, report) =
        writer::write_bytes_with(&loaded, &intent, Some(&answers), &extra).unwrap();
    assert!(!report.written.is_empty(), "the PMI was written too");
    let read = pmi_of(&bytes);
    assert!(!read.datums.is_empty());
    for set in &extra {
        assert_attribute(&read, set);
    }
    assert_eq!(
        read.attributes
            .iter()
            .filter(|a| resume::is_answers(a))
            .count(),
        1
    );
    let read = part::load_bytes("written.step", bytes).unwrap();
    assert_eq!(resume::answers_for(&read).unwrap(), Some(answers.clone()));
    assert_eq!(
        api::analyse(&read).unwrap()["resume"]["answers"],
        json!(answers)
    );

    let jobs = [(&loaded, &intent, Some(&answers))];
    assert_eq!(
        writer::write_parts_bytes_with(&jobs, &extra).unwrap(),
        writer::write_bytes_with(&loaded, &intent, Some(&answers), &extra).unwrap()
    );
}

#[test]
fn empty_extras_are_byte_identical_to_the_existing_apis() {
    let loaded = plate();
    let answers = answers("steel");
    let intent = preview_intent(&loaded, &answers);
    for answers in [Some(&answers), None] {
        let expected = writer::write_bytes(&loaded, &intent, answers).unwrap();
        let jobs = [(&loaded, &intent, answers)];
        assert_eq!(
            writer::write_bytes_with(&loaded, &intent, answers, &[]).unwrap(),
            expected
        );
        assert_eq!(writer::write_parts_bytes(&jobs).unwrap(), expected);
        assert_eq!(
            writer::write_parts_bytes_with(&jobs, &[]).unwrap(),
            expected
        );
    }
}

#[test]
fn stored_answers_set_and_item_names_are_reserved_even_without_answers() {
    let loaded = plate();
    let answers = answers("steel");
    let intent = preview_intent(&loaded, &answers);
    let dir = temp("clash");
    let output = dir.join("untouched.step.gz");
    std::fs::write(&output, b"untouched destination").unwrap();
    let reserved_set = AttributeSet {
        name: resume::NAME.into(),
        ..project(1)
    };
    let reserved_item = AttributeSet {
        // A different set name must not smuggle an answers payload into resume.
        items: resume::attribute_set(&resume::payload(&loaded, &answers).unwrap()).items,
        ..project(1)
    };
    for extra in [reserved_set, reserved_item] {
        let extra = [extra];
        for answers in [Some(&answers), None] {
            let jobs = [(&loaded, &intent, answers)];
            let errors = [
                writer::write_bytes_with(&loaded, &intent, answers, &extra).unwrap_err(),
                writer::write_parts_bytes_with(&jobs, &extra).unwrap_err(),
                writer::write_with(&loaded, &intent, &output, answers, &extra).unwrap_err(),
                writer::write_parts_with(&jobs, &output, &extra).unwrap_err(),
            ];
            for err in errors {
                assert!(matches!(err, WriteError::Invalid(_)), "{err}");
                assert!(err.to_string().contains(resume::NAME), "{err}");
                assert!(
                    err.to_string().contains("reserved for stored answers"),
                    "{err}"
                );
            }
        }
    }
    assert_eq!(std::fs::read(&output).unwrap(), b"untouched destination");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rewriting_a_resumed_part_replaces_extras_as_it_replaces_answers() {
    let loaded = plate();
    let first_answers = answers("steel");
    let first_intent = preview_intent(&loaded, &first_answers);
    let (bytes, _) =
        writer::write_bytes_with(&loaded, &first_intent, Some(&first_answers), &[project(1)])
            .unwrap();
    let loaded = part::load_bytes("written.step", bytes).unwrap();
    let answers = answers("brass");
    let intent = preview_intent(&loaded, &answers);
    // A supplied replacement occurs once; omitting it removes the old one. Both take the
    // ordinary replace path, rather than adding another copy of the PMI or stored answers.
    for extra in [vec![project(2)], vec![]] {
        let (bytes, _) =
            writer::write_bytes_with(&loaded, &intent, Some(&answers), &extra).unwrap();
        let read = pmi_of(&bytes);
        assert_eq!(
            read.attributes.iter().filter(|a| a.name == PROJECT).count(),
            extra.len()
        );
        for set in &extra {
            assert_attribute(&read, set);
        }
        assert_eq!(
            read.attributes
                .iter()
                .filter(|a| resume::is_answers(a))
                .count(),
            1
        );
        let loaded = part::load_bytes("rewritten.step", bytes).unwrap();
        assert_eq!(resume::answers_for(&loaded).unwrap(), Some(answers.clone()));
    }
}

#[test]
fn adding_to_foreign_pmi_keeps_existing_attributes_and_adds_extras() {
    let loaded = plate();
    let answers = answers("steel");
    let intent = preview_intent(&loaded, &answers);
    let existing = AttributeSet {
        name: "foreign project".into(),
        ..project(1)
    };
    // No stored answers: subsequent writes treat this PMI as foreign and use add mode.
    let (bytes, _) =
        writer::write_bytes_with(&loaded, &intent, None, std::slice::from_ref(&existing)).unwrap();
    let loaded = part::load_bytes("foreign.step", bytes).unwrap();
    let intent = preview_intent(&loaded, &answers);
    let (bytes, _) =
        writer::write_bytes_with(&loaded, &intent, Some(&answers), &[project(2)]).unwrap();
    let read = pmi_of(&bytes);
    assert_attribute(&read, &existing);
    assert_attribute(&read, &project(2));
    assert!(
        resume::stored(&bytes).is_empty(),
        "add does not store answers"
    );
}

#[test]
fn extras_work_with_atomic_plain_and_gzip_destinations() {
    let loaded = plate();
    let answers = answers("steel");
    let intent = preview_intent(&loaded, &answers);
    let extra = [project(1)];
    let expected = writer::write_bytes_with(&loaded, &intent, Some(&answers), &extra).unwrap();
    let jobs = [(&loaded, &intent, Some(&answers))];
    let dir = temp("gzip");
    for name in ["out.step", "out.step.gz", "out.STEP.GZ"] {
        let output = dir.join(name);
        let report = writer::write_with(&loaded, &intent, &output, Some(&answers), &extra).unwrap();
        assert_eq!(report.output, output);
        assert_eq!(report.written, expected.1.written);
        let bytes = std::fs::read(&output).unwrap();
        if name != "out.step" {
            assert!(bytes.starts_with(&[0x1f, 0x8b]));
        }
        let read = part::load(&output, None).unwrap();
        assert_eq!(&*read.bytes, &expected.0);
        assert_attribute(&pmi_of(&read.bytes), &extra[0]);
        assert_eq!(resume::answers_for(&read).unwrap(), Some(answers.clone()));
        // The parts variant uses the same staging and compression path.
        writer::write_parts_with(&jobs, &output, &extra).unwrap();
        assert_eq!(std::fs::read(&output).unwrap(), bytes);
    }
    // Empty extras also keep the file API's deterministic gzip bytes.
    let output = dir.join("empty.step.gz");
    writer::write(&loaded, &intent, &output, Some(&answers)).unwrap();
    let expected = std::fs::read(&output).unwrap();
    writer::write_with(&loaded, &intent, &output, Some(&answers), &[]).unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), expected);
    writer::write_parts(&jobs, &output).unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), expected);
    writer::write_parts_with(&jobs, &output, &[]).unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), expected);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn assembly_extras_are_refused_even_when_only_one_part_is_written() {
    let loaded = part::load(&fixture("assembly"), Some(0)).unwrap();
    let answers = answers("steel");
    let intent = preview_intent(&loaded, &answers);
    let extra = [project(1)];
    let jobs = [(&loaded, &intent, Some(&answers))];
    let dir = temp("assembly");
    let output = dir.join("refused.step.gz");
    for err in [
        writer::write_bytes_with(&loaded, &intent, Some(&answers), &extra).unwrap_err(),
        writer::write_parts_bytes_with(&jobs, &extra).unwrap_err(),
        writer::write_with(&loaded, &intent, &output, Some(&answers), &extra).unwrap_err(),
        writer::write_parts_with(&jobs, &output, &extra).unwrap_err(),
    ] {
        assert!(matches!(err, WriteError::Invalid(_)), "{err}");
        assert!(err.to_string().contains("file of one part"), "{err}");
    }
    assert!(!output.exists());
    assert_eq!(
        writer::write_parts_bytes_with(&jobs, &[]).unwrap(),
        writer::write_parts_bytes(&jobs).unwrap()
    );
    assert!(matches!(
        writer::write_parts_bytes_with(&[], &extra),
        Err(WriteError::Invalid(_))
    ));
    std::fs::remove_dir_all(dir).unwrap();
}
