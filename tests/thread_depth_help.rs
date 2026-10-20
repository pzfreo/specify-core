//! The blind thread depth is discoverable before an error, and visible when accepting defaults.
mod common;

use serde_json::{Map, Value, json};
use specify_core::{api, choices, rules, serve::Server};

fn string_post() -> Value {
    common::case("corpus/gramel/string_post")
        .artifact("analysis")
        .unwrap()
}

fn hole(analysis: &Value) -> rules::Question {
    rules::questions(analysis, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id == "hole.function:holes:17.19")
        .unwrap()
}

#[test]
fn blind_tap_help_gives_valid_syntax_and_the_summary_shows_the_computed_depth() {
    let a = string_post();
    let q = hole(&a);
    assert!(q.why.contains("\"M2 tapped, 2.5 mm full thread\""));
    assert!(q.why.contains("drill depth minus 3 pitches"));
    assert!(q.basis.contains("Default minimum full thread: 2.5 mm."));
    assert_eq!(q.default, Some(json!("tapped:M2")));
    let choice = q
        .choices
        .iter()
        .find(|c| c["value"] == "tapped:M2")
        .unwrap();
    assert_eq!(choice["code"], "M2 tapped, 2.5 mm full thread");
    assert_eq!(
        choice["callout"],
        "M2×0.4-6H, 2.5 mm min full thread, ⌀1.6 tap drill"
    );
    let typed = choices::interpret(
        &q.id,
        "M2 tapped, 2.5 mm full thread",
        &q.options,
        q.diameter,
        q.depth,
    )
    .unwrap();
    assert_eq!(typed, "tapped:M2@2.5");
    let review = api::review(&a, &Map::new()).unwrap();
    let presented = review["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == q.id)
        .unwrap();
    assert_eq!(presented["why"], q.why);
    assert_eq!(presented["choices"], json!(q.choices));
    // #20: help remains visible while the proposed tap is unanswered, but its thread PMI
    // (and the depth being checked) appears only after that suggestion is accepted.
    assert!(
        rules::preview(&a, &Map::new())
            .unwrap()
            .requirements
            .is_empty()
    );
    let given = Map::from_iter([(q.id.clone(), q.default.clone().unwrap())]);
    let intent = rules::preview(&a, &given).unwrap();
    assert_eq!(
        intent
            .requirements
            .iter()
            .find(|r| r["kind"] == "thread")
            .unwrap()["full_thread"],
        2.5
    );
}

#[test]
fn interpretation_reports_implicit_and_explicit_depths_without_changing_the_stored_value() {
    let a = string_post();
    let q = hole(&a);
    let mut server = Server::default();
    for (text, value, code) in [
        ("M2", "tapped:M2", "M2 tapped, 2.5 mm full thread"),
        (
            "M2 tapped, 2.3 mm full thread",
            "tapped:M2@2.3",
            "M2 tapped, 2.3 mm full thread",
        ),
    ] {
        let reply = server.reply(&json!({"id": 1, "op": "interpret", "analysis": a, "answers": {}, "question": q.id, "text": text}));
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["value"], value);
        assert_eq!(reply["result"]["code"], code);
    }
}

#[test]
fn through_and_unknown_depth_holes_do_not_promise_a_blind_thread_depth() {
    for bottom in ["through", "unknown_depth"] {
        let mut a = string_post();
        for f in a["features"].as_array_mut().unwrap() {
            if f["id"] == "holes:17.19" {
                if bottom == "through" {
                    f["record"]["bottom"] = json!("through");
                } else {
                    f["record"]["depth"] = Value::Null;
                }
            }
        }
        let q = hole(&a);
        assert!(q.depth.is_none());
        assert!(!q.why.contains("full-thread depth"));
        let choice = q
            .choices
            .iter()
            .find(|c| c["value"] == "tapped:M2")
            .unwrap();
        assert_eq!(choice["code"], "M2 tapped");
        assert!(
            !choice["callout"]
                .as_str()
                .unwrap()
                .contains("min full thread")
        );
    }
}

#[test]
fn the_requested_five_mm_syntax_and_depth_limits_are_preserved() {
    let a = string_post();
    let q = hole(&a);
    assert_eq!(
        choices::interpret(
            &q.id,
            "M2 tapped, 5 mm full thread",
            &q.options,
            q.diameter,
            Some(6.5)
        )
        .unwrap(),
        "tapped:M2@5"
    );
    assert!(
        choices::interpret(
            &q.id,
            "M2 tapped, 5 mm full thread",
            &q.options,
            q.diameter,
            q.depth
        )
        .is_err()
    );
    assert_eq!(
        choices::with_default_full_thread("tapped:M2", Some(3.8)),
        "tapped:M2@2.5"
    );
    assert_eq!(
        choices::with_default_full_thread("tapped:M2@2.3", Some(4.0)),
        "tapped:M2@2.3"
    );
}
