//! A turned diameter's thread choices (stream S3, issue #3): the diameter question asks whether
//! it is "a fit, a thread or knurled", so it offers every ISO 261 thread at that size, coarse
//! and fine, as choices beside the fits and knurls; choosing one writes the same AP242 thread
//! as typing it into the free-text box.
//!
//! The part is `tests/fixtures/spindle.step`, a build123d turned spindle (Ø24, Ø15 and Ø10
//! steps), analysed here by the Rust engine.

mod common;

use std::path::PathBuf;

use haecceity::p21::Document;
use haecceity::pmi;
use haecceity::step::read_part_definitions;
use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::choices;
use specify_core::part::load;
use specify_core::rules::{Question, apply_with_defaults, questions};
use specify_core::standards;

fn spindle() -> PathBuf {
    common::fixtures().join("spindle.step")
}

fn analysis() -> Value {
    api::analyse_file(&spindle(), None).expect("the spindle analyses")
}

/// The diameter question of the turned step of diameter `d`.
fn diameter_question(analysis: &Value, d: f64) -> Question {
    questions(analysis, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id.starts_with("diameter.fit:") && q.diameter == Some(d))
        .unwrap_or_else(|| panic!("no diameter question for Ø{d}"))
}

/// The question's thread options, in order.
fn threads(q: &Question) -> Vec<&str> {
    q.options
        .iter()
        .map(String::as_str)
        .filter(|o| o.starts_with("thread:"))
        .collect()
}

/// The question's choice for `value`.
fn choice<'q>(q: &'q Question, value: &str) -> &'q Value {
    q.choices
        .iter()
        .find(|c| c["value"] == value)
        .unwrap_or_else(|| panic!("no choice {value}"))
}

/// What typing `text` into the question's free-text box answers.
fn typed(q: &Question, text: &str) -> String {
    choices::interpret(&q.id, text, &q.options, q.diameter, q.depth)
        .unwrap_or_else(|e| panic!("{text:?} refused: {e}"))
}

/// The external threads haecceity reads back from a file written with `answer` for `qid`.
fn written_threads(analysis: &Value, qid: &str, answer: &str, name: &str) -> Vec<pmi::Thread> {
    let mut given = Map::new();
    given.insert("part.material".into(), json!("steel"));
    given.insert(qid.into(), json!(answer));
    let intent = apply_with_defaults(analysis, &given, true).expect("an intent");
    let dir = std::env::temp_dir()
        .join("specify-core-thread-choices")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join(format!("{name}.step"));
    let loaded = load(&spindle(), None).unwrap();
    let report = api::write(&loaded, &intent, &out, None).expect("written");
    assert!(
        report
            .written
            .iter()
            .any(|w| w.starts_with("external thread")),
        "{:?}",
        report.written
    );
    let bytes = std::fs::read(&out).unwrap();
    let defs = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    // The spindle's M5 hole is tapped by default: only the external threads are the
    // diameter's.
    pmi::read(&doc, &defs)
        .unwrap()
        .parts
        .swap_remove(0)
        .threads
        .into_iter()
        .filter(|t| matches!(t.side, pmi::ThreadSide::External))
        .collect()
}

#[test]
fn the_spindle_ten_offers_m10_coarse_and_its_fine_pitches() {
    let analysis = analysis();
    let ten = diameter_question(&analysis, 10.0);
    assert_eq!(
        threads(&ten),
        [
            "thread:M10",
            "thread:M10x1.25",
            "thread:M10x1",
            "thread:M10x0.75"
        ]
    );
    let coarse = choice(&ten, "thread:M10");
    assert_eq!(coarse["purpose"], "thread");
    assert_eq!(coarse["code"], "M10 thread");
    assert_eq!(coarse["callout"], "M10×1.5-6g");
    let fine = choice(&ten, "thread:M10x1.25");
    assert_eq!(fine["purpose"], "thread");
    assert_eq!(fine["code"], "M10×1.25 thread");
    assert_eq!(fine["callout"], "M10×1.25-6g");
    // Still beside the fits and knurls, and the general tolerance is still the default.
    for option in ["general", "h6", "g6", "knurl:straight", "knurl:diamond"] {
        assert!(ten.options.iter().any(|o| o == option), "{option}");
    }
    assert_eq!(ten.default, Some(json!("general")));
}

#[test]
fn choosing_a_thread_writes_what_typing_it_writes() {
    let analysis = analysis();
    let ten = diameter_question(&analysis, 10.0);
    for (text, option, qualifier) in [
        ("M10", "thread:M10", "M10x1.5"),
        ("M10x1.5", "thread:M10", "M10x1.5"),
        ("M10×1.25", "thread:M10x1.25", "M10x1.25"),
        ("m10 x 1", "thread:M10x1", "M10x1"),
    ] {
        // The free-text box reads the typed thread as the offered choice itself ...
        let value = typed(&ten, text);
        assert_eq!(value, option, "{text}");
        assert!(ten.options.contains(&value), "{value}");
        // ... so both write the one external thread, with its designation and class.
        let name = option.replace(':', "-");
        let written = written_threads(&analysis, &ten.id, &value, &name);
        assert_eq!(written.len(), 1, "{option}");
        let thread = &written[0];
        assert_eq!(thread.qualifier.as_deref(), Some(qualifier), "{option}");
        assert_eq!(thread.fit_class, standards::EXTERNAL_THREAD_CLASS);
        assert_eq!(thread.form, "M");
    }
    // The intent of the fine choice states its own pitch, not the coarse one.
    let mut given = Map::new();
    given.insert("part.material".into(), json!("steel"));
    given.insert(ten.id.clone(), json!("thread:M10x1.25"));
    let intent = apply_with_defaults(&analysis, &given, true).unwrap();
    let thread = intent
        .requirements
        .iter()
        .find(|r| r["kind"] == "thread" && r["side"] == "external")
        .expect("an external thread requirement");
    assert_eq!(thread["designation"], "M10");
    assert_eq!(thread["pitch"], 1.25);
    assert_eq!(thread["spec"], "M10x1.25");
    assert_eq!(thread["class"], "6g");
    assert_eq!(thread["nominal"], 10.0);
}

#[test]
fn a_diameter_with_no_metric_coarse_size_offers_only_what_iso_261_gives() {
    let analysis = analysis();
    // Ø15 is an ISO 261 size with fine pitches only (a bearing locknut's M15×1): no coarse
    // choice, and a bare M15 typed is refused with the pitches it takes.
    let fifteen = diameter_question(&analysis, 15.0);
    assert_eq!(threads(&fifteen), ["thread:M15x1.5", "thread:M15x1"]);
    let refused = choices::interpret(&fifteen.id, "M15", &fifteen.options, fifteen.diameter, None)
        .unwrap_err();
    assert!(refused.0.contains("M15×1.5"), "{refused}");
    // A pitch ISO 261 does not give is refused, not written.
    assert!(choices::interpret(&fifteen.id, "M15x2", &fifteen.options, Some(15.0), None).is_err());
}

#[test]
fn a_diameter_at_no_metric_size_offers_no_thread() {
    // Ø13 is no ISO 261 size: the spindle's Ø10 step made Ø13 is offered fits and knurls only.
    let mut analysis = analysis();
    let ten = diameter_question(&analysis, 10.0);
    let fid = ten.feature.clone().unwrap();
    for f in analysis["features"].as_array_mut().unwrap() {
        if f["id"] == fid.as_str() {
            f["record"]["diameter"] = json!(13.0);
        }
    }
    let thirteen = diameter_question(&analysis, 13.0);
    assert!(threads(&thirteen).is_empty(), "{:?}", thirteen.options);
    assert!(
        thirteen.choices.iter().all(|c| c["purpose"] != "thread"),
        "{:?}",
        thirteen.choices
    );
    assert!(thirteen.options.iter().any(|o| o == "knurl:diamond"));
    assert!(standards::external_threads(13.0).is_empty());
    assert!(standards::external_threads(62.5).is_empty());
}

#[test]
fn thread_match_is_within_point_zero_five_mm_without_snapping_other_sizes() {
    for d in [9.95, 9.951, 10.0, 10.049, 10.05] {
        assert_eq!(standards::external_threads(d)[0], "M10");
    }
    for d in [9.949, 10.051, 13.0] {
        assert!(standards::external_threads(d).is_empty(), "Ø{d}");
    }
}
