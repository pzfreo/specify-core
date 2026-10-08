//! Review, not interrogation: which questions need a person at all. specify-core's
//! `tests/test_attention.py`, on the oracle's captured analyses of the plate, the corpus spool
//! and NIST CTC-03.

mod common;

use std::collections::HashSet;

use serde_json::{Map, Value, json};
use specify_core::rules::{ATTENTION, Question, questions};

fn analysis(case: &str) -> Value {
    common::case(case)
        .artifact("analysis")
        .expect("a captured analysis")
}

/// `_by_attention`: the questions, every attention level one of `ATTENTION`.
fn by_attention(case: &str) -> Vec<Question> {
    let qs = questions(&analysis(case), &Map::new()).unwrap();
    let levels: HashSet<&str> = qs.iter().map(|q| q.attention).collect();
    assert!(levels.iter().all(|l| ATTENTION.contains(l)), "{levels:?}");
    qs
}

const SPOOL: &str = "corpus/cadgenbench/flanged_spool_132";

// test_attention.py::test_only_material_must_be_answered_on_a_standard_part
#[test]
fn only_material_must_be_answered_on_a_standard_part() {
    // Clearance sizes match standards, so those holes need no check. The plate's top and
    // bottom are the same size, though, and which one it is mounted on cannot be read from
    // the geometry.
    let qs = by_attention("parts/plate");
    let required: Vec<&str> = qs
        .iter()
        .filter(|q| q.attention == "required")
        .map(|q| q.id.as_str())
        .collect();
    assert_eq!(required, ["part.material"]);
    let checks: Vec<&str> = qs
        .iter()
        .filter(|q| q.attention == "check")
        .map(|q| q.id.as_str())
        .collect();
    assert!(checks.contains(&"datum.A"));
    assert!(
        checks
            .iter()
            .all(|c| *c == "datum.A" || c.starts_with("hole.function"))
    );
}

// test_attention.py::test_a_hole_assumed_tapped_is_flagged_for_a_person
#[test]
fn a_hole_assumed_tapped_is_flagged_for_a_person() {
    // A thread is claimed from a diameter alone, so it is proposed, not assumed.
    let qs = by_attention("parts/plate");
    let tapped: Vec<&Question> = qs
        .iter()
        .filter(|q| {
            q.default
                .as_ref()
                .and_then(Value::as_str)
                .is_some_and(|d| d.starts_with("tapped:"))
        })
        .collect();
    assert_eq!(tapped.len(), 1);
    assert_eq!(tapped[0].default, Some(json!("tapped:M5")));
    assert_eq!(tapped[0].attention, "check");
    assert!(
        tapped[0]
            .reason
            .starts_with("⌀4.2 is the tap-drill size for M5")
    );
}

// test_attention.py::test_groups_of_unmatched_holes_are_flagged_with_a_reason
#[test]
fn groups_of_unmatched_holes_are_flagged_with_a_reason() {
    let qs = by_attention(SPOOL);
    // The 6x Ø10 match M8 in ISO 273's coarse series and default to clearance; the two
    // identical 12x Ø8 circles match no bolt series: one check.
    let checks: Vec<&Question> = qs.iter().filter(|q| q.attention == "check").collect();
    assert_eq!(checks.len(), 1);
    let check = checks[0];
    assert!(check.id.contains("bolt_circle"));
    assert!(check.prompt.contains("24× Ø8 holes in 2 identical groups"));
    assert!(check.reason.contains("position tolerance"));
    // A single odd hole (the spool's bore) is just a hole.
    let bore = qs
        .iter()
        .find(|q| q.id == "hole.function:holes:34.55")
        .unwrap();
    assert_eq!(bore.attention, "routine");
}

// test_attention.py::test_tightenings_are_refinements
#[test]
fn tightenings_are_refinements() {
    for q in by_attention(SPOOL) {
        if q.id.starts_with("diameter.fit:")
            || q.id.ends_with(".control")
            || q.id.ends_with(".flatness")
        {
            assert_eq!(q.attention, "refine", "{}", q.id);
        }
    }
}

// test_attention.py::test_a_doubtful_primary_datum_is_flagged
#[test]
fn a_doubtful_primary_datum_is_flagged() {
    // CTC-03's largest face is not NIST's datum A; another is nearly as large.
    let qs = by_attention("corpus/nist/nist_ctc_03_asme1_rc");
    let a = qs.iter().find(|q| q.id == "datum.A").unwrap();
    assert_eq!(a.attention, "check");
    assert!(a.reason.contains("nearly as large"));
}

// test_attention.py::test_attention_is_in_the_json
#[test]
fn attention_is_in_the_json() {
    let qs = questions(&analysis("parts/plate"), &Map::new()).unwrap();
    assert_eq!(qs[0].to_dict()["attention"], "required");
}
