//! The feature table (Specify 0.3.0, ADR P11): read from the file alone, per feature and kind.

mod common;

use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::part::{LoadedPart, load, load_bytes};
use specify_core::resume;

fn map(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn plate() -> LoadedPart {
    load(&common::fixtures().join("parts/plate.step"), None).unwrap()
}

fn fixture(name: &str) -> LoadedPart {
    load(&common::fixtures().join(name), None).unwrap()
}

fn table(loaded: &LoadedPart) -> (Value, Vec<String>) {
    let (analysis, warnings) = api::analyse_reporting(loaded).unwrap();
    (analysis["table"].clone(), warnings)
}

fn row<'a>(table: &'a Value, id: &str) -> &'a Value {
    table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no row {id} in {table}"))
}

/// The rows as (id, state, value), and the file PMI's notation: what the table says,
/// without characteristic numbers.
fn shape(table: &Value) -> (Vec<(String, String, Value)>, Vec<String>) {
    let rows = table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap().to_string(),
                r["state"].as_str().unwrap().to_string(),
                r["value"].clone(),
            )
        })
        .collect();
    let file = table["file_pmi"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["text"].as_str().unwrap().to_string())
        .collect();
    (rows, file)
}

fn write(loaded: &LoadedPart, answers: &Map<String, Value>, stored: bool) -> Vec<u8> {
    let analysis = api::analyse(loaded).unwrap();
    let intent = api::apply(&analysis, answers, true).unwrap();
    api::write_bytes(
        loaded.bytes.to_vec(),
        &[(intent, stored.then(|| answers.clone()))],
    )
    .unwrap()
    .0
}

/// `bytes` with every requirement copied by haecceity's own writer onto the bare plate: the
/// same PMI, written by another tool (no Specify marker, no stored answers, no numbers).
fn as_another_tool(bytes: &[u8]) -> Vec<u8> {
    use haecceity::p21::Document;
    use haecceity::pmi;
    let defs = haecceity::step::read_part_definitions(bytes).unwrap();
    let read = pmi::read(&Document::parse(bytes.to_vec()).unwrap(), &defs).unwrap();
    let mut copied = read.parts[0].clone();
    for t in &mut copied.tolerances {
        t.description = None;
    }
    copied.attributes.retain(|set| {
        !resume::is_answers(set) && set.name != specify_core::characteristics::PROPERTY
    });
    let bare = plate().bytes.to_vec();
    let doc = Document::parse(bare).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(pmi::PartId(0), copied)],
        pmi::write::Mode::Replace,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    doc.apply(&edit).unwrap().bytes
}

#[test]
fn another_tools_requirement_and_specifys_give_the_same_table() {
    let answers = map(json!({
        "part.material": "6082-T6",
        "hole.position:holes:12": "0.1",
        "datum.A.flatness": "0.05",
    }));
    let specify = write(&plate(), &answers, false);
    let other = as_another_tool(&specify);
    let (mine, _) = table(&load_bytes("plate.step", specify).unwrap());
    let (theirs, _) = table(&load_bytes("plate.step", other).unwrap());
    assert_eq!(shape(&mine), shape(&theirs));
    // The answers' requirements are stated in both, whoever wrote them.
    for t in [&mine, &theirs] {
        for id in ["hole.position:holes:12", "datum.A.flatness", "datum.A"] {
            assert_eq!(row(t, id)["state"], "stated", "{id}: {t}");
        }
        assert!(
            row(t, "hole.position:holes:12")["value"]
                .as_str()
                .unwrap()
                .contains("Position 0.1"),
            "{t}"
        );
    }
}

#[test]
fn a_gap_whose_suggestion_is_none_is_not_open() {
    let (t, _) = table(&plate());
    // Every feature-and-kind of a bare part is a gap.
    assert!(
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["state"] == "gap"),
        "{t}"
    );
    for id in [
        "datum.A.flatness",
        "part.coating",
        "part.heat_treatment",
        "part.notes",
    ] {
        let r = row(&t, id);
        assert_eq!(r["open"], false, "{r}");
    }
    assert_eq!(row(&t, "datum.A.flatness")["suggestion"], "none");
    // A gap with a suggestion that writes a requirement is open, and so is one with none at
    // all (the material).
    assert_eq!(row(&t, "hole.position:holes:12")["open"], true);
    assert_eq!(row(&t, "hole.position:holes:12")["suggestion"], "0.25");
    assert_eq!(row(&t, "part.material")["open"], true);
    assert_eq!(row(&t, "part.material")["suggestion"], Value::Null);
    assert!(t["decided_none"].as_array().unwrap().is_empty());
    assert!(t["file_pmi"].as_array().unwrap().is_empty());
}

#[test]
fn decided_none_is_remembered() {
    let answers = map(json!({
        "part.material": "6082-T6",
        "datum.A.flatness": "none",
        "part.coating": "None (as machined)",
        "hole.position:holes:12": "0.1",
    }));
    let bytes = write(&plate(), &answers, true);
    let (t, _) = table(&load_bytes("plate.step", bytes.clone()).unwrap());
    for id in ["datum.A.flatness", "part.coating"] {
        assert_eq!(row(&t, id)["state"], "decided_none", "{id}: {t}");
        assert_eq!(row(&t, id)["open"], false);
    }
    let decided = t["decided_none"].as_array().unwrap();
    assert!(decided.contains(&json!("datum.A.flatness")), "{t}");
    assert!(decided.contains(&json!("part.coating")), "{t}");
    // Stated requirements are not stored: the file states them.
    let stored = &resume::stored(&bytes)[&0];
    let saved = stored["answers"].as_object().unwrap();
    assert_eq!(saved["datum.A.flatness"], "none");
    assert!(!saved.contains_key("hole.position:holes:12"), "{saved:?}");
    assert!(!saved.contains_key("part.material"), "{saved:?}");
    assert_eq!(row(&t, "hole.position:holes:12")["state"], "stated");
}

#[test]
fn a_0_2_files_repeats_are_ignored_on_read_and_dropped_at_save() {
    // Specify 0.2 stored every answer beside the PMI that states it.
    let old = fixture("revisions/spindle_pmi.old.step.gz");
    let stored = &resume::stored(&old.bytes)[&0]["answers"];
    assert_eq!(stored["hole.position:holes:14"], "0.3", "{stored}");
    let (analysis, warnings) = api::analyse_reporting(&old).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let resumed = analysis["resume"]["answers"].as_object().unwrap();
    // Read back: only what leaves no PMI behind.
    for id in resumed.keys() {
        assert!(
            id == "part.locating" || specify_core::feature_table::is_none_value(&resumed[id]),
            "{id} repeats the file's PMI: {resumed:?}"
        );
    }
    let before = analysis["table"].clone();
    assert_eq!(row(&before, "hole.position:holes:14")["state"], "stated");
    assert!(
        row(&before, "hole.position:holes:14")["value"]
            .as_str()
            .unwrap()
            .contains("Position 0.3")
    );
    // A save with the session's answers drops the repeats from the file, and changes no
    // requirement.
    let intent = api::apply(&analysis, resumed, false).unwrap();
    let (saved, _) =
        api::write_bytes(old.bytes.to_vec(), &[(intent, Some(resumed.clone()))]).unwrap();
    let now = &resume::stored(&saved)[&0]["answers"];
    assert_eq!(now, &Value::Object(resumed.clone()));
    let reopened = load_bytes("spindle.step", saved).unwrap();
    let (after, warnings) = table(&reopened);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(shape(&before), shape(&after));
    assert_eq!(before["decided_none"], after["decided_none"]);
    let numbers = |t: &Value| -> Vec<Value> {
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| r["requirements"].as_array().cloned().unwrap_or_default())
            .map(|q| q["number"].clone())
            .collect()
    };
    assert_eq!(numbers(&before), numbers(&after));
}

#[test]
fn a_stored_answer_whose_pmi_was_deleted_is_a_gap_again_mentioned_once() {
    // The edited plate stores flatness 0.05 on datum A; the file no longer has it.
    let edited = fixture("resume-p9/edited-plate.step.gz");
    let (analysis, warnings) = api::analyse_reporting(&edited).unwrap();
    let t = &analysis["table"];
    assert_eq!(row(t, "datum.A.flatness")["state"], "gap");
    let mentions: Vec<_> = warnings
        .iter()
        .filter(|w| w.contains("datum.A.flatness"))
        .collect();
    assert_eq!(mentions.len(), 1, "{warnings:?}");
    // The stored position 0.1 the file changed to 0.2: the file's value is shown, no
    // conflict.
    assert_eq!(row(t, "hole.position:holes:12")["state"], "stated");
    assert!(
        row(t, "hole.position:holes:12")["value"]
            .as_str()
            .unwrap()
            .contains("Position 0.2")
    );
    assert!(analysis["resume"].get("conflicts").is_none());
    assert!(analysis["resume"].get("foreign").is_none());
    // The deletion stands: a save (the "none" suggestions taken, as the old interview
    // requires) does not bring it back, and the next opening says nothing.
    let answers = analysis["resume"]["answers"].as_object().unwrap().clone();
    let intent = api::apply(&analysis, &answers, true).unwrap();
    let (saved, _) = api::write_bytes(edited.bytes.to_vec(), &[(intent, Some(answers))]).unwrap();
    let (t, warnings) = table(&load_bytes("plate.step", saved).unwrap());
    assert_eq!(row(&t, "datum.A.flatness")["state"], "gap");
    assert!(
        !warnings.iter().any(|w| w.contains("datum.A.flatness")),
        "{warnings:?}"
    );
}

#[test]
fn pmi_on_no_recognised_feature_is_file_pmi() {
    let edited = fixture("resume-p9/edited-plate.step.gz");
    let (t, _) = table(&edited);
    // #5 is the plate's length, from face 0 to face 5: no feature's requirement.
    let file: Vec<&Value> = t["file_pmi"].as_array().unwrap().iter().collect();
    let length = file
        .iter()
        .find(|r| r["number"] == "#5")
        .unwrap_or_else(|| panic!("#5 is not file PMI: {t}"));
    assert_eq!(length["type"], "Location_LinearDistance");
    assert_eq!(length["faces"], json!([0, 5]));
    // No row claims it.
    for r in t["rows"].as_array().unwrap() {
        for q in r["requirements"].as_array().into_iter().flatten() {
            assert_ne!(q["number"], "#5", "{r}");
        }
    }
}

/// The ids `review` asks and counts open, for `analysis` with its resumed answers.
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

/// Questions and the table agree: nothing the file states is asked, and every row the table
/// counts open is asked and open.
fn agree(analysis: &Value) {
    let (questions, open) = asked(analysis);
    for r in analysis["table"]["rows"].as_array().unwrap() {
        let id = r["id"].as_str().unwrap().to_string();
        if r["state"] == "stated" {
            assert!(!questions.contains(&id), "{id} is stated but asked");
        }
        if r["open"] == true {
            assert!(
                open.contains(&id),
                "{id} is open in the table, not in questions"
            );
        }
    }
}

#[test]
fn a_hole_with_its_position_stated_is_still_asked_its_thread() {
    let plate = plate();
    let analysis = api::analyse(&plate).unwrap();
    let answers = map(json!({"part.material": "6082-T6"}));
    let mut intent = specify_core::rules::apply_with_defaults(&analysis, &answers, true).unwrap();
    // Only the hole's position is written: its thread (hole.function) is left out.
    intent
        .requirements
        .retain(|r| r.get("question") != Some(&json!("hole.function:holes:12")));
    let bytes = api::write_bytes(plate.bytes.to_vec(), &[(intent, Some(answers))])
        .unwrap()
        .0;
    let reopened = api::analyse(&load_bytes("plate.step", bytes).unwrap()).unwrap();
    let t = &reopened["table"];
    assert_eq!(row(t, "hole.position:holes:12")["state"], "stated");
    assert_eq!(row(t, "hole.function:holes:12")["state"], "gap");
    let (questions, open) = asked(&reopened);
    assert!(
        questions.contains(&"hole.function:holes:12".to_string()),
        "{questions:?}"
    );
    assert!(
        open.contains(&"hole.function:holes:12".to_string()),
        "{open:?}"
    );
    assert!(!questions.contains(&"hole.position:holes:12".to_string()));
    agree(&reopened);
}

#[test]
fn a_knurl_states_its_diameter_on_the_spindle() {
    let spindle = load(&common::fixtures().join("spindle.step"), None).unwrap();
    let (analysis, warnings) = api::analyse_reporting(&spindle).unwrap();
    let t = &analysis["table"];
    let knurled = row(t, "diameter.fit:turned_steps:9");
    assert_eq!(knurled["state"], "stated", "{knurled}");
    assert!(knurled["value"].as_str().unwrap().contains("knurl"));
    assert!(
        !t["file_pmi"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["type"] == "knurl"),
        "{t}"
    );
    assert!(
        !warnings.iter().any(|w| w.contains("no longer in the file")),
        "{warnings:?}"
    );
    // The thread the file states is not asked; the other hole is.
    let (questions, _) = asked(&analysis);
    assert!(!questions.contains(&"hole.function:holes:3.5".to_string()));
    assert!(questions.contains(&"hole.function:holes:7".to_string()));
    agree(&analysis);
}

#[test]
fn a_general_hole_writes_its_size_so_its_answer_is_not_stored() {
    let plate = plate();
    let analysis = api::analyse(&plate).unwrap();
    let answers = map(json!({"part.material": "6082-T6", "hole.function:holes:9": "general"}));
    let intent = specify_core::rules::apply_with_defaults(&analysis, &answers, true).unwrap();
    let bytes = api::write_bytes(
        plate.bytes.to_vec(),
        &[(intent.clone(), Some(answers.clone()))],
    )
    .unwrap()
    .0;
    let stored = &resume::stored(&bytes)[&0]["answers"];
    assert!(stored.get("hole.function:holes:9").is_none(), "{stored}");
    let (t, _) = table(&load_bytes("plate.step", bytes).unwrap());
    assert_eq!(row(&t, "hole.function:holes:9")["state"], "stated");
    // Its dimension deleted (the file written without it): a gap, not decided none.
    let mut without = intent;
    let on_nine = |r: &Map<String, Value>| r.get("faces") == Some(&json!([9]));
    without.general.retain(|r| !on_nine(r));
    without.requirements.retain(|r| !on_nine(r));
    let bytes = api::write_bytes(plate.bytes.to_vec(), &[(without, Some(answers))])
        .unwrap()
        .0;
    let (t, _) = table(&load_bytes("plate.step", bytes).unwrap());
    let hole = row(&t, "hole.function:holes:9");
    assert_eq!(hole["state"], "gap", "{hole}");
    assert!(
        !t["decided_none"]
            .as_array()
            .unwrap()
            .contains(&json!("hole.function:holes:9"))
    );
}

#[test]
fn a_direct_intent_measuring_a_stated_feature_otherwise_is_not_added() {
    let p = plate();
    let mut intent = specify_core::rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    intent.datums.push(map(json!({"letter":"A", "faces":[4]})));
    intent.requirements.push(map(
        json!({"kind":"location", "faces":[9], "reference":[4], "distance":20., "basic":true}),
    ));
    let first = api::write_bytes(p.bytes.to_vec(), &[(intent.clone(), None)])
        .unwrap()
        .0;
    let first = load_bytes("plate.step", first).unwrap();
    let locations = |l: &LoadedPart| {
        specify_core::existing::read_existing(l)
            .unwrap()
            .iter()
            .filter(|e| e.r#type.starts_with("Location_"))
            .count()
    };
    assert_eq!(locations(&first), 1);
    // The same feature-and-kind measured from another reference: the file's stands.
    let mut other = specify_core::rules::Intent::new(serde_json::to_value(&first.binding).unwrap());
    other.requirements.push(map(
        json!({"kind":"location", "faces":[9], "reference":[5], "distance":30., "basic":true}),
    ));
    let (bytes, report) = api::write_bytes(first.bytes.to_vec(), &[(other, None)]).unwrap();
    assert_eq!(locations(&load_bytes("plate.step", bytes).unwrap()), 1);
    assert!(
        report
            .not_written
            .iter()
            .any(|l| l.contains("the file already states it")),
        "{:?}",
        report.not_written
    );
}
