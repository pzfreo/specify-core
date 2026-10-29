//! Identical features are asked about once, and each still gets its own spec: specify-core's
//! `tests/test_same_as.py`, on the oracle's captured analyses of the corpus spool and NIST
//! CTC-03.

mod common;

use serde_json::{Map, Value, json};
use specify_core::rules::{Question, apply, questions, with_all_defaults};

const SPOOL: &str = "corpus/cadgenbench/flanged_spool_132";

fn analysis(case: &str) -> Value {
    common::case(case)
        .artifact("analysis")
        .expect("a captured analysis")
}

/// `_merged`: the question whose prompt has `text`.
fn merged(analysis: &Value, text: &str) -> Question {
    questions(analysis, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.prompt.contains(text))
        .unwrap_or_else(|| panic!("no question asks {text:?}"))
}

/// `answers | _defaults(analysis, answers)`: Python's dict union, the defaults' values
/// winning.
fn with_defaults(analysis: &Value, answers: Map<String, Value>) -> Map<String, Value> {
    let defaults = with_all_defaults(analysis, &answers).unwrap();
    let mut out = answers;
    for (k, v) in defaults {
        out.insert(k, v);
    }
    out
}

// test_same_as.py::test_identical_bolt_circles_are_one_question
#[test]
fn identical_bolt_circles_are_one_question() {
    let spool = analysis(SPOOL);
    let q = merged(&spool, "Ø8 holes");
    assert_eq!(
        q.prompt,
        "What are the 24× Ø8 holes in 2 identical groups for?"
    );
    assert_eq!(q.parts.len(), 2);
    assert_eq!(q.faces.len(), 24);
    let ids: Vec<Value> = q.parts.iter().map(|(id, _)| json!(id)).collect();
    assert_eq!(q.to_dict()["features"], Value::Array(ids));
    let qs = questions(&spool, &Map::new()).unwrap();
    assert!(!qs.iter().any(|x| x.prompt.contains("12× Ø8")));
}

#[test]
fn outer_counterbores_separate_questions_and_keep_bore_requirements() {
    // quiddity #118: a different outer step must not inherit another group's answer.
    let mut spool = analysis(SPOOL);
    let q = merged(&spool, "Ø8 holes");
    let id = &q.parts[0].0;
    let pattern = spool["features"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["id"] == *id)
        .unwrap();
    let steps = json!([{"diameter":18.0,"depth":2.0},{"diameter":15.0,"depth":1.0}]);
    for hole in pattern["record"]["holes"].as_array_mut().unwrap() {
        hole["outer_steps"] = steps.clone();
    }
    let members = pattern["members"].as_array().unwrap().clone();
    for feature in spool["features"].as_array_mut().unwrap() {
        if members.contains(&feature["id"]) {
            feature["record"]["outer_steps"] = steps.clone();
        }
    }
    let qs = questions(&spool, &Map::new()).unwrap();
    let functions: Vec<_> = qs
        .iter()
        .filter(|q| q.id.starts_with("hole.function:") && q.diameter == Some(8.0))
        .collect();
    assert_eq!(functions.len(), 2);
    assert!(functions.iter().all(|q| q.parts.is_empty()));
    let answers = json!({"part.material":"steel", format!("hole.function:{id}"):"fit:H7"});
    let intent = apply(
        &spool,
        &with_defaults(&spool, answers.as_object().unwrap().clone()),
    )
    .unwrap();
    let fit: Vec<_> = intent
        .requirements
        .iter()
        .filter(|r| r.get("fit") == Some(&json!("H7")))
        .collect();
    assert_eq!(fit.len(), 1);
    assert_eq!(fit[0]["feature"], *id);
    assert_eq!(fit[0]["faces"], json!(q.parts[0].1));
    assert_eq!(fit[0]["nominal"], 8.0);
}

// test_same_as.py::test_each_group_keeps_its_own_size_and_position
#[test]
fn each_group_keeps_its_own_size_and_position() {
    let spool = analysis(SPOOL);
    let q = merged(&spool, "Ø8 holes");
    let mut answers = Map::new();
    answers.insert("part.material".into(), json!("steel"));
    answers.insert(q.id.clone(), json!("clearance"));
    let intent = apply(&spool, &with_defaults(&spool, answers)).unwrap();
    let position = q.id.replace("function", "position");
    for (fid, bores) in &q.parts {
        let mine: Vec<_> = intent
            .requirements
            .iter()
            .filter(|r| r.get("feature") == Some(&json!(fid)))
            .collect();
        let mut kinds: Vec<&str> = mine.iter().map(|r| r["kind"].as_str().unwrap()).collect();
        kinds.sort_unstable();
        assert_eq!(kinds, ["position", "size"]);
        assert!(mine.iter().all(|r| r["faces"] == json!(bores)));
        assert!(
            mine.iter()
                .all(|r| r["question"] == json!(q.id) || r["question"] == json!(position))
        );
    }
}

// test_same_as.py::test_identical_turned_diameters_are_one_question
#[test]
fn identical_turned_diameters_are_one_question() {
    let spool = analysis(SPOOL);
    let q = merged(&spool, "Ø130");
    assert!(q.prompt.starts_with("Is the turned 2× Ø130"));
    let mut answers = Map::new();
    answers.insert("part.material".into(), json!("steel"));
    answers.insert(q.id.clone(), json!("h7"));
    let intent = apply(&spool, &with_defaults(&spool, answers)).unwrap();
    let mut fits: Vec<String> = intent
        .requirements
        .iter()
        .filter(|r| r.get("fit") == Some(&json!("h7")))
        .map(|r| r["feature"].as_str().unwrap().to_string())
        .collect();
    fits.sort();
    let mut parts: Vec<String> = q.parts.iter().map(|(id, _)| id.clone()).collect();
    parts.sort();
    assert_eq!(fits, parts);
}

// test_same_as.py::test_datum_holes_are_never_merged
#[test]
fn datum_holes_are_never_merged() {
    // CTC-03's datums B and C are identical holes, each its own datum.
    let ctc03 = analysis("corpus/nist/nist_ctc_03_asme1_rc");
    let faces = ctc03["faces"].as_array().unwrap();
    let bore = |hole: &str| -> Vec<i64> {
        let feature = ctc03["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == hole)
            .unwrap();
        let d = feature["record"]["diameter"].as_f64().unwrap();
        feature["faces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_i64().unwrap())
            .filter(|&i| {
                let f = faces.iter().find(|f| f["id"] == i).unwrap();
                f["kind"] == "cylinder"
                    && (2.0 * f["radius"].as_f64().unwrap_or(0.0) - d).abs() < 1e-6
            })
            .collect()
    };
    let answers = json!({"datum.A": [18, 28], "datum.B": bore("holes:58"),
                         "datum.C": bore("holes:60")});
    let qs = questions(&ctc03, answers.as_object().unwrap()).unwrap();
    for id in ["hole.function:holes:58", "hole.function:holes:60"] {
        assert!(qs.iter().any(|q| q.id == id), "{id}");
    }
}
