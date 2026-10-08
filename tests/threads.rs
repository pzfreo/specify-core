//! Tapped holes, external threads and knurls as intent: the rules side of specify-core's
//! `tests/test_threads.py`, on the oracle's captured analysis of the pin.
//!
//! Not here: writing a typed thread depth and reading it back (`test_a_typed_thread_depth_is_
//! written_and_read_back`'s second half) is the writer's (stage 3); `test_the_serve_child_
//! reports_the_depth_it_kept` is serve's (stage 4), and the interpretation it reports is
//! checked in `tests/choices.rs`.

mod common;

use serde_json::{Map, Value, json};
use specify_core::rules::{Error, apply_with_defaults, questions};
use specify_core::standards;

fn pin() -> Value {
    common::case("parts/pin")
        .artifact("analysis")
        .expect("a captured analysis")
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

/// `_feature`: the id of the feature of `family` at `diameter`.
fn feature(analysis: &Value, family: &str, diameter: f64) -> String {
    analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["family"] == family && f["record"]["diameter"].as_f64() == Some(diameter))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn requirements_of(intent: &specify_core::rules::Intent, kind: &str) -> Vec<Map<String, Value>> {
    intent
        .requirements
        .iter()
        .filter(|r| r["kind"] == kind)
        .cloned()
        .collect()
}

// test_threads.py::test_thread_standards
#[test]
fn thread_standards() {
    assert_eq!(standards::full_thread(8.0, "M5"), 5.5); // 8 - 3 x 0.8, down to 0.5
    assert_eq!(standards::thread_size(6.0), Some("M6"));
    assert_eq!(standards::thread_size(62.0), None);
}

// test_threads.py::test_turned_diameters_offer_a_thread_at_metric_sizes_and_knurls
#[test]
fn turned_diameters_offer_a_thread_at_metric_sizes_and_knurls() {
    let pin = pin();
    let id = format!("diameter.fit:{}", feature(&pin, "turned_steps", 6.0));
    let qs = questions(&pin, &Map::new()).unwrap();
    let six = qs.iter().find(|q| q.id == id).unwrap();
    for option in ["thread:M6", "knurl:straight", "knurl:diamond"] {
        assert!(six.options.iter().any(|o| o == option), "{option}");
    }
    assert_eq!(
        six.prompt,
        "Is the turned Ø6 diameter a fit, a thread or knurled?"
    );
}

// test_threads.py::test_a_tapped_hole_carries_its_thread_and_drill
#[test]
fn a_tapped_hole_carries_its_thread_and_drill() {
    let pin = pin();
    let hole = format!("hole.function:{}", feature(&pin, "holes", 4.2));
    let qs = questions(&pin, &Map::new()).unwrap();
    let asked = qs.iter().find(|q| q.id == hole).unwrap();
    assert_eq!(asked.default, Some(json!("tapped:M5")));
    let intent =
        apply_with_defaults(&pin, &answers(json!({"part.material": "brass"})), true).unwrap();
    let threads = requirements_of(&intent, "thread");
    assert_eq!(threads.len(), 1);
    let thread = &threads[0];
    for (key, value) in [
        ("side", json!("internal")),
        ("designation", json!("M5")),
        ("pitch", json!(0.8)),
        ("class", json!("6H")),
        ("hand", json!("RH")),
    ] {
        assert_eq!(thread[key], value, "{key}");
    }
    assert_eq!(thread["drill_diameter"].as_f64(), Some(4.2));
    assert_eq!(thread["drill_depth"].as_f64(), Some(8.0));
    assert_eq!(thread["full_thread"].as_f64(), Some(5.5));
    assert_eq!(thread["through"], json!(false));
}

// test_threads.py::test_a_typed_thread_depth_is_written_and_read_back (the rules side)
#[test]
fn a_typed_thread_depth_is_written_and_read_back() {
    let pin = pin();
    let hole = format!("hole.function:{}", feature(&pin, "holes", 4.2));
    match questions(&pin, &answers(json!({ hole.clone(): "tapped:M5@6" }))) {
        Err(Error::Invalid(message)) => assert!(message.contains("at most 5.6 mm"), "{message}"),
        other => panic!("not refused: {other:?}"),
    }
    let given = answers(json!({"part.material": "brass", hole: "tapped:M5@5.6"})); // 8 - 3 x 0.8
    let intent = apply_with_defaults(&pin, &given, true).unwrap();
    let threads = requirements_of(&intent, "thread");
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0]["drill_depth"].as_f64(), Some(8.0));
    assert_eq!(threads[0]["full_thread"].as_f64(), Some(5.6));
}

// test_threads.py::test_external_thread_and_knurl_from_turned_answers
#[test]
fn external_thread_and_knurl_from_turned_answers() {
    let pin = pin();
    let six = feature(&pin, "turned_steps", 6.0);
    let ten = feature(&pin, "turned_steps", 10.0);
    let given = answers(json!({
        "part.material": "brass",
        format!("diameter.fit:{six}"): "thread:M6",
        format!("diameter.fit:{ten}"): "knurl:diamond",
    }));
    let intent = apply_with_defaults(&pin, &given, true).unwrap();
    let of = |fid: &str| {
        intent
            .requirements
            .iter()
            .rfind(|r| r.get("feature") == Some(&json!(fid)))
            .unwrap()
            .clone()
    };
    let thread = of(&six);
    assert_eq!(thread["kind"], "thread");
    assert_eq!(thread["side"], "external");
    assert_eq!(thread["spec"], "M6x1");
    assert_eq!(thread["class"], "6g");
    assert_eq!(thread["length"].as_f64(), Some(12.0));
    assert_eq!(thread["nominal"].as_f64(), Some(6.0));
    let knurl = of(&ten);
    assert_eq!(knurl["kind"], "knurl");
    assert_eq!(knurl["pattern"], "diamond");
    assert_eq!(knurl["pitch"].as_f64(), Some(1.0));
    assert_eq!(knurl["diameter"].as_f64(), Some(10.0));
}
