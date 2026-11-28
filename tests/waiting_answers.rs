//! An answer that only waits on another answer is neither refused nor dropped: the rest of the
//! save is written, the answer stays open and is reported, and a later save with what it waits
//! for writes it. Genuine errors still refuse the whole save.

mod common;

use serde_json::{Value, json};
use specify_pmi::edits;
use specify_pmi::part::{LoadedPart, load, load_bytes};
use specify_pmi::writer::apply::{Applied, apply};
use specify_pmi::api;

fn plate() -> LoadedPart {
    load(&common::fixtures().join("parts/plate.step"), None).unwrap()
}

fn run(loaded: &LoadedPart, text: &str) -> Result<Applied, Vec<edits::Failure>> {
    apply(loaded, &edits::read(text).unwrap())
}

fn state(loaded: &LoadedPart, id: &str) -> (Value, bool) {
    let table = api::analyse(loaded).unwrap()["table"].clone();
    let row = table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("no row {id}"));
    (row["state"].clone(), row["open"] == json!(true))
}

fn waiting_ids(applied: &Applied) -> Vec<String> {
    applied.report.waiting.iter().map(|w| w.id.clone()).collect()
}

const GENERAL: &str = "hole.function:holes:9";
const POSITION: &str = "hole.position:holes:12";

#[test]
fn a_hole_left_to_the_general_tolerance_waits_for_it() {
    let first = plate();
    let saved = run(
        &first,
        r#"{"fill": {"part.material": "6082-T6", "hole.function:holes:9": "general"}}"#,
    )
    .unwrap();
    assert_eq!(waiting_ids(&saved), [GENERAL]);
    assert_eq!(saved.report.waiting[0].waits_for, ["part.general_tolerance"]);
    assert_eq!(saved.report.waiting[0].entry, format!("fill[\"{GENERAL}\"]"));
    assert!(saved.report.waiting[0].message.contains("general tolerance"));
    assert!(saved.report.written.iter().any(|l| l.contains("6082-T6")));
    assert!(saved.report.not_written.is_empty(), "{:?}", saved.report.not_written);
    assert_eq!(saved.to_dict()["waiting"][0]["id"], GENERAL);

    // It stays open in the saved file.
    let reopened = load_bytes("a.step", saved.bytes.clone()).unwrap();
    assert!(state(&reopened, GENERAL).1, "still open");
    assert_eq!(state(&reopened, "part.material").0, "stated");

    // A later save, once the part states a general tolerance, writes it.
    let second = run(
        &reopened,
        r#"{"fill": {"part.general_tolerance": "ISO 2768-m", "hole.function:holes:9": "general"}}"#,
    )
    .unwrap();
    assert!(second.report.waiting.is_empty());
    let after = load_bytes("b.step", second.bytes).unwrap();
    assert!(!state(&after, GENERAL).1, "answered");
}

#[test]
fn the_general_tolerance_given_by_an_earlier_save_lets_the_hole_through() {
    let first = plate();
    let one = run(&first, r#"{"fill": {"part.general_tolerance": "ISO 2768-m"}}"#).unwrap();
    assert!(one.report.waiting.is_empty());
    let reopened = load_bytes("a.step", one.bytes).unwrap();
    let two = run(&reopened, r#"{"fill": {"hole.function:holes:9": "general"}}"#).unwrap();
    assert!(two.report.waiting.is_empty());
}

#[test]
fn a_position_waits_for_the_datums_it_cites() {
    let first = plate();
    let saved = run(
        &first,
        r#"{"fill": {"part.material": "6082-T6", "hole.position:holes:12": "0.25"}}"#,
    )
    .unwrap();
    assert_eq!(waiting_ids(&saved), [POSITION]);
    assert_eq!(
        saved.report.waiting[0].waits_for,
        ["datum.A", "datum.B", "datum.C"]
    );
    assert!(saved.report.written.iter().any(|l| l.contains("6082-T6")));
    let reopened = load_bytes("a.step", saved.bytes).unwrap();
    assert!(state(&reopened, POSITION).1, "still open");

    // With the datums in the same list it is written.
    let second = run(
        &reopened,
        r#"{"fill": {"datum.A": [2], "datum.B": [1], "datum.C": [0],
                     "hole.position:holes:12": "0.25"}}"#,
    )
    .unwrap();
    assert!(second.report.waiting.is_empty());
    let after = load_bytes("b.step", second.bytes).unwrap();
    assert!(!state(&after, POSITION).1, "answered");
}

#[test]
fn a_legacy_answers_file_waits_the_same_way() {
    let first = plate();
    let saved = run(
        &first,
        r#"{"part.material": "6082-T6", "hole.function:holes:9": "general",
            "hole.position:holes:12": "0.25"}"#,
    )
    .unwrap();
    assert_eq!(waiting_ids(&saved), [GENERAL, POSITION]);
    assert!(saved.report.written.iter().any(|l| l.contains("6082-T6")));
}

#[test]
fn only_waiting_answers_change_nothing_in_the_file() {
    let first = plate();
    let saved = run(&first, r#"{"fill": {"hole.function:holes:9": "general"}}"#).unwrap();
    assert_eq!(waiting_ids(&saved), [GENERAL]);
    assert!(saved.report.written.is_empty());
    assert_eq!(saved.bytes, first.bytes.to_vec());
}

#[test]
fn the_check_reports_what_waits_and_is_applicable() {
    let first = plate();
    let outcome = specify_pmi::writer::apply::check(
        &first,
        r#"{"fill": {"part.material": "6082-T6", "hole.function:holes:9": "general"}}"#,
    );
    assert!(outcome.applicable, "{:?}", outcome.failures);
    assert_eq!(outcome.waiting.len(), 1);
    let shown = outcome.to_json();
    assert_eq!(shown["waiting"][0]["id"], GENERAL);
    assert!(shown["message"].as_str().unwrap().contains("waits for"));
}

#[test]
fn genuine_errors_still_refuse_the_whole_save() {
    let first = plate();
    // An unknown id beside a waiting answer.
    let refused = run(
        &first,
        r#"{"fill": {"hole.function:holes:9": "general", "hole.function:holes:99": "general"}}"#,
    )
    .unwrap_err();
    assert!(refused.iter().any(|f| f.entry.contains("holes:99")), "{refused:?}");
    // An invalid value beside a waiting answer.
    let refused = run(
        &first,
        r#"{"fill": {"hole.function:holes:9": "general", "part.general_tolerance": "ISO 9999-x"}}"#,
    );
    assert!(refused.is_err(), "an invalid value refuses");
}

/// A hole's location left to the general tolerance (`hole.location` "general", the question's
/// default when `part.locating` is "± dimensions") is a decided answer, not an unwritable one.
#[test]
fn a_location_left_to_the_general_tolerance_is_decided() {
    let first = plate();
    let one = run(
        &first,
        r#"{"fill": {"part.general_tolerance": "ISO 2768-m", "part.locating": "± dimensions"}}"#,
    )
    .unwrap();
    let located = load_bytes("a.step", one.bytes).unwrap();
    let table = api::analyse(&located).unwrap()["table"].clone();
    let ids: Vec<String> = table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["id"].as_str())
        .filter(|id| id.starts_with("hole.location:"))
        .map(String::from)
        .collect();
    assert!(!ids.is_empty(), "± dimensions asks how closely holes are located");
    let (general, tight) = (&ids[0], ids.get(1).unwrap_or(&ids[0]));

    let list = json!({"fill": {general.as_str(): "general"}}).to_string();
    let saved = run(&located, &list).unwrap();
    assert!(saved.report.waiting.is_empty());
    let after = load_bytes("b.step", saved.bytes).unwrap();
    let (state, open) = state(&after, general);
    assert_eq!(state, "decided_none");
    assert!(!open);
    // The legacy answers file gives the same.
    assert!(run(&located, &json!({general.as_str(): "general"}).to_string()).is_ok());

    // A tighter ± still writes a requirement.
    let list = json!({"fill": {"datum.A": [2], "datum.B": [1], "datum.C": [0],
                               tight.as_str(): "±0.1"}})
    .to_string();
    let saved = run(&located, &list).unwrap();
    let after = load_bytes("c.step", saved.bytes).unwrap();
    assert_eq!(state_of(&after, tight), "stated");
}

fn state_of(loaded: &LoadedPart, id: &str) -> Value {
    state(loaded, id).0
}
