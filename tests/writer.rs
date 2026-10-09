//! The writer: specify-core's `tests/test_writer.py`, `test_requirements.py` (the semantic
//! content), `test_resume.py`, `test_merge.py` (re-expressed for haecceity's add mode),
//! `test_conformance.py` (as docs/design.md "Oracle and tests" keeps it), `test_drawn.py`
//! (datum symbols) and the write side of `test_threads.py` and `test_mmc_finish_notes.py`.
//!
//! Each test names the Python test it ports. Analyses are the oracle's (Python's, captured), so
//! intents come from the Rust rules on the analysis the Python test used; the analysis of a
//! file the writer wrote is that analysis with the file's binding and its PMI as the Rust reader
//! reads it (as `api.analyse` composes it: a file whose answers resume is analysed as its
//! geometry alone). What Python asserted of the text route, OpenCascade's text fixes and
//! leaders is not ported (not-applicable entries in `known_differences/writer.json`, raised by
//! `tests/write_oracle.rs`); every other assertion is kept, or re-expressed where haecceity
//! writes the same intent another way, which the comment says.

mod common;

use std::path::{Path, PathBuf};

use haecceity::p21::Document;
use haecceity::pmi::{self, DimTolerance};
use haecceity::step::read_part_definitions;
use regex::Regex;
use serde_json::{Map, Value, json};
use specify_core::existing::{part_settings, read_existing};
use specify_core::features::describe_faces;
use specify_core::part::{LoadedPart, load};
use specify_core::resume::{answers_for, stored};
use specify_core::rules::{Intent, apply_with_defaults, questions};
use specify_core::writer::{WriteError, WriteReport, write};

// --- helpers ---------------------------------------------------------------------------------

/// A fresh directory for one test.
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-writer-tests")
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

/// The oracle's analysis of a case (Python's).
fn analysis(case: &str) -> Value {
    common::case(case)
        .artifact("analysis")
        .expect("a captured analysis")
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

fn apply(analysis: &Value, given: Value) -> Intent {
    apply_with_defaults(analysis, &answers(given), true).expect("an intent")
}

/// The analysis of `loaded` (a file the writer wrote, of the same faces as `base`'s): `base`
/// with the file's binding and the file's PMI as the Rust reader reads it; a file whose
/// answers resume is analysed as its geometry alone and carries them (as `api.analyse`).
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
            out.as_object_mut().unwrap().remove("resume");
        }
    }
    out
}

/// A feature id by family and recorded diameter.
fn feature(analysis: &Value, family: &str, diameter: f64) -> String {
    analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["family"] == family && f["record"]["diameter"].as_f64() == Some(diameter))
        .unwrap_or_else(|| panic!("no {family} of diameter {diameter}"))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The id of the plane face highest in z.
fn top_face(analysis: &Value) -> i64 {
    let faces = analysis["faces"].as_array().unwrap();
    let z = |f: &Value| f["centroid"][2].as_f64().unwrap();
    faces
        .iter()
        .filter(|f| f["kind"] == "plane")
        .max_by(|a, b| z(a).total_cmp(&z(b)))
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

/// The question whose id starts `prefix` and whose prompt contains `text`.
fn question(analysis: &Value, prefix: &str, text: &str) -> String {
    questions(analysis, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id.starts_with(prefix) && q.prompt.contains(text))
        .unwrap_or_else(|| panic!("no question {prefix} .. {text}"))
        .id
}

/// The file's text with its line breaks folded, as Python's tests read it.
fn flat(path: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
    Regex::new(r"\s*\n\s*")
        .unwrap()
        .replace_all(&text, " ")
        .into_owned()
}

/// haecceity's reading of part `k` of the file.
fn pmi_of(path: &Path, k: usize) -> pmi::PartPmi {
    let bytes = std::fs::read(path).unwrap();
    let defs = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    pmi::read(&doc, &defs).unwrap().parts.swap_remove(k)
}

fn types(path: &Path) -> Vec<String> {
    read_existing(&load(path, None).unwrap())
        .unwrap()
        .into_iter()
        .map(|e| e.r#type)
        .collect()
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The plate's intent with only its material answered (Python's `_plate_intent`).
fn plate_intent() -> Intent {
    apply(&analysis("parts/plate"), json!({"part.material": "steel"}))
}

/// A datum on the largest plane and a position of the first cylinder against it (Python's
/// `_datum_and_position`), from the Rust engine's own faces of `loaded`.
fn datum_and_position(loaded: &LoadedPart) -> Intent {
    let faces = describe_faces(loaded).unwrap();
    let plane = faces
        .iter()
        .filter(|f| f.kind == "plane")
        .max_by(|a, b| a.area.total_cmp(&b.area))
        .unwrap();
    let bore = faces.iter().find(|f| f.kind == "cylinder").unwrap();
    let mut intent = Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    intent.datums = vec![answers(json!({"letter": "A", "faces": [plane.id]}))];
    intent.requirements = vec![answers(json!({
        "kind": "position", "feature": "bore", "faces": [bore.id], "tolerance": 0.1,
        "diametral": true, "datums": ["A"],
    }))];
    intent
}

/// The notes every accept-defaults intent has, which haecceity cannot write yet (U3, Q3).
fn default_notes() -> Vec<String> {
    ["surface finish Ra 3.2", "edges Break sharp edges 0.2 max"]
        .iter()
        .map(|n| {
            format!(
                "{n}: haecceity writes no notes yet, having no standard form for them (upstream \
             need U3, question Q3)"
            )
        })
        .collect()
}

// --- test_writer.py --------------------------------------------------------------------------

/// `spool_written`: every kind of requirement v1 writes, including a g6 shaft fit, on corpus
/// `cadgenbench/flanged_spool_132` (skipped without the corpus).
fn spool_written(dir: &Path) -> Option<(Intent, WriteReport, PathBuf, Value)> {
    let case = common::case("corpus/cadgenbench/flanged_spool_132");
    let input = case.input()?;
    let analysis = case.artifact("analysis").unwrap();
    let ids: Vec<String> = questions(&analysis, &Map::new())
        .unwrap()
        .into_iter()
        .map(|q| q.id)
        .collect();
    let pattern = ids
        .iter()
        .find(|i| i.starts_with("hole.function:hole_patterns"))
        .unwrap();
    let bore = ids
        .iter()
        .find(|i| i.starts_with("hole.function:holes"))
        .unwrap();
    let turned: Vec<&String> = ids
        .iter()
        .filter(|i| i.starts_with("diameter.fit"))
        .collect();
    let given = json!({
        "part.material": "6082-T6",
        pattern.clone(): "fit:H7",
        bore.clone(): "fit:H7",
        turned[0].clone(): "p6",
        turned[1].clone(): "g6",
        "datum.A.flatness": "0.02",
    });
    let intent = apply(&analysis, given);
    let out = dir.join("spool-pmi.step");
    let report = write(&load(&input, None).unwrap(), &intent, &out, None).unwrap();
    Some((intent, report, out, analysis))
}

// test_writer.py::test_write_verifies_and_reports, test_limits_are_stated_correctly_in_the_file,
// test_existing_pmi_is_read_back_onto_the_same_faces, test_existing_pmi_suppresses_questions
#[test]
fn the_spool_is_written_verified_and_read_back() {
    let dir = temp("spool");
    let Some((intent, report, out, analysis)) = spool_written(&dir) else {
        println!("skipped: no corpus");
        return;
    };
    // Datums, requirements, material and general tolerance; the part's notes (surface finish
    // and edges at their defaults) are not written but reported (U3, Q3).
    assert_eq!(
        report.written.len(),
        intent.datums.len() + intent.requirements.len() + 2
    );
    assert!(report.written.contains(&"material 6082-T6".to_string()));
    assert_eq!(report.not_written, default_notes());
    assert!(report.warnings.is_empty());

    // The limits: each fit is written as its ISO 286 class (Python wrote bare limits, as
    // OpenCascade writes the grade wrongly), including g6 wholly below nominal and p6 above.
    let read = pmi_of(&out, 0);
    let classes: Vec<String> = read
        .dimensions
        .iter()
        .filter_map(|d| match &d.tolerance {
            DimTolerance::Fit { class, .. } => Some(class.to_string()),
            _ => None,
        })
        .collect();
    for req in intent.requirements.iter().filter(|r| r["kind"] == "size") {
        let fit = req["fit"].as_str().unwrap();
        assert!(classes.iter().any(|c| c == fit), "{fit} not in {classes:?}");
    }
    assert!(classes.iter().any(|c| c == "g6") && classes.iter().any(|c| c == "p6"));

    // Read back onto the same faces.
    let back = load(&out, None).unwrap();
    let found = read_existing(&back).unwrap();
    let positions: Vec<_> = found.iter().filter(|e| e.r#type == "Position").collect();
    assert!(positions.iter().all(|e| e.datums == ["A", "B"]));
    let mut wanted: Vec<Vec<usize>> = intent
        .requirements
        .iter()
        .filter(|r| r["kind"] == "position")
        .map(|r| serde_json::from_value(r["faces"].clone()).unwrap())
        .collect();
    let mut got: Vec<Vec<usize>> = positions.iter().map(|e| e.faces.clone()).collect();
    wanted.sort();
    wanted.dedup();
    got.sort();
    got.dedup();
    assert_eq!(got, wanted);

    // Re-analysing the enriched file does not ask again for sizes it states.
    let before: Vec<String> = questions(&analysis, &Map::new())
        .unwrap()
        .into_iter()
        .map(|q| q.id)
        .collect();
    assert!(before.iter().any(|i| i.starts_with("hole.function")));
    let after = questions(&reanalysed(&analysis, &back), &Map::new()).unwrap();
    assert!(
        !after
            .iter()
            .any(|q| q.id.starts_with("hole.function:holes"))
    );
}

// test_writer.py::test_intent_for_another_file_is_refused
#[test]
fn intent_for_another_file_is_refused() {
    let dir = temp("another");
    let pin = load(&fixture("pin"), None).unwrap();
    match write(&pin, &plate_intent(), &dir.join("x.step"), None) {
        Err(WriteError::Invalid(m)) => assert!(m.contains("different file"), "{m}"),
        other => panic!("not refused: {other:?}"),
    }
    assert!(listing(&dir).is_empty());
}

// test_writer.py::test_unreferenced_datums_are_warned
#[test]
fn unreferenced_datums_are_warned() {
    let dir = temp("unreferenced");
    let analysis = analysis("parts/plate");
    let plate = load(&fixture("plate"), None).unwrap();
    let mut given = json!({"part.material": "steel"});
    for q in questions(&analysis, &Map::new()).unwrap() {
        if q.id.starts_with("hole.function") {
            given[q.id] = json!("general");
        }
    }
    let report = write(
        &plate,
        &apply(&analysis, given.clone()),
        &dir.join("p.step"),
        None,
    )
    .unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    given["datum.A"] = json!([top_face(&analysis)]);
    let report = write(&plate, &apply(&analysis, given), &dir.join("q.step"), None).unwrap();
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    // haecceity writes and reads an unreferenced datum (Python's reader drops it).
    let back = read_existing(&load(&dir.join("q.step"), None).unwrap()).unwrap();
    assert!(back.iter().any(|e| e.kind == "datum" && e.r#type == "A"));
}

// test_writer.py::test_pmi_is_written_on_an_assembly_holding_one_solid
#[test]
fn pmi_is_written_on_an_assembly_holding_one_solid() {
    let dir = temp("wrapped");
    let loaded = load(&fixture("wrapped"), None).unwrap();
    let report = write(
        &loaded,
        &datum_and_position(&loaded),
        &dir.join("out.step"),
        None,
    )
    .unwrap();
    assert_eq!(report.written, ["datum A", "position 0.1 on bore"]);
}

// test_writer.py::test_tolerances_without_any_dimension_keep_their_size
#[test]
fn tolerances_without_any_dimension_keep_their_size() {
    let dir = temp("nodimension");
    let loaded = load(&fixture("plate"), None).unwrap();
    let out = dir.join("out.step");
    write(&loaded, &datum_and_position(&loaded), &out, None).unwrap();
    let back = read_existing(&load(&out, None).unwrap()).unwrap();
    let positions: Vec<_> = back.iter().filter(|e| e.r#type == "Position").collect();
    assert_eq!(positions.len(), 1);
    assert!((positions[0].value.unwrap() - 0.1).abs() < 1e-12);
}

// test_writer.py::test_intent_from_another_loader_version_is_refused
#[test]
fn intent_from_another_loader_version_is_refused() {
    let dir = temp("loader");
    let mut intent = plate_intent();
    intent.binding["loader_version"] = json!(1);
    let plate = load(&fixture("plate"), None).unwrap();
    match write(&plate, &intent, &dir.join("out.step"), None) {
        Err(WriteError::Invalid(m)) => assert_eq!(
            m,
            "intent was made by loader version 1, not 2: analyse the file again"
        ),
        other => panic!("not refused: {other:?}"),
    }
    assert!(listing(&dir).is_empty());
}

/// An intent haecceity refuses as invalid once everything else is checked: a flatness citing a
/// datum (`flatness_tolerance` WR1 forbids datums).
fn refused_intent(base: Intent) -> Intent {
    let mut intent = base;
    let faces = intent.datums[0]["faces"].clone();
    intent.requirements.push(answers(json!({
        "kind": "flatness", "feature": "", "faces": faces, "tolerance": 0.05, "datums": ["A"],
    })));
    intent
}

// test_writer.py::test_a_failed_write_leaves_the_destination_and_the_source. Python injects a
// failing `verify`; here the failures are real: haecceity refusing the intent, and a
// destination that cannot be replaced (a directory) once the file is written and verified.
#[test]
fn a_failed_write_leaves_the_destination_and_the_source() {
    let dir = temp("failed");
    let source = dir.join("part.step");
    std::fs::copy(fixture("plate"), &source).unwrap();
    let destination = dir.join("out.step");
    std::fs::write(&destination, "what was there before").unwrap();
    let before = std::fs::read(&source).unwrap();
    let loaded = load(&source, None).unwrap();
    let intent = refused_intent(plate_intent());
    for target in [&destination, &source] {
        match write(&loaded, &intent, target, None) {
            Err(WriteError::Invalid(m)) => assert!(m.contains("Flatness takes no datums"), "{m}"),
            other => panic!("not refused: {other:?}"),
        }
    }
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "what was there before"
    );
    assert_eq!(std::fs::read(&source).unwrap(), before);
    assert_eq!(listing(&dir), ["out.step", "part.step"]);

    // Written and verified, but the destination is a directory: nothing is left behind.
    let blocked = dir.join("blocked.step");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("inside"), "x").unwrap();
    assert!(matches!(
        write(&loaded, &plate_intent(), &blocked, None),
        Err(WriteError::Failed(_))
    ));
    assert_eq!(listing(&dir), ["blocked.step", "out.step", "part.step"]);
    assert_eq!(listing(&blocked), ["inside"]);
}

// test_writer.py::test_a_failed_merge_leaves_a_part_with_pmi_as_it_was: the add-mode path
// (a part with PMI of its own and no answers to resume) is staged too.
#[test]
fn a_failed_add_leaves_a_part_with_pmi_as_it_was() {
    let dir = temp("failedadd");
    let source = dir.join("part.step");
    let plate = load(&fixture("plate"), None).unwrap();
    write(&plate, &plate_intent(), &source, None).unwrap(); // PMI, and no answers to resume
    let loaded = load(&source, None).unwrap();
    assert!(!read_existing(&loaded).unwrap().is_empty());
    assert!(answers_for(&loaded).unwrap().is_none());
    let before = std::fs::read(&source).unwrap();
    let analysis = reanalysed(&analysis("parts/plate"), &loaded);
    let intent = refused_intent(apply(&analysis, json!({"part.material": "steel"})));
    assert!(matches!(
        write(&loaded, &intent, &source, None),
        Err(WriteError::Invalid(_))
    ));
    assert_eq!(std::fs::read(&source).unwrap(), before);
    assert_eq!(listing(&dir), ["part.step"]);
}

// test_writer.py::test_the_output_keeps_its_permissions_and_its_symlink
#[cfg(unix)]
#[test]
fn the_output_keeps_its_permissions_and_its_symlink() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp("symlink");
    let target = dir.join("real.step");
    std::fs::write(&target, "old").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.join("link.step");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let plate = load(&fixture("plate"), None).unwrap();
    let report = write(&plate, &plate_intent(), &link, None).unwrap();
    assert_eq!(report.output, link);
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(std::fs::read_to_string(&target).unwrap().contains("DATUM("));
    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(listing(&dir), ["link.step", "real.step"]);
}

// test_writer.py::test_the_command_line_says_why_it_cannot_write is the command line's
// (stage 4); its refusal is `intent_from_another_loader_version_is_refused` above.

// --- binding checks, with Python's messages -----------------------------------------------------

#[test]
fn the_binding_is_checked_with_pythons_messages() {
    let dir = temp("binding");
    let plate = load(&fixture("plate"), None).unwrap();
    let refused = |intent: &Intent| match write(&plate, intent, &dir.join("x.step"), None) {
        Err(WriteError::Invalid(m)) => m,
        other => panic!("not refused: {other:?}"),
    };
    let mut intent = plate_intent();
    intent.binding["part"] = json!(1);
    assert_eq!(refused(&intent), "intent was made for part 1, not part 0");
    let mut intent = plate_intent();
    intent.binding["face_count"] = json!(13);
    assert_eq!(
        refused(&intent),
        "intent face count does not match the file"
    );
    let mut intent = plate_intent();
    intent
        .binding
        .as_object_mut()
        .unwrap()
        .remove("loader_version");
    assert_eq!(
        refused(&intent),
        "intent was made by loader version None, not 2: analyse the file again"
    );
    // An intent made before parts were told apart is part 0's
    // (test_assembly.py::test_an_intent_made_before_parts_were_told_apart_still_writes).
    let mut intent = plate_intent();
    intent.binding.as_object_mut().unwrap().remove("part");
    assert!(write(&plate, &intent, &dir.join("old.step"), None).is_ok());
    assert!(matches!(
        specify_core::writer::write_parts(&[], &dir.join("x.step")),
        Err(WriteError::Invalid(m)) if m == "nothing to write"
    ));
}

// --- test_requirements.py (semantic content) ----------------------------------------------------

/// `pin_written`: an M6 thread and a straight knurl on the pin.
fn pin_written(dir: &Path) -> (Value, WriteReport, PathBuf) {
    let analysis = analysis("parts/pin");
    let given = json!({
        "part.material": "CW614N leaded brass",
        format!("diameter.fit:{}", feature(&analysis, "turned_steps", 6.0)): "thread:M6",
        format!("diameter.fit:{}", feature(&analysis, "turned_steps", 10.0)): "knurl:straight",
    });
    let out = dir.join("pin-pmi.step");
    let pin = load(&fixture("pin"), None).unwrap();
    let report = write(&pin, &apply(&analysis, given), &out, None).unwrap();
    (analysis, report, out)
}

// test_requirements.py::test_everything_is_written_and_verified. The straight knurl is not
// written: AP242's turned_knurl needs parameters the intent lacks (question Q4); it is
// reported in not_written.
#[test]
fn everything_is_written_and_verified() {
    let dir = temp("pin");
    let (_, report, _) = pin_written(&dir);
    for wanted in [
        "internal thread M5x0.8-6H",
        "external thread M6x1-6g",
        "material CW614N leaded brass",
        "general tolerance ISO 2768-m",
    ] {
        assert!(
            report.written.iter().any(|w| w.starts_with(wanted)),
            "{wanted}"
        );
    }
    assert!(
        report
            .not_written
            .iter()
            .any(|w| w.starts_with("straight knurl on turned_steps:0: "))
    );
    assert!(!report.written.iter().any(|w| w.contains("knurl")));
}

// test_requirements.py::test_the_standard_constructs, re-expressed: threads as AP242's THREAD
// with their manufacturing data as a user defined attribute set (U8), the general tolerance
// as the 'default tolerances' class, the material as 'material name'.
#[test]
fn the_standard_constructs() {
    let dir = temp("constructs");
    let (_, _, out) = pin_written(&dir);
    let text = flat(&out);
    assert_eq!(
        Regex::new(r"= ?THREAD\(").unwrap().find_iter(&text).count(),
        2
    );
    assert_eq!(
        Regex::new(r"GENERAL_PROPERTY\('','thread manufacturing',\$\)")
            .unwrap()
            .find_iter(&text)
            .count(),
        2
    );
    assert!(Regex::new(r"MEASURE_WITH_UNIT\(LENGTH_MEASURE\(5\.5\),#\d+\)REPRESENTATION_ITEM\('minimum full thread'\)").unwrap().is_match(&text));
    assert!(text.contains("DESCRIPTIVE_REPRESENTATION_ITEM('tolerance class','ISO 2768-m')"));
    assert!(text.contains("PROPERTY_DEFINITION('material property','material name'"));
    assert!(!text.contains("DEFAULT_TOLERANCE_TABLE"));
    let read = pmi_of(&out, 0);
    let fits: Vec<&str> = read.threads.iter().map(|t| t.fit_class.as_str()).collect();
    assert!(fits.contains(&"6H") && fits.contains(&"6g"), "{fits:?}");
}

// test_requirements.py::test_the_file_still_reads_with_the_same_faces,
// test_faces_resolve_inside_an_assembly
#[test]
fn the_file_still_reads_with_the_same_faces() {
    let dir = temp("faces");
    let (_, _, out) = pin_written(&dir);
    let pin = load(&fixture("pin"), None).unwrap();
    let back = load(&out, None).unwrap();
    assert_eq!(back.binding.face_count, pin.binding.face_count);
    assert_eq!(back.definition.faces.len(), pin.binding.face_count);
    let wrapped = load(&fixture("wrapped"), None).unwrap();
    assert_eq!(wrapped.definition.faces.len(), wrapped.binding.face_count);
}

// test_requirements.py::test_its_callouts_read_back_as_notes_not_as_sizes_of_nothing
#[test]
fn threads_read_back_as_notes_not_as_sizes_of_nothing() {
    let dir = temp("notes");
    let (_, _, out) = pin_written(&dir);
    let existing = read_existing(&load(&out, None).unwrap()).unwrap();
    assert!(
        !existing
            .iter()
            .any(|e| e.r#type == "CommonLabel" || e.kind == "dimension")
    );
    let kinds: Vec<&str> = existing
        .iter()
        .filter(|e| e.kind == "note")
        .map(|e| e.r#type.as_str())
        .collect();
    assert!(kinds.contains(&"internal thread") && kinds.contains(&"external thread"));
}

// test_requirements.py::test_a_finish_on_faces_is_not_a_second_default, re-expressed: neither
// finish is written (U3, Q3); both are reported.
#[test]
fn a_finish_on_faces_is_reported_not_written() {
    let dir = temp("finish");
    let analysis = analysis("parts/pin");
    let top = top_face(&analysis);
    let given = json!({
        "part.material": "brass",
        "extra.0": {"faces": [top], "kind": "finish", "value": "Ra 0.8"},
    });
    let pin = load(&fixture("pin"), None).unwrap();
    let out = dir.join("finishes.step");
    let report = write(&pin, &apply(&analysis, given), &out, None).unwrap();
    assert!(
        report
            .not_written
            .iter()
            .any(|n| n.starts_with(&format!("surface finish Ra 0.8 on faces [{top}]: ")))
    );
    assert!(
        report
            .not_written
            .iter()
            .any(|n| n.starts_with("surface finish Ra 3.2: "))
    );
    assert!(
        !read_existing(&load(&out, None).unwrap())
            .unwrap()
            .iter()
            .any(|e| e.r#type == "surface finish")
    );
}

// --- test_merge.py, for haecceity's add mode ------------------------------------------------------

/// `enriched`: the plate with datums A, B, C and a fitted, positioned Ø8 bore, written without
/// answers (so its PMI is, to a later write, the file's own).
fn enriched(dir: &Path) -> PathBuf {
    let analysis = analysis("parts/plate");
    let given = json!({
        "part.material": "steel",
        question(&analysis, "hole.function", "Ø8"): "fit:H7",
        question(&analysis, "hole.function", "6×"): "general",
    });
    let out = dir.join("first.step");
    let plate = load(&fixture("plate"), None).unwrap();
    write(&plate, &apply(&analysis, given), &out, None).unwrap();
    out
}

// test_merge.py::test_only_a_part_with_pmi_is_merged, test_what_the_file_states_is_not_asked_
// again, test_the_new_pmi_cites_the_files_datums, test_the_original_file_is_kept_byte_for_byte
#[test]
fn pmi_is_added_to_a_part_that_has_some() {
    let dir = temp("merge");
    let first = enriched(&dir);
    let plate = load(&fixture("plate"), None).unwrap();
    assert!(read_existing(&plate).unwrap().is_empty());
    let loaded = load(&first, None).unwrap();
    assert!(!read_existing(&loaded).unwrap().is_empty());

    let analysis = reanalysed(&analysis("parts/plate"), &loaded);
    let qs = questions(&analysis, &Map::new()).unwrap();
    let ids: Vec<&str> = qs.iter().map(|q| q.id.as_str()).collect();
    for stated in [
        "datum.A",
        "datum.B",
        "datum.C",
        "part.material",
        "part.general_tolerance",
    ] {
        assert!(!ids.contains(&stated), "{stated} asked again");
    }
    // The part's notes were not written (U3, Q3), so they are offered again with coating and
    // heat treatment (Python wrote the finish and edges, and offered only those two).
    for q in ids.iter().filter(|i| i.starts_with("part.")) {
        assert!(
            [
                "part.coating",
                "part.heat_treatment",
                "part.surface_finish",
                "part.edges"
            ]
            .contains(q),
            "{q}"
        );
    }
    assert!(!qs.iter().any(|q| q.prompt.contains("Ø8")));
    let six = qs
        .iter()
        .find(|q| q.prompt.contains("6×"))
        .unwrap()
        .id
        .clone();
    let intent = apply(&analysis, json!({ six: "clearance:M6" }));
    let second = dir.join("second.step");
    let report = write(&loaded, &intent, &second, None).unwrap();
    assert!(
        !report
            .written
            .iter()
            .any(|w| w.starts_with("material") || w.starts_with("general tolerance"))
    );

    // The new PMI cites the file's datums: none is written again.
    let joined = report.written.join(" ");
    assert!(
        Regex::new(r"position [\d.]+ MMC on hole_patterns")
            .unwrap()
            .is_match(&joined)
    );
    assert!(!report.written.iter().any(|w| w.starts_with("datum ")));
    let letters: Vec<String> = Regex::new(r"DATUM\('[^']*',[^;]*'(\w)'\);")
        .unwrap()
        .captures_iter(&std::fs::read_to_string(&second).unwrap())
        .map(|c| c[1].to_string())
        .collect();
    let mut sorted = letters.clone();
    sorted.sort();
    assert_eq!(sorted, ["A", "B", "C"]);

    // The original file is kept byte for byte around what was added, and all it stated reads
    // back with more beside it.
    let (before, after) = (
        std::fs::read_to_string(&first).unwrap(),
        std::fs::read_to_string(&second).unwrap(),
    );
    let end = before.rfind("ENDSEC;").unwrap();
    assert!(after.starts_with(&before[..end]) && after.ends_with(&before[end..]));
    let old = read_existing(&loaded).unwrap();
    let new = read_existing(&load(&second, None).unwrap()).unwrap();
    assert!(old.iter().all(|e| new.contains(e)) && new.len() > old.len());
    // Its PMI is not the writer's own: no answers are stored.
    assert!(stored(after.as_bytes()).is_empty());
}

// test_merge.py::test_tolerances_written_simply_survive_the_merge
#[test]
fn tolerances_with_datums_survive_an_add() {
    let dir = temp("simple");
    let first = enriched(&dir);
    let loaded = load(&first, None).unwrap();
    let analysis = reanalysed(&analysis("parts/plate"), &loaded);
    let faces = analysis["faces"].as_array().unwrap();
    let bore = faces
        .iter()
        .find(|f| f["kind"] == "cylinder" && (f["radius"].as_f64().unwrap() - 4.0).abs() < 1e-6)
        .unwrap()["id"]
        .clone();
    let top = top_face(&analysis);
    let given = json!({
        "extra.0": {"faces": [bore], "kind": "runout", "value": "0.02", "datums": ["A"]},
        "extra.1": {"faces": [bore], "kind": "total_runout", "value": "0.02", "datums": ["A"]},
        "extra.2": {"faces": [top], "kind": "parallelism", "value": "0.02", "datums": ["A"]},
    });
    let out = dir.join("simple.step");
    write(&loaded, &apply(&analysis, given), &out, None).unwrap();
    let read = types(&out);
    for t in ["CircularRunout", "TotalRunout", "Parallelism"] {
        assert!(read.iter().any(|x| x == t), "{t} in {read:?}");
    }
}

/// NIST CTC-01 (AP242 with semantic PMI), from the committed fixture.
fn ctc01() -> (common::Case, PathBuf) {
    let case = common::case("nist/nist_ctc_01_asme1_ap242");
    let input = case.input().unwrap();
    (case, input)
}

// test_merge.py::test_a_nist_part_keeps_all_its_pmi, test_a_files_own_datums_are_not_drawn_
// again, test_a_file_with_pmi_from_elsewhere_stores_no_answers
#[test]
fn a_nist_part_keeps_all_its_pmi() {
    let dir = temp("ctc01");
    let (case, input) = ctc01();
    let analysis = case.artifact("analysis").unwrap();
    let asked: Vec<String> = questions(&analysis, &Map::new())
        .unwrap()
        .into_iter()
        .map(|q| q.id.split(':').next().unwrap().to_string())
        .collect();
    let allowed = [
        "part.material",
        "part.general_tolerance",
        "part.surface_finish",
        "part.coating",
        "part.heat_treatment",
        "part.edges",
        "datum.B.control",
        "datum.C.control",
    ];
    assert!(
        asked.iter().all(|a| allowed.contains(&a.as_str())),
        "{asked:?}"
    );
    let given = json!({"part.material": "CZ121"});
    let loaded = load(&input, None).unwrap();
    let out = dir.join("ctc01.step");
    let report = write(
        &loaded,
        &apply(&analysis, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    assert_eq!(
        report.written,
        ["material CZ121", "general tolerance ISO 2768-m"]
    );
    let old = read_existing(&loaded).unwrap();
    let new = read_existing(&load(&out, None).unwrap()).unwrap();
    assert_eq!(old, new);
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains("DRAUGHTING_CALLOUT('Datum "));
    assert!(stored(text.as_bytes()).is_empty());
    // Every original byte is kept, and the schema with it (an AP242 file stays AP242).
    let original = std::fs::read_to_string(&input).unwrap();
    let end = original.rfind("ENDSEC;").unwrap();
    assert!(text.starts_with(&original[..end]) && text.ends_with(&original[end..]));
}

// --- test_threads.py, test_mmc_finish_notes.py (write side) ------------------------------------

// test_threads.py::test_a_typed_thread_depth_is_written_and_read_back (the write side). The
// sentence "5.6 mm minimum full thread" is the text route, not written (not-applicable).
#[test]
fn a_typed_thread_depth_is_written_and_read_back() {
    let dir = temp("depth");
    let analysis = analysis("parts/pin");
    let hole = format!("hole.function:{}", feature(&analysis, "holes", 4.2));
    let given = json!({"part.material": "brass", hole: "tapped:M5@5.6"});
    let out = dir.join("deep.step");
    let pin = load(&fixture("pin"), None).unwrap();
    let report = write(
        &pin,
        &apply(&analysis, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    assert_eq!(report.not_written, default_notes());
    let text = flat(&out);
    assert!(Regex::new(r"MEASURE_WITH_UNIT\(LENGTH_MEASURE\(5\.6\),#\d+\)REPRESENTATION_ITEM\('minimum full thread'\)").unwrap().is_match(&text));
    assert!(Regex::new(r"MEASURE_WITH_UNIT\(LENGTH_MEASURE\(8\.\),#\d+\)REPRESENTATION_ITEM\('tapping drill depth'\)").unwrap().is_match(&text));
    assert_eq!(
        answers_for(&load(&out, None).unwrap()).unwrap(),
        given.as_object().cloned()
    );
}

/// test_mmc_finish_notes.py's `written`: MMC positions, every part note and a face finish.
fn notes_written(dir: &Path) -> (Intent, WriteReport, PathBuf, i64) {
    let analysis = analysis("parts/plate");
    let top = top_face(&analysis);
    let given = json!({
        "part.material": "Aluminium 6082-T6",
        question(&analysis, "hole.function", "Ø8"): "fit:H7",
        "part.coating": "Anodise black (MIL-A-8625 Type II)",
        "part.heat_treatment": "Solution treat and age to T6",
        "part.surface_finish": "Ra 1.6",
        "extra.1": {"faces": [top], "kind": "finish", "value": "0.8"},
    });
    let intent = apply(&analysis, given);
    let out = dir.join("plate.step");
    let plate = load(&fixture("plate"), None).unwrap();
    let report = write(&plate, &intent, &out, None).unwrap();
    (intent, report, out, top)
}

// test_mmc_finish_notes.py::test_clearance_holes_are_positioned_at_mmc_and_fitted_ones_are_not
// (write side), test_the_parts_notes_are_written_and_read_back and test_a_face_finish_is_
// written_on_its_face (re-expressed: reported, not written, U3/Q3),
// test_nothing_is_written_for_no_coating_or_heat_treatment
#[test]
fn mmc_positions_are_written_and_notes_are_reported() {
    let dir = temp("mmc");
    let (intent, report, out, top) = notes_written(&dir);
    assert!(
        report
            .written
            .iter()
            .any(|w| w.starts_with("position") && w.contains(" MMC on "))
    );
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("MAXIMUM_MATERIAL_REQUIREMENT")
    );
    assert!(
        intent.part["coating"]
            .as_str()
            .unwrap()
            .starts_with("Anodise black")
    );
    for note in [
        "surface finish Ra 1.6: ",
        "coating Anodise black (MIL-A-8625 Type II): ",
        "heat treatment Solution treat and age to T6: ",
        "edges Break sharp edges 0.2 max: ",
    ] {
        assert!(
            report.not_written.iter().any(|n| n.starts_with(note)),
            "{note}"
        );
    }
    assert!(
        report
            .not_written
            .iter()
            .any(|n| n.starts_with(&format!("surface finish Ra 0.8 on faces [{top}]: ")))
    );
    assert_eq!(
        part_settings(&load(&out, None).unwrap()).unwrap(),
        answers(json!({"material": "Aluminium 6082-T6", "general_tolerance": "ISO 2768-m"}))
    );

    let plain = apply(&analysis("parts/plate"), json!({"part.material": "steel"}));
    assert!(!plain.part.contains_key("coating") && !plain.part.contains_key("heat_treatment"));
    let p = dir.join("p.step");
    write(&load(&fixture("plate"), None).unwrap(), &plain, &p, None).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(
        !Regex::new(r"'(surface treatment|heat treatment)'")
            .unwrap()
            .is_match(&text)
    );
}

// --- test_conformance.py, as kept -----------------------------------------------------------------

/// Every instance of the file is a valid AP242 instance and no EXPRESS rule haecceity checks
/// is violated.
fn assert_conformant(path: &Path) {
    let doc = Document::parse(std::fs::read(path).unwrap()).unwrap();
    let violations = haecceity::express::validate_document(doc.graph());
    assert!(violations.is_empty(), "{}: {violations:?}", path.display());
    let rules = haecceity::express_rules::check_all(&doc);
    assert!(rules.is_empty(), "{}: {rules:?}", path.display());
}

// test_conformance.py::test_no_malformed_runout_zone_is_written: the zone is written as its
// tolerance zone form only, every reference resolves, and the file conforms.
#[test]
fn no_malformed_runout_zone_is_written() {
    let dir = temp("zones");
    let analysis = analysis("parts/pin");
    let runout = questions(&analysis, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id.starts_with("diameter.runout"))
        .unwrap()
        .id;
    let intent = apply(&analysis, json!({"part.material": "brass", runout: "0.02"}));
    assert!(
        intent
            .requirements
            .iter()
            .any(|r| r.get("diametral") == Some(&json!(true)))
    );
    assert!(intent.requirements.iter().any(|r| r["kind"] == "runout"));
    let out = dir.join("zones.step");
    let report = write(&load(&fixture("pin"), None).unwrap(), &intent, &out, None).unwrap();
    assert!(
        report
            .written
            .iter()
            .any(|w| w.starts_with("runout 0.02 on "))
    );
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains("RUNOUT_ZONE"));
    assert!(!text.contains("PLANE_ANGLE_UNIT() SI_UNIT($,.METRE.)"));
    assert!(text.contains("TOLERANCE_ZONE_FORM('cylindrical or circular')"));
    let data = text.split_once("DATA;").unwrap().1;
    let ids: std::collections::BTreeSet<&str> = Regex::new(r"(?m)^#(\d+)\s*=")
        .unwrap()
        .captures_iter(&text)
        .map(|c| c.get(1).unwrap().as_str())
        .collect();
    for r in Regex::new(r"#(\d+)").unwrap().captures_iter(data) {
        assert!(
            ids.contains(r.get(1).unwrap().as_str()),
            "#{} dangles",
            &r[1]
        );
    }
    assert_conformant(&out);
}

// test_conformance.py::test_callout_presentation_is_named, as far as it applies: no unnamed
// tessellated presentation (the datum symbols are named 'datum'); the note leaders it checks
// are not written.
#[test]
fn presentation_is_named() {
    let dir = temp("named");
    let (_, _, out) = pin_written(&dir);
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains("TESSELLATED_GEOMETRIC_SET(''"));
    assert!(!text.contains("TESSELLATED_ANNOTATION_OCCURRENCE(''"));
    assert!(text.contains("TESSELLATED_GEOMETRIC_SET('datum'"));
    assert_conformant(&out);
}

// test_conformance.py::test_limits_are_typed_lengths_and_runout_is_simple
#[test]
fn limits_are_typed_lengths_and_runout_is_simple() {
    let dir = temp("typed");
    let analysis = analysis("parts/pin");
    let qs = questions(&analysis, &Map::new()).unwrap();
    let runout = qs
        .iter()
        .find(|q| q.id.starts_with("diameter.runout"))
        .unwrap()
        .id
        .clone();
    let fit = qs
        .iter()
        .find(|q| q.id.starts_with("diameter.fit"))
        .unwrap()
        .id
        .clone();
    let intent = apply(
        &analysis,
        json!({"part.material": "brass", runout: "0.02", fit: "g6"}),
    );
    assert!(intent.requirements.iter().any(|r| r["kind"] == "size"));
    let out = dir.join("typed.step");
    write(&load(&fixture("pin"), None).unwrap(), &intent, &out, None).unwrap();
    let text = flat(&out);
    assert!(
        !Regex::new(r"= ?MEASURE_WITH_UNIT\(\s*[-+0-9.]")
            .unwrap()
            .is_match(&text)
    );
    assert!(text.contains("LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE("));
    assert!(
        !Regex::new(r"(RUNOUT|PARALLELISM|PERPENDICULARITY)_TOLERANCE\s*\(\s*\)")
            .unwrap()
            .is_match(&text)
    );
    assert!(
        Regex::new(r"= ?CIRCULAR_RUNOUT_TOLERANCE\('")
            .unwrap()
            .is_match(&text)
    );
    // g6 lies wholly below nominal; it is written as the class.
    assert!(
        text.contains("LIMITS_AND_FITS('g','shaft','6',"),
        "no g6 class"
    );
    assert_conformant(&out);
}

// test_conformance.py::test_every_datum_referenced_tolerance_is_written_simply
#[test]
fn every_datum_referenced_tolerance_is_written_simply() {
    let dir = temp("kinds");
    let analysis = analysis("parts/plate");
    let planes: Vec<&Value> = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "plane")
        .collect();
    let top = top_face(&analysis);
    let side = planes
        .iter()
        .find(|f| f["direction"][2].as_f64().unwrap().abs() < 0.1)
        .unwrap()["id"]
        .clone();
    let bore = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "cylinder")
        .unwrap()["id"]
        .clone();
    let mut given = json!({"part.material": "steel"});
    for (n, (face, kind)) in [
        (json!(top), "parallelism"),
        (side, "perpendicularity"),
        (bore.clone(), "runout"),
        (bore, "total_runout"),
    ]
    .into_iter()
    .enumerate()
    {
        given[format!("extra.{n}")] =
            json!({"faces": [face], "kind": kind, "value": "0.05", "datums": ["A"]});
    }
    let out = dir.join("kinds.step");
    write(
        &load(&fixture("plate"), None).unwrap(),
        &apply(&analysis, given),
        &out,
        None,
    )
    .unwrap();
    let text = flat(&out);
    assert!(
        !Regex::new(r"(PARALLELISM|PERPENDICULARITY|RUNOUT)_TOLERANCE\s*\(\s*\)")
            .unwrap()
            .is_match(&text)
    );
    for kind in [
        "PARALLELISM",
        "PERPENDICULARITY",
        "CIRCULAR_RUNOUT",
        "TOTAL_RUNOUT",
    ] {
        assert!(
            Regex::new(&format!(r"= ?{kind}_TOLERANCE\('"))
                .unwrap()
                .is_match(&text),
            "{kind}"
        );
    }
    assert_conformant(&out);
}

// --- test_drawn.py: datum symbols --------------------------------------------------------------

// test_drawn.py::test_a_datum_symbol_presents_its_datum_feature, against haecceity's symbols.
// (test_a_note_is_its_leader_and_no_words: no note leaders are written, not-applicable.)
#[test]
fn a_datum_symbol_presents_its_datum_feature() {
    let dir = temp("drawn");
    let (_, _, out) = pin_written(&dir);
    let text = flat(&out);
    let names: Vec<(String, String)> = Regex::new(r"#(\d+)\s*=\s*DRAUGHTING_CALLOUT\('([^']+)'")
        .unwrap()
        .captures_iter(&text)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect();
    let symbols: Vec<&String> = names
        .iter()
        .filter(|(_, n)| n.starts_with("Datum "))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(symbols.len(), 2, "{names:?}");
    let linked: std::collections::BTreeMap<String, String> =
        Regex::new(r"DRAUGHTING_MODEL_ITEM_ASSOCIATION\('[^']*','',#(\d+),#\d+,#(\d+)\)")
            .unwrap()
            .captures_iter(&text)
            .map(|c| (c[2].to_string(), c[1].to_string()))
            .collect();
    for callout in symbols {
        let aspect = &linked[callout];
        assert!(
            Regex::new(&format!(r"#{aspect}\s*=\s*DATUM_FEATURE\("))
                .unwrap()
                .is_match(&text),
            "callout #{callout} presents #{aspect}, not a datum feature"
        );
    }
}

// --- determinism ---------------------------------------------------------------------------------

#[test]
fn writing_twice_gives_identical_bytes() {
    let dir = temp("twice");
    let analysis = analysis("parts/plate");
    let given = json!({"part.material": "steel"});
    let intent = apply(&analysis, given.clone());
    let plate = load(&fixture("plate"), None).unwrap();
    let (a, b) = (dir.join("a.step"), dir.join("b.step"));
    write(&plate, &intent, &a, given.as_object()).unwrap();
    write(&plate, &intent, &b, given.as_object()).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}
