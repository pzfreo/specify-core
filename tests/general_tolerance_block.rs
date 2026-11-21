//! A general tolerance given as a block (#228; proposal specify-0.3.0, "A general tolerance
//! given as a block"): the four worked examples, a table another tool wrote, a change that
//! replaces the table, and the other two points of #228.

mod common;

use haecceity::pmi::{self, CellValue, GeneralTolerance, LengthUnit, PartPmi, Quantity};
use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::part::{LoadedPart, load, load_bytes};
use specify_core::writer::WriteReport;

fn map(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn plate() -> LoadedPart {
    load(&common::fixtures().join("parts/plate.step"), None).unwrap()
}

/// The #228 half-shell (from the issue's gist): an inch shop drawing's part, its STEP in
/// millimetres.
fn half_shell() -> LoadedPart {
    load(
        &common::fixtures().join("issue228-half-shell.step.gz"),
        None,
    )
    .unwrap()
}

/// Saves `answers` as the editor does, open questions and all (`session::intent_for`).
fn save(loaded: &LoadedPart, answers: &Map<String, Value>) -> (LoadedPart, WriteReport) {
    let analysis = api::analyse(loaded).unwrap();
    let (intent, _open) = specify_core::session::intent_for(loaded, &analysis, answers).unwrap();
    let (bytes, report) =
        specify_core::writer::write_bytes(loaded, &intent, Some(answers)).unwrap();
    (load_bytes("saved.step", bytes).unwrap(), report)
}

fn pmi_of(loaded: &LoadedPart) -> PartPmi {
    specify_core::existing::read(loaded)
        .unwrap()
        .model()
        .clone()
}

/// The general tolerance row of the feature table: (state, value).
fn general_row(loaded: &LoadedPart) -> (String, Value) {
    let analysis = api::analyse(loaded).unwrap();
    let row = analysis["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "part.general_tolerance")
        .unwrap_or_else(|| panic!("no general tolerance row"))
        .clone();
    (
        row["state"].as_str().unwrap().to_string(),
        row["value"].clone(),
    )
}

/// The general tolerance note's text, if the part has one.
fn note(p: &PartPmi) -> Option<String> {
    p.attributes.iter().find_map(|set| {
        set.items.iter().find_map(|(name, value)| match value {
            pmi::AttributeValue::Text(t) if name == "general tolerance" && set.on.is_none() => {
                Some(t.clone())
            }
            _ => None,
        })
    })
}

/// Each table cell's (digits, tolerance text, unit).
fn cells(p: &PartPmi) -> Vec<(String, String, LengthUnit)> {
    let tables: Vec<_> = p
        .general
        .iter()
        .filter_map(|g| match g {
            GeneralTolerance::Table { cells, .. } => Some(cells),
            GeneralTolerance::Class { .. } => None,
        })
        .collect();
    assert_eq!(tables.len(), 1, "one table: {:?}", p.general);
    tables[0]
        .iter()
        .map(|c| {
            let digits = c
                .items
                .iter()
                .find_map(|(n, v)| match v {
                    CellValue::Count(d) if n == "significant number of digits" => {
                        Some(d.as_str().to_string())
                    }
                    _ => None,
                })
                .unwrap();
            let (value, unit) = c
                .items
                .iter()
                .find_map(|(n, v)| match v {
                    CellValue::Value(v) if n == "plus minus tolerance value" => match &v.quantity {
                        Quantity::Length(l) => Some((l.value.as_str().to_string(), l.unit.clone())),
                        Quantity::Angle(_) => None,
                    },
                    _ => None,
                })
                .unwrap();
            (digits, value, unit)
        })
        .collect()
}

fn classes(p: &PartPmi) -> usize {
    p.general
        .iter()
        .filter(|g| matches!(g, GeneralTolerance::Class { .. }))
        .count()
}

// --- Example 1: the #228 half-shell ---------------------------------------------------------

#[test]
fn example_1_the_228_half_shell_gets_an_inch_table_and_a_note_as_stated() {
    let part = half_shell();
    let before = pmi_of(&part);
    let answers = map(json!({
        "part.general_tolerance": ".XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°",
    }));
    let (saved, report) = save(&part, &answers);
    let p = pmi_of(&saved);
    assert_eq!(
        cells(&p),
        vec![
            ("2".into(), "0.01".into(), LengthUnit::Inch),
            ("3".into(), "0.005".into(), LengthUnit::Inch),
        ],
        "{:?}",
        report.written
    );
    assert_eq!(note(&p).as_deref(), Some("±1/64; angles ±1°"));
    // The general tolerance writes only itself: no ISO class, no dimension.
    assert_eq!(classes(&p), 0);
    assert_eq!(p.dimensions.len(), before.dimensions.len());
    assert_eq!(p.tolerances.len(), before.tolerances.len());
    let (state, value) = general_row(&saved);
    assert_eq!(state, "stated");
    assert_eq!(value, ".XX ±.01 · .XXX ±.005 · ±1/64 · angles ±1°");
}

// --- Example 2: ISO 2768-m, as today ---------------------------------------------------------

#[test]
fn example_2_iso_2768_m_is_a_class_as_today() {
    let (saved, _) = save(
        &plate(),
        &map(json!({"part.general_tolerance": "ISO 2768-m"})),
    );
    let p = pmi_of(&saved);
    assert_eq!(classes(&p), 1);
    assert!(
        !p.general
            .iter()
            .any(|g| matches!(g, GeneralTolerance::Table { .. }))
    );
    assert_eq!(note(&p), None);
    assert_eq!(general_row(&saved), ("stated".into(), json!("ISO 2768-m")));
}

// --- Example 3: none stated ------------------------------------------------------------------

#[test]
fn example_3_none_needed_writes_nothing_and_is_remembered() {
    let part = plate();
    let (saved, _) = save(&part, &map(json!({"part.general_tolerance": "none"})));
    let p = pmi_of(&saved);
    assert!(p.general.is_empty(), "{:?}", p.general);
    assert_eq!(note(&p), None);
    let (state, value) = general_row(&saved);
    assert_eq!(state, "decided_none");
    assert_eq!(value, "none");
}

// --- Example 4: a metric block ---------------------------------------------------------------

#[test]
fn example_4_a_metric_block_is_a_millimetre_table() {
    let answers = map(json!({
        "part.general_tolerance": "X ±0.5, X.X ±0.2, X.XX ±0.05, angles ±0°30'",
    }));
    let (saved, report) = save(&plate(), &answers);
    let p = pmi_of(&saved);
    assert_eq!(
        cells(&p),
        vec![
            ("0".into(), "0.5".into(), LengthUnit::Millimetre),
            ("1".into(), "0.2".into(), LengthUnit::Millimetre),
            ("2".into(), "0.05".into(), LengthUnit::Millimetre),
        ],
        "{:?}",
        report.written
    );
    assert_eq!(note(&p).as_deref(), Some("angles ±0°30'"));
    assert_eq!(classes(&p), 0);
    assert_eq!(
        general_row(&saved),
        (
            "stated".into(),
            json!("X ±0.5 · X.X ±0.2 · X.XX ±0.05 · angles ±0°30′")
        )
    );
}
