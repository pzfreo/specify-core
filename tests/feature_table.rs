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

/// Questions and the table agree: nothing the file states is asked, and a feature-and-kind is
/// open in `questions` exactly when the table counts it open (a gap whose suggestion writes
/// nothing is open in neither).
fn agree(analysis: &Value) {
    let (questions, open) = asked(analysis);
    let rows = analysis["table"]["rows"].as_array().unwrap();
    for r in rows {
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
    let mut rowless = Vec::new();
    for id in &open {
        match rows.iter().find(|r| r["id"] == *id) {
            Some(r) => assert_eq!(
                r["open"], true,
                "{id} is open in questions, not in the table: {r}"
            ),
            None => rowless.push(id.clone()),
        }
    }
    // The open questions with no row are exactly the open choices that are not
    // requirements (today `part.locating` alone, when it is open).
    let choices: Vec<_> = open
        .iter()
        .filter(|id| !specify_core::feature_table::is_requirement(id))
        .cloned()
        .collect();
    assert_eq!(rowless, choices, "open with no table row");
    assert!(
        choices.iter().all(|id| id == "part.locating"),
        "{choices:?}"
    );
}

#[test]
fn questions_and_the_table_agree_on_what_is_open() {
    for name in [
        "parts/plate.step",
        "spindle.step",
        "revisions/enclosure_pmi.old.step.gz",
    ] {
        let part = load(&common::fixtures().join(name), None).unwrap();
        agree(&api::analyse(&part).unwrap());
    }
    // The plate's "none" suggestions are gaps, not open, in both.
    let analysis = api::analyse(&plate()).unwrap();
    let (_, open) = asked(&analysis);
    // Its one open question with no row is the locating choice.
    assert!(open.contains(&"part.locating".to_string()), "{open:?}");
    assert!(
        analysis["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["id"] != "part.locating")
    );
    for id in ["datum.A.flatness", "part.coating", "part.notes"] {
        assert!(!open.contains(&id.to_string()), "{id}");
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
        !warnings
            .iter()
            .any(|w| w.contains("no requirement for this answer in the file")),
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

/// Position mode, one hole located by a ± location dimension and no position tolerance: the
/// file locates it, so it is stated (Paul, 2026-11-21: a hole is never located twice by two
/// schemes). It is not asked, and a save writes no position tolerance for it.
#[test]
fn a_hole_located_by_a_plus_minus_dimension_is_stated_in_position_mode() {
    let p = plate();
    let mut intent = specify_core::rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    for (letter, faces) in [("A", [2]), ("B", [1]), ("C", [0])] {
        intent
            .datums
            .push(map(json!({"letter": letter, "faces": faces})));
    }
    intent.requirements.push(map(json!({
        "kind": "location", "faces": [12], "reference": [0],
        "distance": 95., "upper": 0.1, "lower": -0.1,
    })));
    let bytes = api::write_bytes(p.bytes.to_vec(), &[(intent, None)])
        .unwrap()
        .0;
    let located = load_bytes("plate.step", bytes).unwrap();
    let analysis = api::analyse(&located).unwrap();
    let hole = "hole.position:holes:12";
    let r = row(&analysis["table"], hole);
    assert_eq!(r["state"], "stated", "{r}");
    assert!(
        r["value"]
            .as_str()
            .unwrap()
            .contains("Location_LinearDistance 95"),
        "{r}"
    );
    let (questions, _) = asked(&analysis);
    assert!(!questions.contains(&hole.to_string()), "{questions:?}");
    assert_eq!(analysis["part"].get("locating"), None);
    // A save with every default taken (position tolerances for holes that have nothing).
    let answers = map(json!({"part.material": "6082-T6", "part.locating": "position tolerances"}));
    let save = api::apply(&analysis, &answers, true).unwrap();
    assert!(
        !save
            .requirements
            .iter()
            .any(|r| r["kind"] == "position" && r["faces"] == json!([12])),
        "{:?}",
        save.requirements
    );
    let (saved, report) =
        api::write_bytes(located.bytes.to_vec(), &[(save, Some(answers))]).unwrap();
    let positions =
        specify_core::existing::read_existing(&load_bytes("plate.step", saved).unwrap())
            .unwrap()
            .into_iter()
            .filter(|e| e.r#type == "Position" && e.faces == [12])
            .count();
    assert_eq!(positions, 0, "{:?}", report.written);
}

/// A blind hole's depth, a location dimension to its floor, does not locate the hole: its
/// position row stays a gap (the hole's faces are its bore; the floor is not one of them).
#[test]
fn a_depth_dimension_to_a_hole_floor_does_not_locate_the_hole() {
    let shaft = load(&common::fixtures().join("parts/GRM-02_shaft.step"), None).unwrap();
    let mut intent =
        specify_core::rules::Intent::new(serde_json::to_value(&shaft.binding).unwrap());
    intent
        .datums
        .push(map(json!({"letter": "A", "faces": [0]})));
    // Hole 15's floor is face 16: a 3.5 mm depth from the end face 7.
    intent.requirements.push(map(json!({
        "kind": "location", "faces": [16], "reference": [7], "distance": 3.5,
        "upper": 0.1, "lower": -0.1,
    })));
    let bytes = api::write_bytes(shaft.bytes.to_vec(), &[(intent, None)])
        .unwrap()
        .0;
    let written = load_bytes("shaft.step", bytes).unwrap();
    assert!(
        specify_core::existing::read_existing(&written)
            .unwrap()
            .iter()
            .any(|e| e.r#type.starts_with("Location_")
                && (e.faces.contains(&16) || e.faces2.contains(&16))),
        "the depth is written"
    );
    let (t, _) = table(&written);
    assert_eq!(row(&t, "hole.position:holes:15.16")["state"], "gap");
}

/// A file whose datums the rules cannot take as answers still analyses (a table problem never
/// fails analyse): NIST FTC-09 part 0's datum E is four faces that form no one datum feature.
#[test]
fn nist_ftc09_part_0_analyses_and_lists_its_pmi() {
    let path = common::fixtures().join("nist/nist_ftc_09_asme1_ap242-e1.stp.gz");
    let analysis = api::analyse(&load(&path, Some(0)).unwrap()).unwrap();
    let existing = analysis["existing"].as_array().unwrap();
    assert!(!existing.is_empty());
    let table = &analysis["table"];
    let listed =
        table["rows"].as_array().unwrap().len() + table["file_pmi"].as_array().unwrap().len();
    assert!(listed > 0, "{table}");
    agree(&analysis);
}

/// NIST CTC-02 (its datums form questions the rules refused before) analyses too.
#[test]
fn nist_ctc02_analyses_and_lists_its_pmi() {
    let path = common::fixtures().join("nist/nist_ctc_02_asme1_ap242.stp.gz");
    let analysis = api::analyse(&load(&path, None).unwrap()).unwrap();
    assert!(!analysis["existing"].as_array().unwrap().is_empty());
    agree(&analysis);
}
