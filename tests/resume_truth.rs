//! ADR P11 (which replaced P9 §3's reconciliation): the file's PMI is the only copy. An
//! unchanged round trip resumes every stored answer with no conflict; the edited plate is the
//! fixture the feature table's tests (`tests/feature_table.rs`) reopen.

mod common;

use std::path::PathBuf;

use serde_json::{Map, Value, json};
use specify_pmi::api;
use specify_pmi::part::{LoadedPart, load_bytes};
use specify_pmi::resume;

fn plate() -> PathBuf {
    common::fixtures().join("parts").join("plate.step")
}

/// The plate written with `file`'s PMI (defaults taken) while storing `stored` as its
/// answers: a file Specify wrote whose PMI was then edited elsewhere.
const EDITED: &str = "resume-p9/edited-plate.step.gz";

fn map(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

/// The answers the edited plate stores.
fn stored() -> Map<String, Value> {
    map(json!({
        "part.material": "6082-T6",
        "part.locating": "position tolerances",
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
        "datum.A.flatness": "0.05",
        "datum.B.control": "0.1",
        "hole.position:holes:12": "0.1",
    }))
}

/// The answers its PMI was written from: the hole's position changed and the flatness
/// removed. Another tool then added a fit on another hole (unmarked PMI).
fn edited() -> Map<String, Value> {
    let mut answers = stored();
    answers["hole.position:holes:12"] = json!("0.2");
    answers["datum.A.flatness"] = json!("none");
    answers
}

/// `bytes` with hole 9's ⌀8 H7, as Specify writes it, added by another tool: haecceity's own
/// writer, so it carries no Specify provenance marker.
fn with_foreign_fit(bytes: Vec<u8>, analysis: &Value) -> Vec<u8> {
    use haecceity::p21::Document;
    use haecceity::pmi;
    let mut fitted = stored();
    fitted.insert("hole.function:holes:9".into(), json!("fit:H7"));
    let source = write(std::fs::read(plate()).unwrap(), analysis, &fitted, &fitted);
    let defs = haecceity::step::read_part_definitions(&source).unwrap();
    let read = pmi::read(&Document::parse(source.clone()).unwrap(), &defs).unwrap();
    // The hole's size: `read_existing` lists Specify's toleranced dimensions in order.
    let listed = specify_pmi::existing::read_existing(&load(source.clone())).unwrap();
    let k = listed
        .iter()
        .filter(|e| e.kind == "dimension")
        .position(|e| e.r#type == "Size_Diameter" && e.faces == [9])
        .unwrap();
    let fit = read.parts[0]
        .dimensions
        .iter()
        .filter(|d| !matches!(d.tolerance, pmi::DimTolerance::None))
        .nth(k)
        .unwrap()
        .clone();
    let added = pmi::PartPmi {
        features: read.parts[0].features.clone(),
        dimensions: vec![fit],
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

fn write(
    loaded_bytes: Vec<u8>,
    analysis: &Value,
    answers: &Map<String, Value>,
    stored: &Map<String, Value>,
) -> Vec<u8> {
    let intent = api::apply(analysis, answers, true).unwrap();
    common::whole_intent::write_bytes(loaded_bytes, &[(intent, Some(stored.clone()))])
        .unwrap()
        .0
}

fn load(bytes: Vec<u8>) -> LoadedPart {
    load_bytes("plate.step", bytes).unwrap()
}

/// Writes `tests/fixtures/resume-p9/edited-plate.step` (run with `--ignored` to make it again).
#[test]
#[ignore]
fn regenerate_the_edited_plate() {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let bytes = write(
        std::fs::read(plate()).unwrap(),
        &analysis,
        &edited(),
        &stored(),
    );
    let bytes = with_foreign_fit(bytes, &analysis);
    let path = common::fixtures().join(EDITED);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    std::io::Write::write_all(&mut gz, &bytes).unwrap();
    std::fs::write(path, gz.finish().unwrap()).unwrap();
}

// An unchanged round trip resumes every answer with no conflict, across the corpus parts the
// write oracle writes (with QUIDDITY_CORPUS or SPECIFY_CORPUS; the fixtures otherwise).
common::case_group_tests!(check_unchanged_round_trips;
    0 => an_unchanged_round_trip_resumes_every_answer_group_0,
    1 => an_unchanged_round_trip_resumes_every_answer_group_1,
    2 => an_unchanged_round_trip_resumes_every_answer_group_2,
    3 => an_unchanged_round_trip_resumes_every_answer_group_3,
    4 => an_unchanged_round_trip_resumes_every_answer_group_4,
    5 => an_unchanged_round_trip_resumes_every_answer_group_5,
    6 => an_unchanged_round_trip_resumes_every_answer_group_6,
    7 => an_unchanged_round_trip_resumes_every_answer_group_7,
);

fn check_unchanged_round_trips(group: usize) {
    let plate = common::case("parts/plate");
    let mut visited_plate = false;
    let mut parts = 0;
    let mut failures = Vec::new();
    for c in common::cases()
        .into_iter()
        .filter(|case| case.group() == group)
    {
        if c.part().is_some() || c.meta["parts"].as_u64().unwrap_or(1) != 1 {
            continue;
        }
        let Some(input) = c.input() else { continue };
        let Some(answers) = c.meta["answers"].as_object().cloned() else {
            continue;
        };
        let Ok(analysis) = api::analyse_file(&input, None) else {
            continue;
        };
        let Ok(intent) = api::apply(&analysis, &answers, true) else {
            continue;
        };
        let Ok(source) = specify_pmi::part::load(&input, None) else {
            continue;
        };
        let bytes = source.bytes.to_vec();
        let Ok((written, _)) =
            common::whole_intent::write_bytes(bytes, &[(intent, Some(answers.clone()))])
        else {
            continue;
        };
        let back = load(written);
        let Some(expected) = resume::answers_for(&back).unwrap() else {
            continue;
        };
        parts += 1;
        visited_plate |= c.name == plate.name;
        let again = api::analyse(&back).unwrap();
        if again["resume"].get("conflicts").is_some()
            || again["resume"]["answers"] != Value::Object(expected)
        {
            failures.push(format!("{}: {}", c.name, again["resume"]));
        }
    }
    println!(
        "unchanged round trips: {parts} parts, {} with conflicts",
        failures.len()
    );
    // The original sweep required at least one successful round trip. This
    // committed fixture must contribute one even when the corpus is absent.
    if plate.group() == group {
        assert!(visited_plate, "the plate must reach the round-trip check");
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
