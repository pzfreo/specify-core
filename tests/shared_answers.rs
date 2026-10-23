//! Answers held outside the STEP (#35), the application payload the writer carries, the
//! original part, and schema 1 files saved by the pre-alpha browser writer.
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use regex::bytes::{NoExpand, Regex};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use specify_core::{api, carried, features::Feature, part, resume, writer};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn features(analysis: &Value) -> Vec<Feature> {
    serde_json::from_value(analysis["features"].clone()).unwrap()
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

// --- answers held outside the STEP ------------------------------------------------------------

/// FTC-10 as alpha.6 wrote it: its 29 answers (two on a linear pattern whose support has grown)
/// and the face evidence stored with them.
fn ftc10() -> (part::LoadedPart, Value, Map<String, Value>, Value) {
    let base: Value =
        serde_json::from_slice(&std::fs::read(fixture("resume-alpha6/ftc10-reid.json")).unwrap())
            .unwrap();
    let loaded = part::load(&fixture("resume-alpha6/ftc10-reid.step.gz"), Some(0)).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let stored = resume::stored(&loaded.bytes).remove(&0).unwrap();
    let evidence = json!({"schema": stored["schema"], "face_count": stored["face_count"], "faces": stored["faces"]});
    (loaded, analysis, object(base["answers"].clone()), evidence)
}

#[test]
fn answers_with_their_evidence_migrate_as_answers_stored_in_the_step_do() {
    let (loaded, analysis, answers, evidence) = ftc10();
    // What a caller keeps now is what alpha.6 stored: the schema 2 fingerprint is the same.
    assert_eq!(resume::evidence(&loaded).unwrap(), evidence);
    let out = resume::rematch_answers(
        &loaded,
        &features(&analysis),
        &answers,
        Some(&evidence),
        None,
    )
    .unwrap();
    let value = serde_json::to_value(&out).unwrap();
    // Exactly the report the file's own stored answers give on opening.
    assert_eq!(value, analysis["resume"]);
    assert_eq!(out.migrated.len(), 2);
    assert!(out.unresolved.is_empty() && out.unverified.is_none());
    let review = api::review(&analysis, &out.answers).unwrap();
    assert_eq!(review["open"].as_array().unwrap().len(), 2);
}

#[test]
fn without_evidence_unknown_feature_ids_are_unresolved_with_their_values() {
    let (loaded, analysis, answers, _) = ftc10();
    let out = resume::rematch_answers(&loaded, &features(&analysis), &answers, None, None).unwrap();
    assert!(out.migrated.is_empty());
    assert!(
        out.unverified
            .as_deref()
            .unwrap()
            .contains("no saved face fingerprint")
    );
    assert_eq!(out.unresolved.len(), 2);
    for unresolved in &out.unresolved {
        assert!(unresolved.id.contains("hole_patterns.linear:138.140"));
        assert_eq!(unresolved.answer, answers[&unresolved.id]);
        assert!(!unresolved.automatic);
    }
    // Every other answer is kept as it was, in order; nothing is dropped.
    assert_eq!(out.answers.len() + out.unresolved.len(), answers.len());
    let kept: Vec<_> = answers
        .keys()
        .filter(|id| !out.unresolved.iter().any(|u| &u.id == *id))
        .collect();
    assert_eq!(out.answers.keys().collect::<Vec<_>>(), kept);
    // The kept answers are valid for this part: a review takes them.
    api::review(&analysis, &out.answers).unwrap();

    // Evidence for other faces is no evidence: the same, saying why.
    let other =
        json!({"schema": 2, "face_count": loaded.binding.face_count, "faces": "0".repeat(64)});
    let mismatched =
        resume::rematch_answers(&loaded, &features(&analysis), &answers, Some(&other), None)
            .unwrap();
    assert!(
        mismatched
            .unverified
            .as_deref()
            .unwrap()
            .contains("does not match")
    );
    assert_eq!(mismatched.answers, out.answers);
    assert_eq!(mismatched.unresolved.len(), 2);
}

#[test]
fn automatic_answers_are_marked_and_their_ids_follow_migration() {
    let (loaded, analysis, answers, evidence) = ftc10();
    let moved: Vec<String> = answers
        .keys()
        .filter(|id| id.contains("hole_patterns.linear:138.140"))
        .cloned()
        .collect();
    let material = "part.material".to_string();
    let automatic = [moved.clone(), vec![material.clone(), "datum.Z".into()]].concat();
    let migrated = resume::rematch_answers(
        &loaded,
        &features(&analysis),
        &answers,
        Some(&evidence),
        Some(&automatic),
    )
    .unwrap();
    let renamed: Vec<String> = moved
        .iter()
        .map(|old| {
            migrated
                .migrated
                .iter()
                .find(|(o, _)| o == old)
                .unwrap()
                .1
                .clone()
        })
        .collect();
    let mut expected = renamed;
    if answers.contains_key(&material) {
        expected.push(material.clone());
    }
    assert_eq!(migrated.automatic.as_ref().unwrap(), &expected);

    let unmatched = resume::rematch_answers(
        &loaded,
        &features(&analysis),
        &answers,
        None,
        Some(&automatic),
    )
    .unwrap();
    assert!(unmatched.unresolved.iter().all(|u| u.automatic));
    let value = serde_json::to_value(&unmatched).unwrap();
    assert_eq!(value["unresolved"][0]["automatic"], json!(true));
    assert!(
        unmatched
            .automatic
            .unwrap()
            .iter()
            .all(|id| !moved.contains(id))
    );
}

#[test]
fn without_evidence_a_datum_on_a_face_the_part_lacks_is_unresolved() {
    let loaded = part::load(&fixture("parts/plate.step"), None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let answers = object(json!({"datum.A": [0], "datum.B": [999], "part.material": "steel"}));
    let out = resume::rematch_answers(&loaded, &features(&analysis), &answers, None, None).unwrap();
    assert_eq!(
        Value::Object(out.answers),
        json!({"datum.A": [0], "part.material": "steel"})
    );
    assert_eq!(out.unresolved[0].id, "datum.B");
    assert_eq!(out.unresolved[0].answer, json!([999]));
}

#[test]
fn malformed_evidence_is_refused() {
    let loaded = part::load(&fixture("parts/plate.step"), None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    for bad in [
        json!([]),
        json!({"schema": 3, "face_count": 1, "faces": ""}),
        json!({"schema": 2, "faces": ""}),
    ] {
        let error =
            resume::rematch_answers(&loaded, &features(&analysis), &Map::new(), Some(&bad), None)
                .unwrap_err();
        assert!(error.to_string().starts_with("Invalid evidence"), "{error}");
    }
}

// --- schema 1 files of the pre-alpha browser writer ---------------------------------------------

// tests/fixtures/legacy-schema1/README.md: written by Specify 42ff22c0 under the kernel of the
// time, whose signs of zero differ from quiddity v0.1.1's, so its schema 1 hash cannot be
// reproduced. Its answers resume by unchanged feature ID, with a warning, and rewriting stores
// them as schema 2.
#[test]
fn a_pre_alpha_browser_export_resumes_its_answers() {
    let path = fixture("legacy-schema1/legacy_specify.step");
    let loaded = part::load(&path, None).unwrap();
    let stored = resume::stored(&loaded.bytes).remove(&0).unwrap();
    assert_eq!(stored["schema"], json!(1));
    assert_ne!(
        stored["faces"],
        json!(resume::fingerprint_v1(&loaded).unwrap())
    );
    let expected = json!({"part.material": "6082-T6", "hole.function:holes:9": "fit:H7"});
    assert_eq!(
        resume::answers_for(&loaded).unwrap().map(Value::Object),
        Some(expected.clone())
    );
    let (analysis, warnings) = api::analyse_reporting(&loaded).unwrap();
    assert_eq!(analysis["resume"]["answers"], expected);
    assert!(analysis["resume"]["unresolved"].is_null());
    assert!(
        analysis["resume"]["unverified"]
            .as_str()
            .unwrap()
            .contains("schema 1")
    );
    assert_eq!(analysis["existing"], json!([]));
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let review = api::review(
        &analysis,
        analysis["resume"]["answers"].as_object().unwrap(),
    )
    .unwrap();
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["code"] == "saved_answers_unverified")
    );

    // Rewriting replaces its PMI (it is the writer's own) and stores schema 2, which verifies.
    let answers = object(expected.clone());
    let intent = api::preview(&analysis, &answers).unwrap();
    let (bytes, _) = writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
    let back = part::load_bytes("again.step", bytes).unwrap();
    assert_eq!(resume::stored(&back.bytes)[&0]["schema"], json!(2));
    let again = api::analyse(&back).unwrap();
    assert_eq!(again["resume"]["answers"], expected);
    assert!(again["resume"]["unverified"].is_null());
}

// --- the application payload and the original part ----------------------------------------------

const PROJECT: &str = "draftwright editor project";

/// The editor's `readProject` / draftwright-mcp's `project::read`: the last item of the project
/// name, base64 of JSON.
fn editor_read(step: &[u8]) -> Option<Value> {
    let found = Regex::new(&format!(
        r"DESCRIPTIVE_REPRESENTATION_ITEM\s*\(\s*'{PROJECT}'\s*,\s*'([A-Za-z0-9+/=]+)'\s*\)"
    ))
    .unwrap();
    let encoded = found
        .captures_iter(step)
        .last()?
        .get(1)?
        .as_bytes()
        .to_vec();
    serde_json::from_slice(&STANDARD.decode(encoded).ok()?).ok()
}

fn schema_statement() -> Regex {
    Regex::new(r"FILE_SCHEMA\s*\([\s\S]*?\)\s*;").unwrap()
}

/// draftwright-mcp's `project::next_instance`, the editor's `nextInstance`.
fn next_instance(step: &[u8]) -> u64 {
    let names = Regex::new(r"'(?:[^']|'')*'|/\*[\s\S]*?\*/|#(\d+)\s*=").unwrap();
    names
        .captures_iter(step)
        .filter_map(|c| c.get(1))
        .filter_map(|m| std::str::from_utf8(m.as_bytes()).ok()?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

/// draftwright-mcp's `project::original`, the editor's `originalOf`.
fn editor_original(step: &[u8], project: &Value) -> Option<Vec<u8>> {
    let from = project["generatedFrom"].as_u64()?;
    let start = Regex::new(&format!(r"(?m)^#{from}\s*="))
        .unwrap()
        .find(step)?
        .start();
    let end = Regex::new(r"ENDSEC\s*;")
        .unwrap()
        .find_iter(step)
        .last()?
        .start();
    let joined = [&step[..start], &step[end..]].concat();
    let header = project["source"]["header"].as_str()?.as_bytes();
    Some(
        schema_statement()
            .replace(&joined, NoExpand(header))
            .into_owned(),
    )
}

/// The editor's stored project for `source`, as `storedProject` makes it.
fn editor_project(source: &[u8]) -> Value {
    let header =
        String::from_utf8_lossy(schema_statement().find(source).unwrap().as_bytes()).into_owned();
    json!({
        "schema": 1, "name": "Plate ‘α’",
        "source": {"filename": "plate.step", "sha256": sha256(source), "header": header},
        "generatedFrom": next_instance(source),
        "edits": {"spec": {"schema": "draftwright.spec/2", "sheet": {}}, "answers": {"part.material": "steel"}, "auto": []},
    })
}

fn write(
    loaded: &part::LoadedPart,
    answers: &Value,
    extra: &[haecceity::pmi::AttributeSet],
) -> Vec<u8> {
    let analysis = api::analyse(loaded).unwrap();
    let answers = object(answers.clone());
    let intent = api::preview(&analysis, &answers).unwrap();
    writer::write_bytes_with(loaded, &intent, Some(&answers), extra)
        .unwrap()
        .0
}

#[test]
fn the_carried_project_is_read_by_the_editors_reader_and_gives_back_the_original() {
    let source = std::fs::read(fixture("parts/plate.step")).unwrap();
    let loaded = part::load_bytes("plate.step", source.clone()).unwrap();
    let project = editor_project(&source);
    let extra = carried::attributes(&source, PROJECT, &project).unwrap();
    let written = write(&loaded, &json!({"part.material": "steel"}), &extra);

    // The editor and draftwright-mcp read what is written today, and recover the part.
    assert_eq!(editor_read(&written), Some(project.clone()));
    assert_eq!(
        sha256(&editor_original(&written, &project).unwrap()),
        sha256(&source)
    );
    // And so does the engine.
    assert_eq!(carried::payload(&written, PROJECT), Some(project.clone()));
    assert_eq!(carried::original(&written), Some(source.clone()));
    let back = part::load_bytes("plate.step", written.clone()).unwrap();
    assert_eq!(
        resume::answers_for(&back).unwrap().map(Value::Object),
        Some(json!({"part.material": "steel"}))
    );

    // A file the editor wrote (its base64, no original recorded) reads the same.
    let encoded = STANDARD.encode(project.to_string());
    let editor_only = vec![haecceity::pmi::AttributeSet {
        name: PROJECT.into(),
        on: None,
        items: vec![(
            PROJECT.into(),
            haecceity::pmi::AttributeValue::Text(encoded),
        )],
        description: None,
        general_property: None,
    }];
    let old = write(&loaded, &json!({"part.material": "steel"}), &editor_only);
    assert_eq!(carried::payload(&old, PROJECT), Some(project));
    assert_eq!(carried::original(&old), None);
}

#[test]
fn rewriting_a_written_file_keeps_its_first_original() {
    let source = std::fs::read(fixture("parts/plate.step")).unwrap();
    let loaded = part::load_bytes("plate.step", source.clone()).unwrap();
    let value = json!({"revision": 1});
    let extra = carried::attributes(&source, "an app", &value).unwrap();
    let first = write(&loaded, &json!({"part.material": "steel"}), &extra);
    let reopened = part::load_bytes("plate.step", first.clone()).unwrap();
    let value = json!({"revision": 2, "text": "it's \\ ‘quoted’"});
    let extra = carried::attributes(&first, "an app", &value).unwrap();
    let second = write(&reopened, &json!({"part.material": "brass"}), &extra);
    assert_ne!(first, second);
    assert_eq!(carried::payload(&second, "an app"), Some(value));
    assert_eq!(carried::original(&second), Some(source));
    // A write without a payload records no original.
    assert_eq!(
        carried::original(&write(&loaded, &json!({"part.material": "steel"}), &[])),
        None
    );
}

#[test]
fn a_payload_must_have_a_name_of_its_own() {
    let source = std::fs::read(fixture("parts/plate.step")).unwrap();
    for name in ["", resume::NAME, carried::ORIGINAL] {
        assert!(
            carried::attributes(&source, name, &json!({})).is_err(),
            "{name:?}"
        );
    }
    assert!(carried::attributes_encoded(&source, PROJECT, "not base64!").is_err());
}
