//! Answers that leave no PMI behind are stored and read back (ADR P11). Reopening ignores
//! stored answers where the file already has PMI in their scope: the file states them, and
//! the file's PMI survives saving.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::part::{LoadedPart, load_bytes};
use specify_core::session::{self, Opened, Session};

/// The plate's answers that write no PMI, with the defaults it is offered. (Hole 9's
/// "general" writes its size under the general tolerance, which the file then states.)
const SILENT: [&str; 7] = [
    "part.coating",
    "part.heat_treatment",
    "part.notes",
    "part.locating",
    "datum.A.flatness",
    "datum.B.control",
    "datum.C.control",
];

/// Of the plate's defaults, some that write PMI.
const WRITING: [&str; 6] = [
    "datum.A",
    "datum.B",
    "datum.C",
    "hole.position:holes:12",
    "hole.function:hole_patterns.grid:6.7.8.10.11.13",
    "part.general_tolerance",
];

fn plate() -> PathBuf {
    common::fixtures().join("parts").join("plate.step")
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("answers_without_pmi")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn map(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn material() -> Map<String, Value> {
    map(json!({"part.material": "6082-T6"}))
}

fn load(bytes: Vec<u8>) -> LoadedPart {
    load_bytes("plate.step", bytes).unwrap()
}

/// Whether the feature table says the file states `id`.
fn stated(analysis: &Value, id: &str) -> bool {
    analysis["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["id"] == id && r["state"] == "stated")
}

/// Each of `ids` is resumed when it leaves no PMI behind, else stated by the file.
fn resumed_or_stated<'a>(analysis: &Value, ids: impl IntoIterator<Item = &'a String>) {
    let answers = analysis["resume"]["answers"].as_object().unwrap();
    for id in ids {
        assert!(
            answers.contains_key(id) || stated(analysis, id),
            "{id} is neither resumed nor stated: {}",
            analysis["resume"]
        );
    }
}

fn asked(analysis: &Value) -> (Vec<String>, Vec<String>) {
    let answers = analysis["resume"]["answers"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let review = api::review(analysis, &answers).unwrap();
    let ids = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|q| q.as_str().or_else(|| q["id"].as_str()).map(str::to_owned))
            .collect()
    };
    (ids(&review["questions"]), ids(&review["open"]))
}

/// The plate written with `answers` (defaults taken) and `stored` stored.
fn written(answers: &Map<String, Value>, stored: &Map<String, Value>) -> Vec<u8> {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let intent = api::apply(&analysis, answers, true).unwrap();
    api::write_bytes(
        std::fs::read(plate()).unwrap(),
        &[(intent, Some(stored.clone()))],
    )
    .unwrap()
    .0
}

fn session_of(path: &Path) -> session::Shared {
    Arc::new(Mutex::new(
        Session::open(Opened::read(path).unwrap(), false).unwrap(),
    ))
}

/// The open questions of `part` with a default, with it: what the page offers to approve.
fn offered(analysis: &Value, answers: &Map<String, Value>) -> Map<String, Value> {
    let qs = api::questions(analysis, answers).unwrap();
    specify_core::rules::open_questions(&qs, answers)
        .iter()
        .filter_map(|q| q.default.clone().map(|d| (q.id.clone(), d)))
        .collect()
}

/// The page's Save with the answers given, then every default offered approved, round by round
/// (taking one can ask another), each an edit list filling them: the file last saved.
fn approve_all(path: &Path, given: &Map<String, Value>) -> PathBuf {
    let shared = session_of(path);
    let mut fill = given.clone();
    let mut saved = None;
    loop {
        let analysis = session::analysis(&shared, 0).unwrap();
        let answers = session::lock(&shared)
            .answers(0)
            .cloned()
            .unwrap_or_default();
        let mut with_given = answers.clone();
        with_given.extend(fill.clone());
        fill.extend(offered(&analysis, &with_given));
        if fill.is_empty() {
            break;
        }
        let list = specify_core::edits::parse_value(json!({ "fill": fill })).unwrap();
        saved = Some(
            session::save_edits(&shared, None, &list, "page").unwrap_or_else(|e| panic!("{e}")),
        );
        fill = Map::new();
    }
    PathBuf::from(saved.expect("defaults were offered").path)
}

/// Approved on the page, the defaults that write no PMI are stored and resumed: reopening
/// does not ask them again.
#[test]
fn approved_defaults_that_write_no_pmi_resume_and_are_not_asked() {
    let dir = scratch("approved");
    let input = dir.join("plate.step");
    std::fs::copy(plate(), &input).unwrap();
    let saved = approve_all(&input, &material());
    let again = api::analyse_file(&saved, None).unwrap();
    let resume = &again["resume"];
    assert!(resume.get("conflicts").is_none(), "{resume}");
    let answers = resume["answers"].as_object().unwrap();
    // ADR P11: a "none" suggestion is not open, so the page never offers it for approval:
    // left as a gap, it is not stored and is not open on reopening either. The choice that
    // writes nothing (how holes are located) is offered, approved and stored.
    for id in SILENT {
        let none = id != "part.locating";
        assert_eq!(!answers.contains_key(id), none, "{id}: {resume}");
    }
    // Those that write PMI are not stored; the file states them.
    for id in WRITING {
        assert!(!answers.contains_key(id), "{id} is stored: {resume}");
        assert!(stated(&again, id), "{id} is not stated");
    }
    let (_, open) = asked(&again);
    assert!(open.is_empty(), "asked again: {open:?}");
}

/// `write --accept-defaults` (every default, the person's "accept all"): the defaults that
/// write no PMI are stored with the answers and resumed; those that write PMI are not stored
/// and read back from the file as present (#204), and nothing is asked again.
#[test]
fn accept_defaults_on_the_command_line_stores_the_defaults_that_write_no_pmi() {
    let dir = scratch("cli");
    let answers = dir.join("answers.json");
    let out = dir.join("out.step");
    let run = |args: &[&std::ffi::OsStr]| {
        let got = std::process::Command::new(env!("CARGO_BIN_EXE_specify-core"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            got.status.success(),
            "{}",
            String::from_utf8_lossy(&got.stderr)
        );
    };
    let analysed = api::analyse_file(&plate(), None).unwrap();
    std::fs::write(
        &answers,
        common::approve_edits(&analysed, &material()).to_string(),
    )
    .unwrap();
    run(&[
        "write".as_ref(),
        plate().as_os_str(),
        answers.as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
    ]);
    let again = api::analyse_file(&out, None).unwrap();
    let resume = &again["resume"];
    assert!(resume.get("conflicts").is_none(), "{resume}");
    let answers = resume["answers"].as_object().unwrap();
    assert!(stated(&again, "part.material"), "{resume}");
    // The file has an implicit locating method and did not offer it for approval. An
    // explicit edit list has no reason to turn that rule default into a stored choice.
    assert!(!answers.contains_key("part.locating"));
    for id in SILENT.into_iter().filter(|id| *id != "part.locating") {
        assert!(answers.contains_key(id), "{id} is not resumed: {resume}");
    }
    for id in WRITING {
        assert!(!answers.contains_key(id), "{id} is stored: {resume}");
        assert!(stated(&again, id), "{id} is not stated");
    }
    let (_, open) = asked(&again);
    assert!(open.is_empty(), "asked again: {open:?}");
}

/// `bytes` with a flatness of 0.05 on datum A's face added by another tool: haecceity's own
/// writer, so it carries no Specify provenance marker.
fn with_foreign_flatness(bytes: Vec<u8>) -> Vec<u8> {
    use haecceity::p21::Document;
    use haecceity::pmi;
    let mut flat = material();
    flat.insert("datum.A.flatness".into(), json!("0.05"));
    let source = written(&flat, &flat);
    let defs = haecceity::step::read_part_definitions(&source).unwrap();
    let read = pmi::read(&Document::parse(source.clone()).unwrap(), &defs).unwrap();
    let flatness: Vec<_> = read.parts[0]
        .tolerances
        .iter()
        .filter(|t| t.kind == pmi::ToleranceKind::Flatness)
        .map(|t| pmi::GeometricTolerance {
            // Specify's provenance marker, which another tool does not write.
            description: None,
            ..t.clone()
        })
        .collect();
    assert_eq!(flatness.len(), 1);
    let added = pmi::PartPmi {
        features: read.parts[0].features.clone(),
        tolerances: flatness,
        ..Default::default()
    };
    let doc = Document::parse(bytes).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(pmi::PartId(0), added)],
        pmi::write::Mode::Add,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    doc.apply(&edit).unwrap().bytes
}

fn flatness_on_a(existing: &Value) -> Vec<&Value> {
    existing
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"] == "Flatness" && e["faces"] == json!([2]))
        .collect()
}

#[test]
fn foreign_flatness_withholds_stored_none() {
    let (all, bytes) = accepted_all();
    assert_eq!(all["datum.A.flatness"], "none");
    let mut part = load(with_foreign_flatness(bytes));
    for _ in 0..2 {
        let analysis = api::analyse(&part).unwrap();
        // Existing foreign flatness already answers the question under #204.
        assert_withheld(&analysis, "datum.A.flatness", false);
        // The other silent answers, which nothing in the file contradicts, still resume.
        let answers = analysis["resume"]["answers"].as_object().unwrap();
        for id in SILENT.iter().filter(|id| **id != "datum.A.flatness") {
            assert!(answers.contains_key(*id), "{id}: {}", analysis["resume"]);
        }
        let flatness = flatness_on_a(&analysis["existing"]);
        assert_eq!(flatness.len(), 1, "{}", analysis["existing"]);
        assert_eq!(flatness[0]["value"], 0.05);
        assert!(flatness[0].get("question").is_none());
        (part, _) = saved_again(&part, &analysis);
        assert!(
            specify_core::existing::read_existing(&part)
                .unwrap()
                .iter()
                .any(|e| { e.r#type == "Flatness" && e.faces == [2] && e.value == Some(0.05) })
        );
    }
}

/// An answer that wrote PMI whose PMI is not in the file: the deletion stands (ADR P11). Its
/// feature-and-kind is a gap, with no conflict, and the answer is not mistaken for one that
/// never wrote anything (it is not resumed). The writer no longer stores such an answer, so
/// there is nothing to mention; `tests/feature_table.rs` reopens a 0.2 file that did.
#[test]
fn an_answer_whose_pmi_was_removed_is_a_gap_again() {
    let mut given = material();
    given.insert("datum.A.flatness".into(), json!("0.05"));
    let mut wrote = given.clone();
    wrote.insert("datum.A.flatness".into(), json!("none"));
    // Written without the flatness while storing it: the file removed it.
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let intent = api::apply(&analysis, &wrote, true).unwrap();
    let mut stored = specify_core::rules::with_all_defaults(&analysis, &given).unwrap();
    stored.insert("datum.A.flatness".into(), json!("0.05"));
    let bytes = api::write_bytes(
        std::fs::read(plate()).unwrap(),
        &[(intent, Some(stored.clone()))],
    )
    .unwrap()
    .0;
    let (again, warnings) = api::analyse_reporting(&load(bytes)).unwrap();
    assert!(
        again["resume"].get("conflicts").is_none(),
        "{}",
        again["resume"]
    );
    assert!(again["resume"]["answers"].get("datum.A.flatness").is_none());
    let row = again["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "datum.A.flatness")
        .unwrap();
    assert_eq!(row["state"], "gap", "{row}");
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.contains("datum.A.flatness"))
            .count(),
        0,
        "{warnings:?}"
    );
    // The silent answers stored beside it resume.
    let answers = again["resume"]["answers"].as_object().unwrap();
    for id in SILENT.iter().filter(|id| **id != "datum.A.flatness") {
        assert!(answers.contains_key(*id), "{id}: {}", again["resume"]);
    }
}

/// ADR P11 rule 2: an approval the plan writes nothing for (the pin's ⌀10 left to the general
/// tolerance: its surface is datum A, and no size is written on a datum's own faces) is stored
/// as decided. Saved as an edit list (piece 6) on the pin, part 1 of the assembly: reopened,
/// its row is decided, not a gap, and a second save with every default keeps it.
#[test]
fn an_approval_that_writes_nothing_stays_decided() {
    let dir = scratch("writes-nothing");
    let input = dir.join("assembly.step");
    std::fs::copy(
        common::fixtures().join("parts").join("assembly.step"),
        &input,
    )
    .unwrap();
    let id = "diameter.fit:turned_steps:0";
    let shared = session_of(&input);
    let pin = map(json!({"part.material": "steel", (id): "general"}));
    let analysis = api::analyse_file(&input, Some(1)).unwrap();
    let mut fill = pin.clone();
    fill.extend(offered(&analysis, &pin));
    let list = specify_core::edits::parse_value(json!({ "fill": fill })).unwrap();
    let saved = session::save_part_edits(&shared, Some(1), None, &list, "page")
        .unwrap_or_else(|e| panic!("{e}"));
    let report = saved.report.to_string();
    assert!(
        report.contains(&format!(
            "answer {id} ('general'): approved, but nothing of its kind was written on its feature"
        )),
        "{report}"
    );
    let again = api::analyse_file(Path::new(&saved.path), Some(1)).unwrap();
    let row = again["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap();
    assert_eq!(
        (&row["state"], &row["value"]),
        (&json!("decided_none"), &json!("general")),
        "{row}"
    );
    let answers = again["resume"]["answers"].as_object().unwrap();
    assert_eq!(
        answers.get(id),
        Some(&json!("general")),
        "{}",
        again["resume"]
    );
    // Saved again with every default the reopened file offers: it stays decided.
    let shared = session_of(Path::new(&saved.path));
    let more = offered(&again, answers);
    let list = specify_core::edits::parse_value(json!({ "fill": more })).unwrap();
    let twice = session::save_part_edits(&shared, Some(1), None, &list, "page")
        .unwrap_or_else(|e| panic!("{e}"));
    let third = api::analyse_file(Path::new(&twice.path), Some(1)).unwrap();
    assert_eq!(third["resume"]["answers"].get(id), Some(&json!("general")));
}

/// The same round trip on the command line, step by step: `write --accept-defaults`, then on
/// the saved file `questions --open` does not list the decided approval, and a plain `write`
/// (no defaults, no answers) is not refused as incomplete.
#[test]
fn the_command_line_keeps_an_approval_that_writes_nothing_decided() {
    let dir = scratch("writes-nothing-cli");
    let run = |args: &[&std::ffi::OsStr]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_specify-core"))
            .args(args)
            .output()
            .unwrap();
        (
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    let path = |name: &str| dir.join(name).into_os_string();
    std::fs::copy(
        common::fixtures().join("parts/pin.step"),
        dir.join("pin.step"),
    )
    .unwrap();
    let analysed = api::analyse_file(&dir.join("pin.step"), None).unwrap();
    std::fs::write(
        dir.join("steel.json"),
        common::approve_edits(&analysed, &material()).to_string(),
    )
    .unwrap();
    std::fs::write(dir.join("none.json"), "{}").unwrap();
    let os = |s: &str| std::ffi::OsString::from(s);
    let (ok, _, err) = run(&[
        &os("analyse"),
        &path("pin.step"),
        &os("-o"),
        &path("a.json"),
    ]);
    assert!(ok, "{err}");
    let (ok, out, err) = run(&[
        &os("write"),
        &path("pin.step"),
        &path("steel.json"),
        &os("-o"),
        &path("saved.step"),
    ]);
    assert!(ok, "{err}");
    assert!(
        out.contains("diameter.fit:turned_steps:0"),
        "reported: {out}"
    );
    let (ok, _, err) = run(&[
        &os("analyse"),
        &path("saved.step"),
        &os("-o"),
        &path("b.json"),
    ]);
    assert!(ok, "{err}");
    let (ok, out, err) = run(&[&os("questions"), &path("b.json"), &os("--open")]);
    assert!(ok, "{err}");
    assert!(!out.contains("diameter.fit:turned_steps:0"), "{out}");
    let (ok, _, err) = run(&[
        &os("write"),
        &path("saved.step"),
        &path("none.json"),
        &os("-o"),
        &path("again.step"),
    ]);
    assert!(ok, "a plain write of the saved file: {err}");
    let again = api::analyse_file(&dir.join("again.step"), None).unwrap();
    assert_eq!(
        again["resume"]["answers"]["diameter.fit:turned_steps:0"],
        json!("general")
    );
}

/// `bytes` with `added` written by haecceity's own writer (Add): another tool's PMI, without
/// Specify's provenance marker.
fn with_foreign(bytes: Vec<u8>, added: haecceity::pmi::PartPmi) -> Vec<u8> {
    use haecceity::p21::Document;
    use haecceity::pmi;
    let doc = Document::parse(bytes).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(pmi::PartId(0), added)],
        pmi::write::Mode::Add,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    doc.apply(&edit).unwrap().bytes
}

/// The plate written with every default approved: `part.notes: []` and the other silent
/// answers stored.
fn accepted_all() -> (Map<String, Value>, Vec<u8>) {
    let all = specify_core::rules::with_all_defaults(
        &api::analyse_file(&plate(), None).unwrap(),
        &material(),
    )
    .unwrap();
    let bytes = written(&all, &all);
    (all, bytes)
}

#[test]
fn silent_answers_resume_on_clean_plate_and_enclosure_faces() {
    for path in [
        plate(),
        common::fixtures().join("parts/electronics-enclosure-base.step.gz"),
    ] {
        let part = specify_core::part::load(&path, None).unwrap();
        let analysis = api::analyse(&part).unwrap();
        let mut answers = material();
        let hole = api::questions(&analysis, &answers)
            .unwrap()
            .into_iter()
            .find(|q| q.id.starts_with("hole.function:"))
            .expect("the clean part has a hole function question");
        answers.insert(hole.id.clone(), json!("general"));
        answers.insert("part.coating".into(), json!("None (as machined)"));
        answers.insert("part.heat_treatment".into(), json!("None"));
        answers.insert("part.notes".into(), json!([]));
        let intent = api::preview(&analysis, &answers).unwrap();
        let (bytes, _) =
            api::write_bytes(part.bytes.to_vec(), &[(intent, Some(answers.clone()))]).unwrap();
        let back = load_bytes("clean.step", bytes).unwrap();
        assert!(
            specify_core::existing::read_existing(&back)
                .unwrap()
                .is_empty()
        );
        let again = api::analyse(&back).unwrap();
        // ADR P11: what leaves no PMI behind is stored. The hole's `general`, written by
        // preview (which plans no general dimension), wrote nothing: rule 2 stores it, listed
        // as such, so the hole stays decided and is not asked again.
        let mut stored = specify_core::feature_table::storable(&answers);
        stored.insert(hole.id.clone(), json!("general"));
        stored.insert("_specify".into(), json!({"no_pmi": [hole.id]}));
        assert_eq!(again["resume"]["answers"], json!(stored));
        assert!(again["resume"].get("conflicts").is_none());
        let open = asked(&again).1;
        for id in answers.keys() {
            assert!(!open.contains(id), "{id}");
        }
    }
}

fn assert_withheld(analysis: &Value, id: &str, open: bool) {
    let resume = &analysis["resume"];
    assert!(resume["answers"].get(id).is_none(), "{id}: {resume}");
    assert!(
        !resume["conflicts"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|c| c["id"] == id),
        "{id}: {resume}"
    );
    assert_eq!(
        asked(analysis).1.iter().any(|q| q == id),
        open,
        "{id}: {resume}"
    );
}

/// Reopen and save as the editor does, leaving newly open questions unanswered.
fn saved_again(loaded: &LoadedPart, analysis: &Value) -> (LoadedPart, Value) {
    let answers = analysis["resume"]["answers"].as_object().unwrap().clone();
    let intent =
        api::preview(analysis, &answers).unwrap_or_else(|e| panic!("{e}\n{}", analysis["resume"]));
    let back = load(
        api::write_bytes(loaded.bytes.to_vec(), &[(intent, Some(answers))])
            .unwrap()
            .0,
    );
    let again = api::analyse(&back).unwrap();
    (back, again)
}

/// Locate the nominal measure through the dimension's own STEP provenance.
fn location_measure(part: &LoadedPart, mm: f64, basic: bool) -> Option<std::ops::Range<usize>> {
    use haecceity::p21::Document;
    use haecceity::pmi::{self, DimTolerance, DimensionKind, Quantity};
    let doc = Document::parse(part.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&part.bytes).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let (i, _) = read.parts[0].dimensions.iter().enumerate().find(|(_, d)| {
        matches!(d.kind, DimensionKind::Location { .. })
            && d.tolerance == if basic { DimTolerance::Basic } else { DimTolerance::None }
            && matches!(d.nominal.as_ref().map(|v| &v.quantity), Some(Quantity::Length(n)) if n.mm() == mm)
    })?;
    read.provenance.parts[0].dimensions[i]
        .iter()
        .find_map(|&id| {
            let span = doc.span(id)?;
            let record = std::str::from_utf8(&part.bytes[span.clone()]).unwrap();
            (record.contains("MEASURE_WITH_UNIT") && record.contains("'nominal value'"))
                .then_some(span)
        })
}

#[test]
fn edited_marked_basic_location_survives_two_saves() {
    edited_location_survives_two_saves(95, 97, true, "hole.position:holes:12");
}

#[test]
fn edited_marked_nominal_location_survives_two_saves() {
    edited_location_survives_two_saves(50, 52, false, "hole.function:holes:9");
}

fn edited_location_survives_two_saves(old: i32, new: i32, basic: bool, question: &str) {
    let (mut stored, _) = accepted_all();
    let answers = stored.clone();
    if basic {
        stored.insert(question.into(), json!("none"));
    }
    let source = load(written(&answers, &stored));
    let span = location_measure(&source, f64::from(old), basic).unwrap();
    let mut text = String::from_utf8(source.bytes.to_vec()).unwrap();
    let record = text[span.clone()].replace(
        &format!("LENGTH_MEASURE({old}.)"),
        &format!("LENGTH_MEASURE({new}.)"),
    );
    assert_ne!(text[span.clone()], record);
    text.replace_range(span, &record);
    let mut part = load(text.into_bytes());
    assert!(location_measure(&part, f64::from(new), basic).is_some());
    for _ in 0..2 {
        let analysis = api::analyse(&part).unwrap();
        // The basic case has file position PMI. In the nominal case the file states the
        // hole's size under the general tolerance (ADR P11: whoever wrote it).
        assert_withheld(&analysis, question, false);
        (part, _) = saved_again(&part, &analysis);
        assert!(
            location_measure(&part, f64::from(new), basic).is_some(),
            "lost {new} mm on saving {question}"
        );
    }
}

#[test]
fn flatness_on_the_files_moved_datum_survives_two_saves() {
    let (stored, _) = accepted_all();
    assert_eq!(stored["datum.A"], json!([2]));
    assert_eq!(stored["datum.A.flatness"], "none");
    let mut answers = stored.clone();
    // general dimensions are rows (Paul, 2026-11-21). Accept their suggestions with
    // datum A in its final position, as the measured spans can change when it moves.
    answers.retain(|id, _| !id.starts_with("length:planes:") && !id.starts_with("location:holes:"));
    answers.insert("datum.A".into(), json!([4]));
    answers.insert("datum.A.flatness".into(), json!("0.05"));
    let mut part = load(written(&answers, &stored));
    let has_flatness = |part: &LoadedPart| {
        specify_core::existing::read_existing(part)
            .unwrap()
            .iter()
            .any(|e| e.r#type == "Flatness" && e.faces == [4] && e.value == Some(0.05))
    };
    assert!(has_flatness(&part));
    for _ in 0..2 {
        let analysis = api::analyse(&part).unwrap();
        assert_withheld(&analysis, "datum.A.flatness", false);
        (part, _) = saved_again(&part, &analysis);
        assert!(has_flatness(&part), "lost flatness on the file's datum");
    }
}

#[test]
fn an_extra_marked_note_survives_two_saves() {
    let (mut stored, _) = accepted_all();
    stored.insert("part.notes".into(), json!(["KEEP DRY"]));
    let mut answers = stored.clone();
    let notes = json!(["KEEP DRY", "DEBURR ALL OVER"]);
    answers.insert("part.notes".into(), notes.clone());
    let mut part = load(written(&answers, &stored));
    assert_eq!(
        specify_core::existing::part_settings_provenance(&part).unwrap()["notes"],
        json!(["specify-core:written:v1", "specify-core:written:v1"])
    );
    for _ in 0..2 {
        let analysis = api::analyse(&part).unwrap();
        assert_withheld(&analysis, "part.notes", false);
        (part, _) = saved_again(&part, &analysis);
        assert_eq!(
            specify_core::existing::part_settings(&part).unwrap()["notes"],
            notes
        );
    }
}

#[test]
fn a_marked_position_tolerance_contradicts_stored_none() {
    let hole = "hole.position:holes:12";
    let (mut answers, _) = accepted_all();
    answers.insert(hole.into(), json!("0.2"));
    let mut stored = answers.clone();
    stored.insert(hole.into(), json!("none"));
    let edited = load(written(&answers, &stored));
    let positions = |part: &LoadedPart| {
        specify_core::existing::read_existing(part)
            .unwrap()
            .into_iter()
            .filter(|e| e.r#type == "Position" && e.faces == [12])
            .map(|e| (e.value.unwrap(), e.provenance.is_some()))
            .collect::<Vec<_>>()
    };
    assert_eq!(positions(&edited), [(0.2, true)]);

    let analysis = api::analyse(&edited).unwrap();
    let resume = &analysis["resume"];
    let (back, again) = saved_again(&edited, &analysis);
    assert_eq!(positions(&back), [(0.2, true)], "{resume}");
    // The marked position answers the question as file PMI (ADR P11).
    assert_withheld(&analysis, hole, false);
    assert!(stated(&analysis, hole), "{resume}");
    let (back, _) = saved_again(&back, &again);
    assert_eq!(positions(&back), [(0.2, true)]);
}

#[test]
fn a_marked_note_contradicts_stored_no_notes() {
    let (stored, _) = accepted_all();
    let notes = json!(["KEEP DRY"]);
    let mut answers = stored.clone();
    answers.insert("part.notes".into(), notes.clone());
    let edited = load(written(&answers, &stored));
    let analysis = api::analyse(&edited).unwrap();
    // The file's complete notes answer the setting under #204.
    assert_withheld(&analysis, "part.notes", false);
    let (back, again) = saved_again(&edited, &analysis);
    assert_eq!(
        specify_core::existing::part_settings(&back).unwrap()["notes"],
        notes
    );
    assert_eq!(again["part"]["notes"], notes);
    let (back, _) = saved_again(&back, &again);
    assert_eq!(
        specify_core::existing::part_settings(&back).unwrap()["notes"],
        notes
    );
}

#[test]
fn mixed_provenance_notes_contradict_the_stored_list() {
    let (mut stored, _) = accepted_all();
    assert_eq!(stored["part.notes"], json!([]));
    let mut answers = stored.clone();
    answers.insert("part.notes".into(), json!(["KEEP DRY"]));
    let notes = json!(["KEEP DRY", "DEBURR ALL OVER"]);
    for (old, index) in [(json!([]), 2), (json!(["KEEP DRY"]), 1)] {
        stored.insert("part.notes".into(), old.clone());
        let edited = load(with_general_note(
            written(&answers, &stored),
            "DEBURR ALL OVER",
            index,
        ));
        assert_eq!(
            specify_core::existing::part_settings(&edited).unwrap()["notes"],
            notes
        );
        let provenance = specify_core::existing::part_settings_provenance(&edited).unwrap();
        assert!(provenance["notes"][0].is_string());
        assert!(provenance["notes"][1].is_null());

        let analysis = api::analyse(&edited).unwrap();
        let resume = &analysis["resume"];
        let (back, again) = saved_again(&edited, &analysis);
        assert_eq!(
            specify_core::existing::part_settings(&back).unwrap()["notes"],
            notes,
            "{resume}"
        );
        assert_eq!(analysis["part"]["notes"], notes);
        // Both notes are file entries, so the setting is already answered under #204.
        assert_withheld(&analysis, "part.notes", false);
        assert_eq!(again["part"]["notes"], notes, "{}", again["resume"]);
        let (back, _) = saved_again(&back, &again);
        assert_eq!(
            specify_core::existing::part_settings(&back).unwrap()["notes"],
            notes
        );
        assert_eq!(
            specify_core::existing::part_settings_provenance(&back).unwrap()["notes"],
            provenance["notes"]
        );
    }
}

fn with_general_note(bytes: Vec<u8>, text: &str, index: i64) -> Vec<u8> {
    use haecceity::pmi::{AttributeSet, AttributeValue, PartPmi};
    let note = AttributeSet {
        name: "semantic text".into(),
        on: None,
        items: vec![
            ("general note".into(), AttributeValue::Text(text.into())),
            ("note index".into(), AttributeValue::Integer(index)),
        ],
        description: None,
        general_property: None,
    };
    with_foreign(
        bytes,
        PartPmi {
            attributes: vec![note],
            ..Default::default()
        },
    )
}

/// "No notes" stored, then another tool adds a general note: the file's note contradicts the
/// stored `[]`. It is shown (`part.notes`), the answer is not resumed, and the note stays in
/// the file through a save.
#[test]
fn another_tools_general_note_contradicts_stored_no_notes() {
    let (all, bytes) = accepted_all();
    assert_eq!(all["part.notes"], json!([]));
    let edited = load(with_general_note(bytes, "DEBURR ALL OVER", 1));
    let analysis = api::analyse(&edited).unwrap();
    assert_eq!(
        analysis["part"]["notes"],
        json!(["DEBURR ALL OVER"]),
        "{}",
        analysis["part"]
    );
    // The imported note fills the part setting under #204.
    assert_withheld(&analysis, "part.notes", false);
    let (back, again) = saved_again(&edited, &analysis);
    assert_eq!(
        again["part"]["notes"],
        json!(["DEBURR ALL OVER"]),
        "{}",
        again["part"]
    );
    let (back, again) = saved_again(&back, &again);
    assert_eq!(
        again["part"]["notes"],
        json!(["DEBURR ALL OVER"]),
        "{}",
        again["part"]
    );
    // A session save of it keeps the note too.
    let dir = scratch("note");
    let input = dir.join("noted.step");
    std::fs::write(&input, back.bytes.as_slice()).unwrap();
    let shared = session_of(&input);
    let saved = session::save_edits(&shared, None, &Default::default(), "page")
        .unwrap_or_else(|e| panic!("{e}"));
    let reopened = api::analyse_file(Path::new(&saved.path), None).unwrap();
    assert_eq!(
        reopened["part"]["notes"],
        json!(["DEBURR ALL OVER"]),
        "{}",
        reopened["part"]
    );
}

/// A "general" hole (no PMI), then another tool adds a thread callout on it: a requirement of
/// another kind than "general"'s alternatives would write, on the question's faces, still
/// contradicts the stored answer.
#[test]
fn another_tools_thread_on_a_general_hole_contradicts_it() {
    use haecceity::p21::Document;
    use haecceity::pmi::{self, PartPmi};
    let (all, bytes) = accepted_all();
    assert_eq!(all["hole.function:holes:9"], "general");
    // A file stating hole 9's size (to name its feature) and hole 12's M5 thread.
    let mut fitted = all.clone();
    fitted.insert("hole.function:holes:9".into(), json!("fit:H7"));
    let source = written(&fitted, &fitted);
    let defs = haecceity::step::read_part_definitions(&source).unwrap();
    let read = pmi::read(&Document::parse(source.clone()).unwrap(), &defs).unwrap();
    let listed = specify_core::existing::read_existing(&load(source.clone())).unwrap();
    let k = listed
        .iter()
        .filter(|e| e.kind == "dimension")
        .position(|e| e.r#type == "Size_Diameter" && e.faces == [9])
        .unwrap();
    let size = read.parts[0]
        .dimensions
        .iter()
        .filter(|d| !matches!(d.tolerance, pmi::DimTolerance::None))
        .nth(k)
        .unwrap();
    let pmi::DimensionKind::Size { feature, .. } = &size.kind else {
        panic!("not a size: {size:?}");
    };
    let mut thread = read.parts[0].threads[0].clone();
    thread.feature = *feature;
    thread.partial_area = Some(*feature);
    thread.runout = None;
    let edited = load(with_foreign(
        bytes,
        PartPmi {
            features: read.parts[0].features.clone(),
            threads: vec![thread],
            ..Default::default()
        },
    ));
    let threads: Vec<_> = specify_core::existing::read_existing(&edited)
        .unwrap()
        .into_iter()
        .filter(|e| e.r#type == "internal thread" && e.faces == [9])
        .collect();
    assert_eq!(threads.len(), 1, "{threads:?}");
    let mut part = edited;
    for _ in 0..2 {
        let analysis = api::analyse(&part).unwrap();
        // The imported thread already establishes the hole's function under #204.
        assert_withheld(&analysis, "hole.function:holes:9", false);
        (part, _) = saved_again(&part, &analysis);
        assert_eq!(
            specify_core::existing::read_existing(&part)
                .unwrap()
                .iter()
                .filter(|e| e.r#type == "internal thread" && e.faces == [9])
                .count(),
            1
        );
    }
}

/// Another part than the plate, with holes and datums: written with every default approved,
/// and with `write --accept-defaults`'s stored answers, it reopens with nothing asked again
/// and no conflict.
#[test]
fn another_part_resumes_its_silent_answers() {
    let input = common::fixtures().join("parts/electronics-enclosure-base.step.gz");
    let source = specify_core::part::load(&input, None).unwrap();
    let analysis = api::analyse_file(&input, None).unwrap();
    assert!(
        analysis["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["id"].as_str().is_some_and(|id| id.starts_with("holes")))
    );
    let given = material();
    let all = specify_core::rules::with_all_defaults(&analysis, &given).unwrap();
    assert!(all.keys().any(|id| id.starts_with("datum.")), "{all:?}");
    let intent = api::apply(&analysis, &given, true).unwrap();
    let cli = api::stored_with_defaults(&source, &analysis, &given).unwrap();
    assert!(cli.len() > given.len(), "{cli:?}");
    for stored in [&all, &cli] {
        let bytes = api::write_bytes(
            source.bytes.to_vec(),
            &[(intent.clone(), Some(stored.clone()))],
        )
        .unwrap()
        .0;
        let back = load_bytes("enclosure.step", bytes).unwrap();
        let again = api::analyse(&back).unwrap();
        let resume = &again["resume"];
        assert!(resume.get("conflicts").is_none(), "{resume}");
        resumed_or_stated(&again, stored.keys());
        let (_, open) = asked(&again);
        assert!(open.is_empty(), "asked again: {open:?}");
    }
}

/// `bytes` with Specify's process UDA changed to cast by another tool: the item edited and
/// the provenance marker on its property link removed.
fn with_foreign_cast_process(bytes: Vec<u8>) -> Vec<u8> {
    use haecceity::p21::{Attribute, Document, Edit, RawEntity};
    use haecceity::pmi;
    use specify_core::process;
    let item = |value| {
        format!(
            "DESCRIPTIVE_REPRESENTATION_ITEM('{}','{value}')",
            process::ITEM
        )
    };
    let original = String::from_utf8(bytes).unwrap();
    assert_eq!(original.matches(&item(process::MACHINED)).count(), 1);
    let doc = Document::parse(
        original
            .replace(&item(process::MACHINED), &item(process::CAST))
            .into_bytes(),
    )
    .unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let i = read.parts[0]
        .attributes
        .iter()
        .position(|set| set.name == process::PROPERTY)
        .unwrap();
    let markers: Vec<_> = read.provenance.parts[0].attributes[i]
        .iter()
        .flat_map(|&id| doc.referrers(id))
        .copied()
        .filter(|&id| {
            matches!(doc.get(id), Some(RawEntity::Simple { name, attributes, .. })
                if name == "DESCRIPTION_ATTRIBUTE"
                    && matches!(attributes.first(),
                        Some(Attribute::String(s)) if s == "specify-core:written:v1"))
        })
        .collect();
    assert_eq!(markers.len(), 1);
    let mut edit = Edit::new();
    edit.remove(markers[0]);
    doc.apply(&edit).unwrap().bytes
}

/// The file's process and its process UDAs.
fn file_process(part: &LoadedPart) -> (usize, Value) {
    use haecceity::p21::Document;
    use haecceity::pmi;
    use specify_core::process;
    let doc = Document::parse(part.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let sets = read.parts[0]
        .attributes
        .iter()
        .filter(|set| set.name == process::PROPERTY)
        .count();
    (sets, process::read_pmi(&read.parts[0]).unwrap().value)
}

/// Another tool changed the process (#208) and added a flatness on datum A against the stored
/// "none" (no-PMI answers). Two reopen-and-save rounds keep both: the file states them (ADR
/// P11), with no conflict, and the stored "none" is dropped.
#[test]
fn foreign_process_and_flatness_survive_two_saves() {
    use specify_core::process;
    let (mut all, _) = accepted_all();
    assert_eq!(all["datum.A.flatness"], "none");
    all.insert(process::QUESTION.into(), json!(process::MACHINED));
    let bytes = written(&all, &all);
    let mut part = load(with_foreign_cast_process(with_foreign_flatness(bytes)));
    assert_eq!(file_process(&part), (1, json!(process::CAST)));
    for round in 0..2 {
        let analysis = api::analyse(&part).unwrap();
        // ADR P11: the file's process is simply the file's: no conflict, in any round.
        assert!(
            analysis["resume"].get("conflicts").is_none(),
            "{}",
            analysis["resume"]
        );
        assert!(
            analysis["resume"]["answers"]
                .get(process::QUESTION)
                .is_none()
        );
        assert_eq!(analysis["part"]["process"], process::CAST);
        assert_eq!(analysis["process"]["source"], "file");
        assert_withheld(&analysis, "datum.A.flatness", false);
        let flatness = flatness_on_a(&analysis["existing"]);
        assert_eq!(flatness.len(), 1, "{}", analysis["existing"]);
        assert_eq!(flatness[0]["value"], 0.05);
        (part, _) = saved_again(&part, &analysis);
        assert_eq!(
            file_process(&part),
            (1, json!(process::CAST)),
            "round {round}"
        );
        assert!(
            specify_core::existing::read_existing(&part)
                .unwrap()
                .iter()
                .any(|e| e.r#type == "Flatness" && e.faces == [2] && e.value == Some(0.05)),
            "round {round}: lost the flatness"
        );
        // ADR P11: the file's flatness states the question, so the stored "none" is not read
        // back. (It stays in the payload until a save from the table drops it: piece 2.)
        let again = api::analyse(&part).unwrap();
        assert!(again["resume"]["answers"].get("datum.A.flatness").is_none());
    }
}
