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

#[test]
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
    // A save of what it resumes changes nothing the carry wrote: every record keeps its text
    // and every number stays. (The page's answers save adds its empty stored-answers record,
    // which the in-place writer leaves out when nothing is stored; piece 6 moves the page's
    // save to edit lists.)
    let again = bytes(&revised.write_pmi(&answers, None, None).unwrap());
    let records = |b: &[u8]| -> Vec<String> {
        String::from_utf8_lossy(b)
            .split(';')
            .map(|r| r.trim().to_owned())
            .collect()
    };
    let resaved = records(&again);
    for record in records(&revised_bytes) {
        assert!(resaved.contains(&record), "{record}");
    }
    assert_eq!(
        Session::open("again", &again).unwrap().snapshot()["analysis"]["characteristics"],
        snapshot["analysis"]["characteristics"]
    );
}
