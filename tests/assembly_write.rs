//! An assembly written a part at a time and together: the write tests of specify-core's
//! `tests/test_assembly.py`, on the oracle's analyses of `parts/assembly` (a plate and a pin
//! placed twice). The parts come from `part::load_all` and the jobs are built over references
//! to them, as stage 4's API composes `write_parts`.

mod common;

use std::path::{Path, PathBuf};

use haecceity::p21::Document;
use haecceity::pmi::{self, DimTolerance};
use haecceity::step::read_part_definitions;
use regex::Regex;
use serde_json::{Map, Value, json};
use specify_core::existing::{part_settings, read_existing};
use specify_core::part::{LoadedPart, load, load_all};
use specify_core::resume::{self, answers_for, stored};
use specify_core::rules::{Intent, apply_with_defaults};
use specify_core::writer::{WriteError, WriteReport, write, write_parts};

const PLATE: usize = 0;
const PIN: usize = 1;

fn answers(k: usize) -> Map<String, Value> {
    let material = if k == PLATE { "steel" } else { "brass" };
    json!({ "part.material": material })
        .as_object()
        .cloned()
        .unwrap()
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-assembly-tests")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn assembly() -> PathBuf {
    common::fixtures().join("parts").join("assembly.step")
}

fn analysis(k: usize) -> Value {
    common::case(&format!("parts/assembly#{k}"))
        .artifact("analysis")
        .expect("a captured analysis")
}

/// The analysis of part `loaded` of a file the writer wrote, as `api.analyse` composes it.
fn reanalysed(k: usize, loaded: &LoadedPart) -> Value {
    let mut out = analysis(k);
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

fn intent(analysis: &Value, k: usize) -> Intent {
    apply_with_defaults(analysis, &answers(k), true).unwrap()
}

/// Every part of `path` written together, each with its intent (from `analyses`) and answers.
fn write_all(path: &Path, analyses: &[Value], out: &Path) -> Result<WriteReport, WriteError> {
    let parts = load_all(path).unwrap();
    let intents: Vec<Intent> = (0..parts.len()).map(|k| intent(&analyses[k], k)).collect();
    let given: Vec<Map<String, Value>> = (0..parts.len()).map(answers).collect();
    let jobs: Vec<_> = parts
        .iter()
        .zip(&intents)
        .zip(&given)
        .map(|((l, i), a)| (l, i, Some(a)))
        .collect();
    write_parts(&jobs, out)
}

/// The `written` fixture: both parts written together with their answers.
fn written(dir: &Path) -> (Vec<Intent>, WriteReport, PathBuf) {
    let out = dir.join("written.step");
    let analyses = [analysis(PLATE), analysis(PIN)];
    let report = write_all(&assembly(), &analyses, &out).unwrap();
    let intents = (0..2).map(|k| intent(&analyses[k], k)).collect();
    (intents, report, out)
}

fn pmi_of(path: &Path, k: usize) -> pmi::PartPmi {
    let bytes = std::fs::read(path).unwrap();
    let defs = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    pmi::read(&doc, &defs).unwrap().parts.swap_remove(k)
}

fn datum_count(path: &Path) -> usize {
    Regex::new(r"= ?DATUM\(")
        .unwrap()
        .find_iter(&std::fs::read_to_string(path).unwrap())
        .count()
}

// test_assembly.py::test_both_parts_are_written_and_verified. The parts' notes are reported,
// not written (U3, Q3).
#[test]
fn both_parts_are_written_and_verified() {
    let dir = temp("both");
    let (_, report, _) = written(&dir);
    assert!(
        report
            .written
            .contains(&"plate: material steel".to_string())
    );
    assert!(
        report
            .written
            .contains(&"pin: internal thread M5x0.8-6H on holes:3.5".to_string())
    );
    assert_eq!(report.not_written.len(), 4);
    assert!(
        report
            .not_written
            .iter()
            .all(|n| n.starts_with("plate: ") || n.starts_with("pin: "))
    );
}

// test_assembly.py::test_each_part_has_its_own_pmi, test_each_part_resumes_its_own_answers,
// test_each_part_is_verified_against_its_own_pmi (re-expressed on haecceity's reading of each
// part: the plate's sizes, the pin none; each part's own material)
#[test]
fn each_part_has_its_own_pmi_and_answers() {
    let dir = temp("own");
    let (intents, _, out) = written(&dir);
    for (k, intent) in intents.iter().enumerate() {
        let back = load(&out, Some(k)).unwrap();
        let mut datums: Vec<(String, Vec<usize>)> = read_existing(&back)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "datum")
            .map(|e| (e.r#type, e.faces))
            .collect();
        let mut wanted: Vec<(String, Vec<usize>)> = intent
            .datums
            .iter()
            .map(|d| {
                (
                    d["letter"].as_str().unwrap().to_string(),
                    serde_json::from_value(d["faces"].clone()).unwrap(),
                )
            })
            .collect();
        datums.sort();
        wanted.sort();
        assert_eq!(datums, wanted, "part {k}");
        assert_eq!(answers_for(&back).unwrap(), Some(answers(k)), "part {k}");
    }
    // Two parts, each with its own datum A, under its own letter in the file.
    let mut letters: Vec<String> = Regex::new(r"DATUM\('','',#\d+,\.F\.,'(\w+)'\)")
        .unwrap()
        .captures_iter(&std::fs::read_to_string(&out).unwrap())
        .map(|c| c[1].to_string())
        .collect();
    letters.sort();
    assert_eq!(letters, ["A", "A", "B", "B", "C"]);
    let (plate, pin) = (pmi_of(&out, PLATE), pmi_of(&out, PIN));
    assert!(
        plate
            .dimensions
            .iter()
            .any(|d| matches!(d.tolerance, DimTolerance::Fit { .. }))
    );
    assert!(pin.dimensions.is_empty());
    assert_eq!(plate.material.unwrap().id, "steel");
    assert_eq!(pin.material.unwrap().id, "brass");
}

// test_assembly.py::test_writing_one_part_again_alone_is_refused
#[test]
fn writing_one_part_again_alone_is_refused() {
    let dir = temp("alone");
    let (_, _, out) = written(&dir);
    let pin = load(&out, Some(PIN)).unwrap();
    let again = intent(&reanalysed(PIN, &pin), PIN);
    let given = answers(PIN);
    match write(&pin, &again, &dir.join("x.step"), Some(&given)) {
        Err(WriteError::Invalid(m)) => assert_eq!(
            m,
            "part(s) 0 have answers stored in this file; write them again with these, or their \
             PMI would be lost"
        ),
        other => panic!("not refused: {other:?}"),
    }
    match write(&pin, &again, &dir.join("x.step"), None) {
        Err(WriteError::Invalid(m)) => {
            assert_eq!(
                m,
                "a part of an assembly is written with the answers it came from"
            )
        }
        other => panic!("not refused: {other:?}"),
    }
}

// test_assembly.py::test_written_again_through_the_service_its_pmi_is_replaced (the writer's
// half; serve is stage 4)
#[test]
fn written_again_its_pmi_is_replaced() {
    let dir = temp("again");
    let (_, _, out) = written(&dir);
    let parts = load_all(&out).unwrap();
    let analyses: Vec<Value> = parts
        .iter()
        .enumerate()
        .map(|(k, l)| reanalysed(k, l))
        .collect();
    let again = dir.join("again.step");
    write_all(&out, &analyses, &again).unwrap();
    assert_eq!(datum_count(&again), 5);
    assert_eq!(datum_count(&out), 5);
    for k in [PLATE, PIN] {
        assert_eq!(
            answers_for(&load(&again, Some(k)).unwrap()).unwrap(),
            Some(answers(k))
        );
    }
}

// test_assembly.py::test_an_assembly_with_pmi_of_its_own_is_refused, re-expressed (question Q6,
// a deliberate difference): Python refuses an assembly whose PMI is not known to be its own,
// as OpenCascade cannot add to it; haecceity's add mode writes it, keeping every original byte
// and storing no answers (its PMI cannot be told from what is added).
#[test]
fn an_assembly_with_pmi_of_its_own_is_added_to() {
    let dir = temp("foreign");
    let (_, _, out) = written(&dir);
    let foreign = dir.join("foreign.step");
    // The same PMI, but no longer known to be specify-core's.
    std::fs::write(
        &foreign,
        std::fs::read_to_string(&out)
            .unwrap()
            .replace("'pmi-assist answers'", "'other'"),
    )
    .unwrap();
    assert!(stored(&std::fs::read(&foreign).unwrap()).is_empty());
    let pin = load(&foreign, Some(PIN)).unwrap();
    let analysis = reanalysed(PIN, &pin);
    let mut given = answers(PIN);
    // What the file states is not asked again; a coating is added.
    given.insert("part.coating".into(), json!("Zinc plate"));
    let intent = apply_with_defaults(&analysis, &given, true).unwrap();
    let x = dir.join("x.step");
    let report = write(&pin, &intent, &x, Some(&given)).unwrap();
    assert!(!report.written.iter().any(|w| w.starts_with("datum ")));
    // A part of an assembly reports no settings (as Python), so its material is asked again;
    // the material it states is kept, not written twice.
    assert!(
        report.warnings.contains(
            &"material brass is already stated in the file; not written again".to_string()
        ),
        "{:?}",
        report.warnings
    );
    let (before, after) = (
        std::fs::read_to_string(&foreign).unwrap(),
        std::fs::read_to_string(&x).unwrap(),
    );
    let end = before.rfind("ENDSEC;").unwrap();
    assert!(after.starts_with(&before[..end]) && after.ends_with(&before[end..]));
    assert!(stored(after.as_bytes()).is_empty());
}

// test_assembly.py::test_answers_stored_for_a_part_the_file_no_longer_has_are_refused
#[test]
fn answers_stored_for_a_part_the_file_no_longer_has_are_refused() {
    let dir = temp("moved");
    let (_, _, out) = written(&dir);
    let text = std::fs::read_to_string(&out).unwrap();
    let pattern = Regex::new(r"'pmi-assist answers','([A-Za-z0-9+/=]+)'").unwrap();
    let changed = pattern.replace_all(&text, |c: &regex::Captures| {
        use base64::Engine;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(&c[1])
            .unwrap();
        let mut p: Value = serde_json::from_slice(&raw).unwrap();
        if p["part"] == json!(PIN) {
            p["part"] = json!(5);
        }
        format!("'pmi-assist answers','{}'", resume::encode(&p))
    });
    let moved = dir.join("changed.step");
    std::fs::write(&moved, changed.as_ref()).unwrap();
    let parts = load_all(&moved).unwrap();
    let analyses: Vec<Value> = parts
        .iter()
        .enumerate()
        .map(|(k, l)| reanalysed(k, l))
        .collect();
    match write_all(&moved, &analyses, &dir.join("x.step")) {
        Err(WriteError::Invalid(m)) => assert_eq!(
            m,
            "the answers stored for part(s) 5 are not for this assembly's parts: it has changed \
             since specify-core wrote it"
        ),
        other => panic!("not refused: {other:?}"),
    }
}

// Two jobs for one part are refused with Python's message; so are parts of two files.
#[test]
fn a_part_is_written_once_and_parts_are_of_one_file() {
    let dir = temp("once");
    let parts = load_all(&assembly()).unwrap();
    let pin = intent(&analysis(PIN), PIN);
    let given = answers(PIN);
    let jobs = [
        (&parts[PIN], &pin, Some(&given)),
        (&parts[PIN], &pin, Some(&given)),
    ];
    match write_parts(&jobs, &dir.join("x.step")) {
        Err(WriteError::Invalid(m)) => assert_eq!(m, "a part is written once"),
        other => panic!("not refused: {other:?}"),
    }
    let plate_file = load(&common::fixtures().join("parts").join("plate.step"), None).unwrap();
    let jobs = [
        (&parts[PIN], &pin, Some(&given)),
        (&plate_file, &pin, Some(&given)),
    ];
    match write_parts(&jobs, &dir.join("x.step")) {
        Err(WriteError::Invalid(m)) => {
            assert_eq!(m, "the parts written together must be of one file")
        }
        other => panic!("not refused: {other:?}"),
    }
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
}
