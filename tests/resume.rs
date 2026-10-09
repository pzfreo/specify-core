//! Answers stored in a written file and resumed from it: specify-core's `tests/test_resume.py`,
//! the cross-engine forms (Python's stored answers read here, this port's read by Python's
//! pattern), the fingerprint's JSON text, and a Python-written file rewritten by replace.

mod common;

use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use specify_core::existing::{part_settings, read_existing};
use specify_core::part::{LoadedPart, load};
use specify_core::pyjson;
use specify_core::resume::{self, answers_for, fingerprint, stored};
use specify_core::rules::{Intent, apply_with_defaults};
use specify_core::writer::write;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-resume-tests")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture(name: &str) -> PathBuf {
    common::fixtures()
        .join("parts")
        .join(format!("{name}.step"))
}

fn analysis(case: &str) -> Value {
    common::case(case)
        .artifact("analysis")
        .expect("a captured analysis")
}

fn apply(analysis: &Value, given: Value) -> Intent {
    apply_with_defaults(analysis, given.as_object().unwrap(), true).expect("an intent")
}

fn feature(analysis: &Value, family: &str, diameter: f64) -> String {
    analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["family"] == family && f["record"]["diameter"].as_f64() == Some(diameter))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn flat(path: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
    Regex::new(r"\s*\n\s*")
        .unwrap()
        .replace_all(&text, " ")
        .into_owned()
}

/// The analysis of a file the writer wrote, as `api.analyse` composes it (see tests/writer.rs).
fn reanalysed(base: &Value, loaded: &LoadedPart) -> Value {
    let mut out = base.clone();
    out["binding"] = serde_json::to_value(&loaded.binding).unwrap();
    match answers_for(loaded).unwrap() {
        Some(a) => {
            out["existing"] = json!([]);
            out["part"] = json!({});
            out["resume"] = json!({ "answers": a });
        }
        None => {
            out["existing"] = serde_json::to_value(read_existing(loaded).unwrap()).unwrap();
            out["part"] = Value::Object(part_settings(loaded).unwrap());
        }
    }
    out
}

// --- test_resume.py --------------------------------------------------------------------------

/// Counts of what a written file holds, as Python's `_counts` (`saved` counts the stored
/// answers' items).
fn counts(path: &Path) -> (usize, usize, usize) {
    let text = flat(path);
    let n = |pattern: &str| Regex::new(pattern).unwrap().find_iter(&text).count();
    (
        n(r"= ?DATUM\("),
        n(r"POSITION_TOLERANCE"),
        n(r"DESCRIPTIVE_REPRESENTATION_ITEM\('pmi-assist answers'"),
    )
}

// test_resume.py::test_a_written_file_resumes_its_answers_and_is_rewritten_not_added_to
#[test]
fn a_written_file_resumes_its_answers_and_is_rewritten_not_added_to() {
    let dir = temp("resume");
    let base = analysis("parts/pin");
    let knurl = feature(&base, "turned_steps", 10.0);
    let given =
        json!({"part.material": "brass", format!("diameter.fit:{knurl}"): "knurl:straight"});
    let first = dir.join("first.step");
    let pin = load(&fixture("pin"), None).unwrap();
    write(
        &pin,
        &apply(&base, given.clone()),
        &first,
        given.as_object(),
    )
    .unwrap();

    // Opened again: the answers, and nothing of the file's own PMI to keep.
    let loaded = load(&first, None).unwrap();
    assert_eq!(answers_for(&loaded).unwrap(), given.as_object().cloned());
    let again = reanalysed(&base, &loaded);
    assert_eq!(again["resume"], json!({ "answers": given }));
    assert_eq!(again["existing"], json!([]));

    // Changed and written again: the PMI is the new answers', once.
    let mut changed = given.clone();
    changed["part.material"] = json!("steel");
    let second = dir.join("second.step");
    let report = write(
        &loaded,
        &apply(&again, changed.clone()),
        &second,
        changed.as_object(),
    )
    .unwrap();
    assert!(report.written.contains(&"material steel".to_string()));
    assert_eq!(counts(&first), counts(&second));
    assert_eq!(counts(&second).2, 1);
    let text = flat(&second);
    assert!(text.contains("'steel'") && !text.contains("'brass'"));
    assert_eq!(
        answers_for(&load(&second, None).unwrap()).unwrap(),
        changed.as_object().cloned()
    );
}

/// A resumed file whose thread is answered away: the replace is the one haecceity's removal
/// plan refuses for a thread (upstream need U14), so the part is cleared first, and what the
/// rewrite adds is numbered from the cleared file's largest id, reusing removed ids. The check
/// that nothing consumed survives must not mistake such a reused id for a survivor.
#[test]
fn a_resumed_files_thread_can_be_answered_away() {
    let dir = temp("unthread");
    let base = analysis("parts/pin");
    let given = json!({"part.material": "brass"});
    let first = dir.join("first.step");
    let pin = load(&fixture("pin"), None).unwrap();
    let report = write(
        &pin,
        &apply(&base, given.clone()),
        &first,
        given.as_object(),
    )
    .unwrap();
    assert!(
        report
            .written
            .iter()
            .any(|w| w.starts_with("internal thread"))
    );

    let loaded = load(&first, None).unwrap();
    let again = reanalysed(&base, &loaded);
    let hole = format!("hole.function:{}", feature(&base, "holes", 4.2));
    let mut changed = given.clone();
    changed[hole] = json!("general");
    let second = dir.join("second.step");
    let report = write(
        &loaded,
        &apply(&again, changed.clone()),
        &second,
        changed.as_object(),
    )
    .unwrap();
    assert!(!report.written.iter().any(|w| w.contains("thread")));
    let text = flat(&second);
    assert!(!text.contains("thread manufacturing"));
    assert_eq!(
        answers_for(&load(&second, None).unwrap()).unwrap(),
        changed.as_object().cloned()
    );
}

// test_resume.py::test_without_answers_nothing_is_stored
#[test]
fn without_answers_nothing_is_stored() {
    let dir = temp("noanswers");
    let analysis = analysis("parts/pin");
    let out = dir.join("plain.step");
    let pin = load(&fixture("pin"), None).unwrap();
    write(
        &pin,
        &apply(&analysis, json!({"part.material": "brass"})),
        &out,
        None,
    )
    .unwrap();
    assert!(stored(&std::fs::read(&out).unwrap()).is_empty());
    assert!(answers_for(&load(&out, None).unwrap()).unwrap().is_none());
}

// test_resume.py::test_answers_for_other_faces_are_not_resumed
#[test]
fn answers_for_other_faces_are_not_resumed() {
    let dir = temp("forged");
    let analysis = analysis("parts/pin");
    let given = json!({"part.material": "brass"});
    let out = dir.join("x.step");
    let pin = load(&fixture("pin"), None).unwrap();
    write(
        &pin,
        &apply(&analysis, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    let mut payload = Value::Object(stored(text.as_bytes()).remove(&0).unwrap());
    payload["faces"] = json!("0".repeat(64));
    let forged = resume::encode(&payload);
    let pattern = Regex::new(r"('pmi-assist answers',')[A-Za-z0-9+/=]*'").unwrap();
    let changed = pattern
        .replace(&text, format!("${{1}}{forged}'").as_str())
        .into_owned();
    assert_ne!(changed, text);
    std::fs::write(&out, changed).unwrap();
    assert!(stored(&std::fs::read(&out).unwrap()).contains_key(&0));
    assert!(answers_for(&load(&out, None).unwrap()).unwrap().is_none());
}

// Python resumes what the Rust writer stores: its pattern matches the stored item.
#[test]
fn the_stored_item_is_the_one_pythons_pattern_reads() {
    let dir = temp("pattern");
    let analysis = analysis("parts/plate");
    let given = json!({"part.material": "steel"});
    let out = dir.join("x.step");
    let plate = load(&fixture("plate"), None).unwrap();
    write(
        &plate,
        &apply(&analysis, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    // resume.py's _SAVED, over the text with line breaks removed.
    let text = Regex::new(r"\s*\n\s*")
        .unwrap()
        .replace_all(&std::fs::read_to_string(&out).unwrap(), "")
        .into_owned();
    let saved = Regex::new(
        r"DESCRIPTIVE_REPRESENTATION_ITEM\(\s*'pmi-assist answers'\s*,\s*'([A-Za-z0-9+/=]*)'\s*\)",
    )
    .unwrap();
    let found: Vec<_> = saved.captures_iter(&text).collect();
    assert_eq!(found.len(), 1);
    let payload = stored(text.as_bytes()).remove(&0).unwrap();
    assert_eq!(found[0][1], resume::encode(&Value::Object(payload)));
}

// --- the fingerprint, across engines ---------------------------------------------------------------

/// The plate's faces as Python's `describe_faces` gives them (kind, centroid rounded to 3
/// decimals: `round(c, 3)` of OpenCascade's centroids, signs of zero included), and the
/// fingerprint Python stored for them in the file it wrote (`parts/plate/written.step.gz`).
const PYTHON_PLATE: &str = r#"[["plane", [-50.0, -0.0, -0.0]], ["plane", [0.0, -30.0, -0.0]], ["plane", [-0.109, -0.0, 5.0]], ["plane", [0.0, 30.0, -0.0]], ["plane", [-0.109, -0.0, -5.0]], ["plane", [50.0, -0.0, -0.0]], ["cylinder", [-30.0, -15.0, -0.0]], ["cylinder", [0.0, -15.0, -0.0]], ["cylinder", [30.0, -15.0, -0.0]], ["cylinder", [0.0, 0.0, -0.0]], ["cylinder", [-30.0, 15.0, -0.0]], ["cylinder", [0.0, 15.0, -0.0]], ["cylinder", [45.0, -0.0, -0.0]], ["cylinder", [30.0, 15.0, -0.0]]]"#;

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// The fingerprint hashes Python's exact `json.dumps` text: given Python's faces, pyjson writes
// the text Python hashed.
#[test]
fn the_fingerprint_text_is_pythons() {
    let faces: Value = serde_json::from_str(PYTHON_PLATE).unwrap();
    assert_eq!(pyjson::dumps(&faces), PYTHON_PLATE);
    let written = common::case("parts/plate").written_step().unwrap();
    let payload = stored(&std::fs::read(&written).unwrap())
        .remove(&0)
        .unwrap();
    assert_eq!(payload["faces"], json!(sha(PYTHON_PLATE)));
}

// Python's stored answers are read in Python's form, in every file Python wrote.
#[test]
fn pythons_stored_answers_are_read() {
    for case in common::cases() {
        if case.part().is_some() {
            continue;
        }
        let (Some(written), Some(Value::Array(back))) =
            (case.written_step(), case.artifact("readback"))
        else {
            continue;
        };
        let found = stored(&std::fs::read(&written).unwrap());
        for (k, python) in back.iter().enumerate() {
            if python["resume"].is_object() {
                assert_eq!(
                    found.get(&k).map(|p| &p["answers"]),
                    Some(&python["resume"]),
                    "{} part {k}",
                    case.name
                );
            }
        }
    }
}

// Why a Python-written file does not resume here (known difference, question in
// docs/questions.md): its payload is read and its face count matches, but its fingerprint is
// of OpenCascade's centroids, whose zero components carry OpenCascade's signs; the Rust
// engine's centroids round to the same numbers with other signs of zero.
#[test]
fn a_python_fingerprint_differs_only_in_signs_of_zero() {
    let loaded = load(&fixture("plate"), None).unwrap();
    let ours = fingerprint(&loaded).unwrap();
    assert_ne!(ours, sha(PYTHON_PLATE));
    let python: Value = serde_json::from_str(PYTHON_PLATE).unwrap();
    let rust: Vec<Value> = specify_core::features::describe_faces(&loaded)
        .unwrap()
        .iter()
        .map(|f| {
            json!([
                f.kind,
                f.centroid
                    .iter()
                    .map(|c| specify_core::pyfmt::round_half_even(*c, 3))
                    .collect::<Vec<_>>()
            ])
        })
        .collect();
    let python = python.as_array().unwrap();
    assert_eq!(python.len(), rust.len());
    for (p, r) in python.iter().zip(&rust) {
        assert_eq!(p[0], r[0]);
        for (a, b) in p[1]
            .as_array()
            .unwrap()
            .iter()
            .zip(r[1].as_array().unwrap())
        {
            assert_eq!(a.as_f64(), b.as_f64(), "{p} {r}"); // -0.0 == 0.0
        }
    }
    let written = common::case("parts/plate").written_step().unwrap();
    let back = load(&written, None).unwrap();
    let payload = stored(&back.bytes).remove(&0).unwrap();
    assert_eq!(payload["face_count"], json!(back.binding.face_count));
    assert!(answers_for(&back).unwrap().is_none());
}

// A file Python wrote whose stored fingerprint is the Rust engine's resumes, and is rewritten
// by replace: Python's text route, presentation and stored answers go, and the answers stored
// are this port's only.
#[test]
fn a_python_written_file_that_resumes_is_rewritten_by_replace() {
    let dir = temp("python-written");
    let written = common::case("parts/plate").written_step().unwrap();
    let text = std::fs::read_to_string(&written).unwrap();
    let ours = fingerprint(&load(&fixture("plate"), None).unwrap()).unwrap();
    let mut payload = Value::Object(stored(text.as_bytes()).remove(&0).unwrap());
    payload["faces"] = json!(ours);
    let pattern = Regex::new(r"('pmi-assist answers',')[A-Za-z0-9+/=]*'").unwrap();
    let patched = dir.join("python.step");
    std::fs::write(
        &patched,
        pattern
            .replace(
                &text,
                format!("${{1}}{}'", resume::encode(&payload)).as_str(),
            )
            .as_ref(),
    )
    .unwrap();
    let loaded = load(&patched, None).unwrap();
    let given = json!({"part.material": "brass"});
    assert_eq!(
        answers_for(&loaded).unwrap(),
        Some(Map::from_iter([(
            "part.material".to_string(),
            json!("steel")
        )]))
    );
    let base = reanalysed(&analysis("parts/plate"), &loaded);
    let out = dir.join("out.step");
    write(
        &loaded,
        &apply(&base, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    let after = flat(&out);
    assert!(!after.contains("'manufacturing requirement'"));
    assert!(!after.contains("PROPERTY_DEFINITION('pmi-assist answers','schema 1'"));
    assert_eq!(
        after
            .matches("DESCRIPTIVE_REPRESENTATION_ITEM('pmi-assist answers'")
            .count(),
        1
    );
    assert!(after.contains("'brass'") && !after.contains("'steel'"));
    assert_eq!(
        answers_for(&load(&out, None).unwrap()).unwrap(),
        given.as_object().cloned()
    );
}
