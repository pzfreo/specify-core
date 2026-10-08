//! Location dimensions, basic from a position's datum planes and ± from the edges:
//! specify-core's `tests/test_locations.py`, on the oracle's captured analysis of the plate.
//!
//! Each test's write and read-back is the writer's (stage 3); the intent each asserts is here.
//! `test_only_untoleranced_locations_are_marked_basic` is not ported: it tests `mark_basic`, an
//! edit of OpenCascade's output that this port does not make (haecceity writes a basic
//! dimension as basic).

mod common;

use serde_json::{Map, Value, json};
use specify_core::choices::interpret;
use specify_core::locations::parse;
use specify_core::rules::{Intent, LOCATING, apply_with_defaults, questions};

fn plate() -> Value {
    common::case("parts/plate")
        .artifact("analysis")
        .expect("a captured analysis")
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

fn locations(intent: &Intent) -> Vec<&Map<String, Value>> {
    intent
        .requirements
        .iter()
        .filter(|r| r["kind"] == "location")
        .collect()
}

// test_locations.py::test_positioned_holes_get_basic_dimensions_from_their_datum_planes
#[test]
fn positioned_holes_get_basic_dimensions_from_their_datum_planes() {
    let intent =
        apply_with_defaults(&plate(), &answers(json!({"part.material": "steel"})), true).unwrap();
    let basic = locations(&intent);
    assert!(!basic.is_empty());
    assert!(
        basic
            .iter()
            .all(|r| r["basic"] == json!(true) && !r.contains_key("upper"))
    );
    let datum_faces: Vec<&Value> = intent
        .datums
        .iter()
        .filter(|d| d["letter"] == "B" || d["letter"] == "C")
        .map(|d| &d["faces"])
        .collect();
    assert!(basic.iter().all(|r| datum_faces.contains(&&r["reference"])));
    // The M5 hole at x=45, y=0 on a 100 x 60 plate: 95 and 30 from its edges.
    let mut m5: Vec<f64> = basic
        .iter()
        .filter(|r| r["feature"] == "holes:12")
        .map(|r| r["distance"].as_f64().unwrap())
        .collect();
    m5.sort_by(f64::total_cmp);
    assert_eq!(m5, [30.0, 95.0]);
}

// test_locations.py::test_holes_located_by_plus_minus_can_be_given_a_tolerance
#[test]
fn holes_located_by_plus_minus_can_be_given_a_tolerance() {
    let plate = plate();
    let qs = questions(&plate, &answers(json!({"part.locating": LOCATING[1]}))).unwrap();
    let asked: Vec<_> = qs
        .iter()
        .filter(|q| q.id.starts_with("hole.location"))
        .collect();
    assert!(!asked.is_empty());
    assert!(
        asked
            .iter()
            .all(|q| q.default == Some(json!("general")) && q.options.iter().any(|o| o == "±0.05"))
    );
    let grid = asked
        .iter()
        .find(|q| q.id.contains("grid"))
        .unwrap()
        .id
        .clone();
    let given = answers(json!({
        "part.locating": LOCATING[1],
        "part.material": "steel",
        grid: "±0.05",
    }));
    let intent = apply_with_defaults(&plate, &given, true).unwrap();
    let located = locations(&intent);
    assert_eq!(located.len(), 12);
    assert!(
        located
            .iter()
            .all(|r| r["upper"].as_f64() == Some(0.05) && r["lower"].as_f64() == Some(-0.05))
    );
    assert!(!located.iter().any(|r| r.contains_key("basic")));
}

// test_locations.py::test_general_writes_no_location
#[test]
fn general_writes_no_location() {
    let given = answers(json!({"part.locating": LOCATING[1], "part.material": "steel"}));
    let intent = apply_with_defaults(&plate(), &given, true).unwrap();
    assert!(locations(&intent).is_empty());
}

// test_locations.py::test_a_typed_location_tolerance (every parameter)
#[test]
fn a_typed_location_tolerance() {
    for (text, value) in [
        ("0.03", "±0.03"),
        ("±0.1", "±0.1"),
        ("+-0.2", "±0.2"),
        ("none", "general"),
    ] {
        let none: [&str; 0] = [];
        assert_eq!(
            interpret("hole.location:x", text, &none, None, None).unwrap(),
            value
        );
        let expected = (value != "general").then(|| value[2..].parse::<f64>().unwrap());
        assert_eq!(parse(&json!(value)).unwrap(), expected, "{value}");
    }
}

// test_locations.py::test_a_part_whose_only_dimensions_are_locations_keeps_its_tolerance_values
// (the intent; the value read back is the writer's)
#[test]
fn a_part_whose_only_dimensions_are_locations_keeps_its_tolerance_values() {
    let plate = plate();
    let mut given = answers(json!({"part.material": "steel"}));
    for q in questions(&plate, &Map::new()).unwrap() {
        let tapped = q
            .default
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(|d| d.starts_with("tapped"));
        if q.id.starts_with("hole.function") && !tapped {
            given.insert(q.id.clone(), json!("general"));
        }
    }
    let intent = apply_with_defaults(&plate, &given, true).unwrap();
    assert!(!intent.requirements.iter().any(|r| r["kind"] == "size"));
    assert!(!locations(&intent).is_empty());
    let positions: Vec<_> = intent
        .requirements
        .iter()
        .filter(|r| r["kind"] == "position")
        .collect();
    assert_eq!(positions.len(), 1);
}
