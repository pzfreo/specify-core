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
    // Beyond Python: turned_knurl's parameters from DIN 82, each with its source (Q4, Q15).
    assert_eq!(knurl["teeth"], json!(31)); // π × 10 / 1 = 31.4
    assert_eq!(knurl["tooth_depth"].as_f64(), Some(0.5));
    assert_eq!(knurl["root_fillet"].as_f64(), Some(0.1));
    assert_eq!(knurl["helix_angle"].as_f64(), Some(30.0));
    assert!(knurl.get("hand").is_none() && knurl.get("helix_hand").is_none());
    assert_eq!(knurl["standard"], "DIN 82");
    let derived: Vec<&String> = knurl["derived"].as_object().unwrap().keys().collect();
    assert_eq!(
        derived,
        ["teeth", "tooth_depth", "root_fillet", "helix_angle"]
    );
}

/// A straight knurl states the number of teeth, tooth depth and root fillet, and neither a
/// helix angle nor a hand (questions Q4, Q15).
#[test]
fn a_straight_knurl_states_its_standard_parameters() {
    let pin = pin();
    let ten = feature(&pin, "turned_steps", 10.0);
    let given = answers(json!({
        "part.material": "brass",
        format!("diameter.fit:{ten}"): "knurl:straight",
    }));
    let intent = apply_with_defaults(&pin, &given, true).unwrap();
    let knurl = &requirements_of(&intent, "knurl")[0];
    let keys: Vec<&String> = knurl.keys().collect();
    assert_eq!(
        keys,
        [
            "kind",
            "feature",
            "faces",
            "pattern",
            "pitch",
            "diameter",
            "teeth",
            "tooth_depth",
            "root_fillet",
            "standard",
            "derived",
            "question"
        ]
    );
    assert_eq!(knurl["teeth"], json!(31));
    assert!(
        knurl["derived"]["teeth"]
            .as_str()
            .unwrap()
            .starts_with("π × 10 / 1 rounded")
    );
}

/// The knurling table: DIN 82's pitches (IS 3403:1981 §3.2), teeth rounded with the pitch kept,
/// depth P/2 of the 90° profile, the wheel's crest radius as the root fillet (IS 6776:1980
/// §4.3), 30° for a diamond; refused off the table.
#[test]
fn knurl_parameters_follow_the_standard() {
    let k = standards::knurl_parameters("straight", 0.8, 20.0).unwrap();
    assert_eq!(k.teeth, 79); // 78.54
    assert_eq!(
        (k.tooth_depth, k.root_fillet, k.helix_angle),
        (0.4, 0.1, None)
    );
    let k = standards::knurl_parameters("diamond", 1.6, 25.0).unwrap();
    assert_eq!(k.teeth, 49); // 49.09
    assert_eq!(
        (k.tooth_depth, k.root_fillet, k.helix_angle),
        (0.8, 0.15, Some(30.0))
    );
    let k = standards::knurl_parameters("straight", 1.0, 10.0).unwrap();
    assert_eq!(k.root_fillet, 0.1); // up to and including 1.0 mm
    let k = standards::knurl_parameters("straight", 1.2, 10.0).unwrap();
    assert_eq!(k.root_fillet, 0.15);
    assert_eq!(
        standards::knurl_parameters("straight", 0.7, 10.0).unwrap_err(),
        "a pitch of 0.7 mm is not one of DIN 82's (0.5, 0.6, 0.8, 1, 1.2, 1.6 mm)"
    );
    assert!(
        standards::knurl_parameters("diagonal", 1.0, 10.0)
            .unwrap_err()
            .contains("straight or diamond")
    );
    assert!(standards::knurl_parameters("straight", 1.0, 0.0).is_err());
}
