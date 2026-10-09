//! A smoke test of the whole chain on the Rust engine's own analyses, before the command line
//! composes it (stage 4): for every synthetic part (each part of the assemblies), the analysis
//! is built by the library in Python's layout (`describe_faces`, `recognise`, mates and
//! contacts for a part of an assembly, `read_existing`, `part_settings`), then the rules ask
//! their questions, apply the defaults, and the writer writes the intent. Nothing may panic,
//! no rule may refuse the analysis as invalid (which would mean a record the rules read is
//! shaped differently by quiddity-rust), and every write must verify.
//!
//! Feature ids differ between engines by design (defining against constituent faces, upstream
//! need U2), so these analyses are not compared with Python's entry by entry; that the chain
//! runs on them is what is checked. The pin's tapped hole is such a hole (its faces lack the
//! floor), and the plate's general dimensions run over the plane levels the Rust analysis
//! gives.

mod common;

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use specify_core::existing::{part_settings, read_existing};
use specify_core::features::{describe_faces, recognise};
use specify_core::mates::{contacts, mates};
use specify_core::part::{LoadedPart, load_all};
use specify_core::rules::{Error, Intent, apply_with_defaults, questions};
use specify_core::writer::write_parts;

/// specify-core's analysis of `loaded` (Python's `api.analyse`), from the Rust engine.
fn analyse(loaded: &LoadedPart, all: &[LoadedPart]) -> Value {
    let mut faces: Vec<Value> = describe_faces(loaded)
        .unwrap()
        .iter()
        .map(|f| serde_json::to_value(f).unwrap())
        .collect();
    let features = recognise(loaded).unwrap();
    let mut analysis = Map::new();
    analysis.insert("schema".into(), json!(1));
    analysis.insert(
        "binding".into(),
        serde_json::to_value(&loaded.binding).unwrap(),
    );
    if loaded.part_count > 1 {
        let k = loaded.binding.part;
        for (index, contact) in contacts(all, k) {
            faces[index.parse::<usize>().unwrap()]["contact"] = contact;
        }
        analysis.insert("faces".into(), json!(faces));
        analysis.insert("features".into(), serde_json::to_value(features).unwrap());
        analysis.insert("mates".into(), Value::Object(mates(all, k)));
    } else {
        analysis.insert("faces".into(), json!(faces));
        analysis.insert("features".into(), serde_json::to_value(features).unwrap());
    }
    analysis.insert(
        "existing".into(),
        serde_json::to_value(read_existing(loaded).unwrap()).unwrap(),
    );
    analysis.insert("part".into(), Value::Object(part_settings(loaded).unwrap()));
    Value::Object(analysis)
}

fn synthetic_parts() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(common::fixtures().join("parts"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "step"))
        .collect();
    out.sort();
    out
}

fn temp() -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-rust-analyses")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Questions, defaults and intent for one part's analysis; a refusal is reported by the
/// caller, naming the part.
fn intent(analysis: &Value) -> Result<Intent, Error> {
    questions(analysis, &Map::new())?;
    let given = json!({"part.material": "steel"});
    apply_with_defaults(analysis, given.as_object().unwrap(), true)
}

fn write_file(path: &Path, dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    let parts = load_all(path).unwrap();
    let mut intents = Vec::new();
    for loaded in &parts {
        let analysis = analyse(loaded, &parts);
        match intent(&analysis) {
            Ok(i) => intents.push(i),
            Err(e) => problems.push(format!("{name} part {}: {e}", loaded.binding.part)),
        }
    }
    if !problems.is_empty() {
        return problems;
    }
    let answers = json!({"part.material": "steel"});
    let jobs: Vec<_> = parts
        .iter()
        .zip(&intents)
        .map(|(l, i)| (l, i, answers.as_object()))
        .collect();
    match write_parts(&jobs, &dir.join(format!("{name}.step"))) {
        Ok(report) => {
            assert!(!report.written.is_empty(), "{name}: nothing written");
            println!(
                "{name}: {} written, {} not written",
                report.written.len(),
                report.not_written.len()
            );
        }
        Err(e) => problems.push(format!("{name}: {e}")),
    }
    problems
}

#[test]
fn the_rules_and_the_writer_run_on_rust_analyses() {
    let dir = temp();
    let mut problems = Vec::new();
    for path in synthetic_parts() {
        problems.extend(write_file(&path, &dir));
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The pin's tapped hole as the Rust engine recognises it: its defining faces only, the bore
/// without the floor (U2), and still a tapped hole with its thread written.
#[test]
fn a_hole_without_its_floor_is_tapped_and_written() {
    let dir = temp().join("pin");
    std::fs::create_dir_all(&dir).unwrap();
    let path = common::fixtures().join("parts").join("pin.step");
    let parts = load_all(&path).unwrap();
    let analysis = analyse(&parts[0], &parts);
    let hole = analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["family"] == "holes")
        .unwrap();
    let faces = hole["faces"].as_array().unwrap();
    assert!(
        faces
            .iter()
            .all(|f| analysis["faces"][f.as_u64().unwrap() as usize]["kind"] == "cylinder")
    );
    let i = intent(&analysis).unwrap();
    assert!(
        i.requirements
            .iter()
            .any(|r| r["kind"] == "thread" && r["side"] == "internal")
    );
    let report = write_parts(&[(&parts[0], &i, None)], &dir.join("pin.step")).unwrap();
    assert!(
        report
            .written
            .iter()
            .any(|w| w.starts_with("internal thread M5x0.8-6H on holes:"))
    );
    let _ = std::fs::remove_dir_all(&dir);
}
