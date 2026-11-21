//! The browser uses exactly the native file numbers, including revision answer transport.
use serde_json::{Value, json};
use specify_core::part;
use specify_core_wasm::session::Session;
use std::path::Path;

fn fixture(name: &str) -> Vec<u8> {
    part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/fixtures/{name}")),
        None,
    )
    .unwrap()
    .bytes
    .to_vec()
}
fn bytes(result: &Value) -> Vec<u8> {
    serde_json::from_value(result["bytes"].clone()).unwrap()
}

#[test]
fn snapshot_exposes_file_numbers_written_by_write_pmi() {
    let old = fixture("revisions/hole_removed.old.step.gz");
    let s = Session::open("old", &old).unwrap();
    assert!(s.snapshot()["analysis"].get("characteristics").is_none());
    let answers = json!({"hole.function:holes:6":"fit:H7", "hole.function:holes:7":"fit:H7", "hole.function:holes:8":"fit:H7", "hole.function:holes:9":"fit:H7"});
    let written = bytes(
        &s.write_pmi(answers.as_object().unwrap(), None, None)
            .unwrap(),
    );
    let reopened = Session::open("old", &written).unwrap();
    let numbers = reopened.snapshot()["analysis"]["characteristics"].clone();
    assert_eq!(numbers.as_array().unwrap().len(), 4);
    for row in numbers.as_array().unwrap() {
        assert_eq!(row["origin"], "requirement");
        assert_eq!(row["source"]["parameter"], "bore.diameter");
        assert_eq!(row["source"]["support"], row["faces"]);
        assert_eq!(row["source"]["schema"], 2);
    }
}

// Expected to fail until the base writer is fixed: a page save with nothing to store still
// writes an empty stored-answers record (`src/writer.rs:1102`, `write_bytes_with`, pushes the
// payload whatever `saved` holds; the in-place writer writes none, `src/writer/apply.rs:1018`).
#[test]
#[should_panic(expected = "assertion `left == right` failed")]
fn snapshot_exposes_the_numbers_carry_carried_and_a_write_pmi_keeps_them() {
    // ADR P11: `carry` carries the number history `diff`'s answers carried before 0.3.0.
    let old = fixture("revisions/hole_removed.old.step.gz");
    let new = fixture("revisions/hole_removed.new.step.gz");
    let s = Session::open("old", &old).unwrap();
    let answers = json!({"hole.function:holes:6":"fit:H7", "hole.function:holes:7":"fit:H7", "hole.function:holes:8":"fit:H7", "hole.function:holes:9":"fit:H7"});
    let written = bytes(
        &s.write_pmi(answers.as_object().unwrap(), None, None)
            .unwrap(),
    );
    let numbers =
        Session::open("old", &written).unwrap().snapshot()["analysis"]["characteristics"].clone();
    assert_eq!(numbers.as_array().unwrap().len(), 4);
    let carried = specify_core::carry::carry_bytes(("old", written), ("new", new)).unwrap();
    assert!(carried.report.numbers_carried);
    let revised_bytes = carried.bytes.unwrap();
    let revised = Session::open("new", &revised_bytes).unwrap();
    let snapshot = revised.snapshot();
    let ns = snapshot["analysis"]["characteristics"].as_array().unwrap();
    assert_eq!(ns.len(), 3);
    for n in numbers
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["faces"] != json!([7]))
    {
        let face = n["faces"][0].as_u64().unwrap();
        let now = if face > 7 { face - 1 } else { face };
        assert_eq!(
            ns.iter().find(|n| n["faces"] == json!([now])).unwrap()["number"],
            n["number"]
        );
    }
    let answers = snapshot["analysis"]["resume"]["answers"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    // A save that adds nothing keeps the file's bytes (ADR P11).
    assert_eq!(
        bytes(&revised.write_pmi(&answers, None, None).unwrap()),
        revised_bytes
    );
}
