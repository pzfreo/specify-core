//! Questions, defaults and intent: specify-core's `tests/test_rules.py`.
//!
//! Where a Python test analyses a part, the oracle's captured Python analysis of it is used
//! (`parts/plate`, the corpus spool), so the rules are checked independently of recognition.

mod common;

use serde_json::{Map, Value, json};
use specify_core::rules::{Error, Question, apply_with_defaults, questions};

fn analysis(case: &str) -> Value {
    common::case(case)
        .artifact("analysis")
        .expect("a captured analysis")
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().cloned().expect("an object")
}

fn by_prompt<'q>(qs: &'q [Question], text: &str) -> &'q Question {
    qs.iter()
        .find(|q| q.prompt.contains(text))
        .unwrap_or_else(|| panic!("no question asks {text:?}"))
}

fn by_id<'q>(qs: &'q [Question], id: &str) -> &'q Question {
    qs.iter()
        .find(|q| q.id == id)
        .unwrap_or_else(|| panic!("no question {id}"))
}

const SPOOL: &str = "corpus/cadgenbench/flanged_spool_132";

// test_rules.py::test_defaults_come_from_standard_sizes
#[test]
fn defaults_come_from_standard_sizes() {
    let qs = questions(&analysis("parts/plate"), &Map::new()).unwrap();
    assert_eq!(
        by_prompt(&qs, "the 6× Ø6.6").default,
        Some(json!("clearance:M6"))
    );
    assert_eq!(
        by_prompt(&qs, "Ø4.2 hole").default,
        Some(json!("tapped:M5"))
    );
    assert_eq!(by_prompt(&qs, "Ø8 hole").default, Some(json!("general")));
}

// test_rules.py::test_datums_default_to_three_square_planes
#[test]
fn datums_default_to_three_square_planes() {
    let plate = analysis("parts/plate");
    let qs = questions(&plate, &Map::new()).unwrap();
    let faces = plate["faces"].as_array().unwrap();
    let face = |letter: &str| {
        let id = by_id(&qs, &format!("datum.{letter}"))
            .default
            .as_ref()
            .unwrap()[0]
            .clone();
        faces.iter().find(|f| f["id"] == id).unwrap().clone()
    };
    let (a, b, c) = (face("A"), face("B"), face("C"));
    let largest = faces
        .iter()
        .filter(|f| f["kind"] == "plane")
        .map(|f| f["area"].as_f64().unwrap())
        .fold(f64::MIN, f64::max);
    assert_eq!(a["area"].as_f64().unwrap(), largest);
    let dot = |u: &Value, v: &Value| {
        (0..3)
            .map(|k| u[k].as_f64().unwrap() * v[k].as_f64().unwrap())
            .sum::<f64>()
    };
    assert!(dot(&a["direction"], &b["direction"]).abs() < 1e-9);
    assert!(dot(&b["direction"], &c["direction"]).abs() < 1e-9);
}

// test_rules.py::test_position_follows_the_fastener_formula
#[test]
fn position_follows_the_fastener_formula() {
    let qs = questions(&analysis("parts/plate"), &Map::new()).unwrap();
    let grid = by_prompt(&qs, "Position tolerance (Ø) for the 6× Ø6.6");
    let tapped = by_prompt(&qs, "Position tolerance (Ø) for the Ø4.2");
    assert_eq!(grid.default, Some(json!("0.6"))); // floating: 6.6 - 6
    assert_eq!(tapped.default, Some(json!("0.25"))); // fixed: (5.5 - 5) / 2
}

// test_rules.py::test_general_holes_get_no_position_question
#[test]
fn general_holes_get_no_position_question() {
    let qs = questions(&analysis("parts/plate"), &Map::new()).unwrap();
    assert!(
        !qs.iter()
            .any(|q| q.prompt.contains("Position") && q.prompt.contains("Ø8 hole"))
    );
}

// test_rules.py::test_incomplete_answers_are_refused
#[test]
fn incomplete_answers_are_refused() {
    match apply_with_defaults(&analysis("parts/plate"), &Map::new(), true) {
        Err(Error::Incomplete(e)) => {
            assert_eq!(e.missing, ["part.material"]);
            assert_eq!(e.to_string(), "1 question(s) unanswered: part.material");
        }
        other => panic!("not refused as incomplete: {other:?}"),
    }
}

// test_rules.py::test_intent_from_defaults
#[test]
fn intent_from_defaults() {
    let intent = apply_with_defaults(
        &analysis("parts/plate"),
        &answers(json!({"part.material": "6082-T6"})),
        true,
    )
    .unwrap();
    let mut kinds: Vec<&str> = intent
        .requirements
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .filter(|k| *k != "location")
        .collect();
    kinds.sort_unstable();
    assert_eq!(kinds, ["position", "position", "size", "thread"]);
    // Each positioned hole has basic dimensions from B and C: 6 + 1 holes, 2 each.
    let basic = intent
        .requirements
        .iter()
        .filter(|r| r.get("basic") == Some(&json!(true)))
        .count();
    assert_eq!(basic, 14);
    let letters: Vec<&Value> = intent.datums.iter().map(|d| &d["letter"]).collect();
    assert_eq!(letters, [&json!("A"), &json!("B"), &json!("C")]);
}

// test_rules.py::test_off_table_holes_can_be_clearance_for_an_unstated_bolt
#[test]
fn off_table_holes_can_be_clearance_for_an_unstated_bolt() {
    // The spool's Ø8 holes match no ISO 273 size.
    let spool = analysis(SPOOL);
    let qs = questions(&spool, &Map::new()).unwrap();
    let eights: Vec<&Question> = qs
        .iter()
        .filter(|q| q.id.starts_with("hole.function") && q.prompt.contains("Ø8 holes"))
        .collect();
    assert!(!eights.is_empty());
    assert!(
        eights
            .iter()
            .all(|q| q.options.iter().any(|o| o == "clearance"))
    );
    let answered = answers(json!({ eights[0].id.clone(): "clearance" }));
    let qs = questions(&spool, &answered).unwrap();
    let position = by_id(&qs, &eights[0].id.replace("function", "position"));
    assert_eq!(position.default, Some(json!("2"))); // Ø8 - M6, the largest bolt it clears
}

// test_rules.py::test_a_coarse_series_hole_defaults_to_clearance
#[test]
fn a_coarse_series_hole_defaults_to_clearance() {
    let qs = questions(&analysis(SPOOL), &Map::new()).unwrap();
    let tens = qs
        .iter()
        .find(|q| q.id.starts_with("hole.function") && q.prompt.contains("Ø10 holes"))
        .unwrap();
    assert_eq!(tens.default, Some(json!("clearance:M8")));
    assert_eq!(tens.attention, "routine");
}

// test_rules.py::test_a_bore_is_not_offered_as_clearance
#[test]
fn a_bore_is_not_offered_as_clearance() {
    let qs = questions(&analysis(SPOOL), &Map::new()).unwrap();
    let bore = by_id(&qs, "hole.function:holes:34.55");
    assert!(!bore.options.iter().any(|o| o.starts_with("clearance")));
}

/// A 300 x 200 x 10 plate with `distinct` holes of different sizes and `alike` identical Ø6.6
/// holes, as an analysis: no part of the corpus has more than 85 holes.
fn many_holes(distinct: usize, alike: usize) -> Value {
    let plane = |id: usize, centroid: [f64; 3], direction: [f64; 3], area: f64| {
        json!({"id": id, "kind": "plane", "area": area, "centroid": centroid,
               "direction": direction})
    };
    let mut faces = vec![
        plane(0, [0.0, 0.0, 5.0], [0.0, 0.0, 1.0], 60000.0),
        plane(1, [0.0, 0.0, -5.0], [0.0, 0.0, -1.0], 59000.0),
        plane(2, [150.0, 0.0, 0.0], [1.0, 0.0, 0.0], 2000.0),
        plane(3, [-150.0, 0.0, 0.0], [-1.0, 0.0, 0.0], 2000.0),
        plane(4, [0.0, 100.0, 0.0], [0.0, 1.0, 0.0], 3000.0),
        plane(5, [0.0, -100.0, 0.0], [0.0, -1.0, 0.0], 3000.0),
    ];
    let mut features = Vec::new();
    for k in 0..distinct + alike {
        let id = faces.len();
        let d = if k < distinct {
            1.0 + 0.013 * k as f64
        } else {
            6.6
        };
        let location = [
            -140.0 + 7.0 * (k % 40) as f64,
            -90.0 + 7.0 * (k / 40) as f64,
            0.0,
        ];
        faces.push(json!({"id": id, "kind": "cylinder", "area": 31.4 * d,
                          "centroid": location, "direction": [0.0, 0.0, 1.0],
                          "origin": location, "radius": d / 2.0}));
        features.push(json!({"id": format!("holes:{id}"), "family": "holes",
                             "record": {"diameter": d, "depth": 10.0, "bottom": "through",
                                        "axis": [0.0, 0.0, 1.0], "location": location},
                             "faces": [id]}));
    }
    json!({
        "schema": 1,
        "binding": {"source_sha256": "synthetic", "face_count": faces.len(),
                    "loader_version": 2, "part": 0},
        "faces": faces,
        "features": features,
        "existing": [],
        "part": {},
    })
}

// Not from specify-core: the rules recompute questions to a fixed point, so a part with
// hundreds of holes must not make them quadratic.
#[test]
fn hundreds_of_holes_take_seconds() {
    let analysis = many_holes(250, 250);
    let started = std::time::Instant::now();
    let qs = questions(&analysis, &Map::new()).unwrap();
    assert_eq!(
        qs.iter()
            .filter(|q| q.id.starts_with("hole.function:"))
            .count(),
        251
    );
    let intent =
        apply_with_defaults(&analysis, &answers(json!({"part.material": "steel"})), true).unwrap();
    assert!(intent.requirements.len() > 500);
    specify_core::problems::problems(&analysis, &Map::new()).unwrap();
    let elapsed = started.elapsed().as_secs_f64();
    println!("500 holes: {elapsed:.2} s");
    assert!(elapsed < 10.0, "{elapsed:.1} s");
}
