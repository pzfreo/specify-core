//! The edit list (src/edits.rs): parsing with paths, all-or-nothing validation against a file
//! state, contradictions, and 0.2 answers files read as fills.

use std::path::Path;
use std::process::Command;

use serde_json::{Map, Value, json};
use specify_pmi::edits::{self, EditList, FileState, KindState, MemoryState, Number, Requirement};

fn stated(value: Value) -> KindState {
    KindState::Stated { value }
}

/// A small file: hole 12's position is a gap, hole 14's is stated 0.2 (#21), the coating is
/// decided none, hole 3's size is a gap with a suggestion, #30 is file PMI on no feature.
/// (#23's feature is a gap: the requirement lookup and the kind lookup are independent here.)
fn state() -> MemoryState {
    let mut s = MemoryState::default();
    s.kinds.insert(
        "hole.position:holes:12".into(),
        KindState::Gap {
            suggestion: Some(json!("0.25")),
        },
    );
    s.kinds
        .insert("hole.position:holes:14".into(), stated(json!("0.2")));
    s.kinds
        .insert("hole.function:holes:14".into(), stated(json!("tapped:M5")));
    s.kinds.insert(
        "part.coating".into(),
        KindState::DecidedNone {
            value: json!("none"),
        },
    );
    s.kinds.insert(
        "hole.size:holes:3".into(),
        KindState::Gap {
            suggestion: Some(json!("general")),
        },
    );
    s.kinds.insert(
        "datum.A.flatness".into(),
        KindState::Gap { suggestion: None },
    );
    s.requirements.insert(
        21,
        Requirement {
            tolerance: "0.2".into(),
            feature: Some("hole.position:holes:14".into()),
        },
    );
    s.requirements.insert(
        17,
        Requirement {
            tolerance: "±0.1".into(),
            feature: Some("hole.function:holes:14".into()),
        },
    );
    s.requirements.insert(
        23,
        Requirement {
            tolerance: "0.2".into(),
            feature: Some("hole.size:holes:3".into()),
        },
    );
    s.requirements.insert(
        30,
        Requirement {
            tolerance: "±0.2".into(),
            feature: None,
        },
    );
    s
}

fn list(value: Value) -> EditList {
    edits::parse_value(value).expect("a well-formed list")
}

fn entries(outcome: &edits::Outcome) -> Vec<&str> {
    outcome.failures.iter().map(|f| f.entry.as_str()).collect()
}

#[test]
fn a_good_list_is_all_applicable() {
    let l = list(json!({
        "fill": {"hole.position:holes:12": "0.2"},
        "change": {"hole.position:holes:14": {"to": "0.1", "expect": "0.2"}},
        "items": {"remove": ["#30"]},
        "none": ["datum.A.flatness"],
        "accept_suggestions": ["hole.size:holes:3"]
    }));
    let outcome = edits::validate(&l, &state());
    assert!(outcome.applicable, "{}", outcome.message());
    assert!(outcome.checked_against_file);
    assert_eq!(outcome.entries, 5);
    assert!(outcome.failures.is_empty());
    assert!(
        outcome
            .message()
            .starts_with("All 5 entries can be applied.")
    );
}

#[test]
fn every_failure_kind_is_listed_at_once_and_nothing_is_written() {
    let l = list(json!({
        "fill": {
            "hole.position:holes:14": "0.3",
            "hole.position:holes:99": "0.1"
        },
        "change": {
            "hole.function:holes:14": {"to": "tapped:M6", "expect": "tapped:M4"},
            "hole.position:holes:12": {"to": "0.1", "expect": "0.2"}
        },
        "items": {
            "change": {
                "#30": {"tolerance": "±0.1", "expect": "±0.2"},
                "#23": {"tolerance": "0.1", "expect": "0.5"}
            },
            "remove": ["#404"]
        },
        "none": ["part.nothing"],
        "accept_suggestions": ["datum.A.flatness"]
    }));
    let outcome = edits::validate(&l, &state());
    assert!(!outcome.applicable);
    let failing = entries(&outcome);
    for expected in [
        r#"fill["hole.position:holes:14"]"#,
        r#"fill["hole.position:holes:99"]"#,
        r#"change["hole.function:holes:14"]"#,
        r#"change["hole.position:holes:12"]"#,
        r##"items.change["#30"]"##,
        r##"items.change["#23"]"##,
        r##"items.remove["#404"]"##,
        r#"none["part.nothing"]"#,
        r#"accept_suggestions["datum.A.flatness"]"#,
    ] {
        assert!(failing.contains(&expected), "{expected} not in {failing:?}");
    }
    assert_eq!(failing.len(), 9, "{failing:?}");
    let reason = |entry: &str| {
        outcome
            .failures
            .iter()
            .find(|f| f.entry == entry)
            .map(|f| f.reason.clone())
            .unwrap()
    };
    // A differing fill says what the file states and how to change it.
    assert!(reason(r#"fill["hole.position:holes:14"]"#).contains("already states \"0.2\""));
    // A stale expect names the file's value.
    assert!(
        reason(r#"change["hole.function:holes:14"]"#).contains("the file states \"tapped:M5\"")
    );
    // The unsupported item change says so plainly.
    assert!(reason(r##"items.change["#30"]"##).contains("not supported yet"));
    assert!(reason(r##"items.remove["#404"]"##).contains("no requirement numbered #404"));
    assert!(reason(r#"fill["hole.position:holes:99"]"#).contains("no feature-and-kind"));
    let message = outcome.message();
    assert!(message.starts_with("Nothing was written."), "{message}");
    assert!(message.contains("all or nothing"));
    assert!(message.contains("9 of its 9 entries cannot be applied"));
    for f in &outcome.failures {
        assert!(message.contains(&f.entry));
    }
    let shown = outcome.to_json();
    assert_eq!(shown["written"], json!(false));
    assert_eq!(shown["failures"].as_array().unwrap().len(), 9);
}

#[test]
fn one_bad_entry_refuses_the_whole_list() {
    let l = list(json!({
        "fill": {"hole.position:holes:12": "0.2"},
        "change": {"hole.position:holes:14": {"to": "0.1", "expect": "0.3"}}
    }));
    let outcome = edits::validate(&l, &state());
    assert!(!outcome.applicable);
    assert_eq!(entries(&outcome), [r#"change["hole.position:holes:14"]"#]);
    assert!(
        outcome
            .message()
            .contains("1 of its 2 entries cannot be applied")
    );
}

#[test]
fn a_fill_equal_to_what_the_file_states_does_nothing() {
    let l = list(json!({
        "fill": {"hole.position:holes:14": "0.2", "hole.function:holes:14": " tapped:M5 "},
        "none": ["part.coating"]
    }));
    let outcome = edits::validate(&l, &state());
    assert!(outcome.applicable, "{}", outcome.message());
    assert_eq!(
        outcome.unchanged,
        [
            r#"fill["hole.function:holes:14"]"#,
            r#"fill["hole.position:holes:14"]"#,
            r#"none["part.coating"]"#
        ]
    );
    // A number given for a stated text is the same value.
    let l = list(json!({"fill": {"hole.position:holes:14": 0.2}}));
    assert!(edits::validate(&l, &state()).applicable);
}

#[test]
fn contradictions_in_one_list_are_refused() {
    let l = list(json!({
        "fill": {"hole.position:holes:12": "0.2"},
        "none": ["hole.position:holes:12"],
        "accept_suggestions": ["hole.position:holes:12"],
        "items": {"change": {"#21": {"tolerance": "0.1", "expect": "0.2"}}, "remove": ["#21"]}
    }));
    // Found without the file.
    let alone = edits::validate_alone(&l);
    assert!(!alone.checked_against_file && !alone.applicable);
    assert_eq!(
        entries(&alone),
        [
            r#"fill["hole.position:holes:12"] and none["hole.position:holes:12"] and accept_suggestions["hole.position:holes:12"]"#,
            r##"items.change["#21"] and items.remove["#21"]"##,
        ]
    );
    // And with it, alongside the per-entry checks.
    let outcome = edits::validate(&l, &state());
    assert_eq!(outcome.failures.len(), 2, "{:?}", entries(&outcome));
}

#[test]
fn an_item_and_its_feature_and_kind_in_one_list_contradict() {
    let l = list(json!({
        "change": {"hole.position:holes:14": {"to": "0.1", "expect": "0.2"}},
        "items": {"change": {"#21": {"tolerance": "0.1", "expect": "0.2"}}}
    }));
    let outcome = edits::validate(&l, &state());
    assert_eq!(
        entries(&outcome),
        [r##"items.change["#21"] and change["hole.position:holes:14"]"##]
    );
    // Removing a requirement and deciding its feature-and-kind none is allowed.
    let l = list(json!({
        "items": {"remove": ["#21"]},
        "none": ["hole.position:holes:14"]
    }));
    let outcome = edits::validate(&l, &state());
    assert!(outcome.applicable, "{}", outcome.message());
    // Without the removal, none on a stated feature-and-kind is refused.
    let l = list(json!({"none": ["hole.position:holes:14"]}));
    let outcome = edits::validate(&l, &state());
    assert_eq!(entries(&outcome), [r#"none["hole.position:holes:14"]"#]);
}

#[test]
fn leaving_something_out_addresses_nothing() {
    let outcome = edits::validate(&EditList::default(), &state());
    assert!(outcome.applicable);
    assert_eq!(outcome.entries, 0);
}

#[test]
fn schema_errors_name_their_path() {
    let failures = edits::parse(
        r##"{
          "fill": {"a.b": null},
          "change": {"hole.position:holes:14": {"to": "0.1", "expected": "0.2"}},
          "items": {"change": {"21": {"tolerance": "0.1", "expect": "0.2"},
                               "#22": {"tolerance": "0.1"}},
                    "remove": ["#17", 18], "delete": []},
          "none": ["part.coating", ""],
          "accept_suggestion": []
        }"##,
    )
    .unwrap_err();
    let at = |entry: &str| {
        failures
            .iter()
            .find(|f| f.entry == entry)
            .unwrap_or_else(|| panic!("no failure at {entry}: {failures:?}"))
            .reason
            .clone()
    };
    assert!(at(r#"fill["a.b"]"#).contains("null"));
    assert!(at(r#"change["hole.position:holes:14"]"#).contains("unknown field `expected`"));
    assert!(at(r#"items.change["21"]"#).contains("#N"));
    assert!(at(r##"items.change["#22"]"##).contains("missing field `expect`"));
    assert!(at("items.remove[1]").contains("#N"));
    assert!(at("items.delete").contains("unknown field"));
    assert!(at("none[1]").contains("feature-and-kind id"));
    assert!(at("accept_suggestion").contains("unknown field"));
    assert_eq!(failures.len(), 8, "{failures:?}");

    // Not JSON at all, and JSON that is not an object.
    let f = edits::parse("{").unwrap_err();
    assert!(f[0].reason.starts_with("not valid JSON"));
    let f = edits::parse("[]").unwrap_err();
    assert!(f[0].reason.contains("JSON object"));

    // A malformed list, checked: nothing written, every path named.
    let outcome = edits::check(r#"{"fills": {}}"#, Some(&state()));
    assert!(!outcome.well_formed && !outcome.applicable);
    assert!(outcome.message().starts_with("Nothing was written."));
    assert!(outcome.message().contains("- fills: unknown field"));
}

#[test]
fn a_0_2_answers_file_becomes_fills_and_nones() {
    let answers: Map<String, Value> = serde_json::from_value(json!({
        "_specify": {"evidence": {}},
        "_specify_characteristic_numbers": {},
        "hole.position:holes:14": "0.2",
        "part.coating": "None (as machined)",
        "part.heat_treatment": "None",
        "datum.A.flatness": "none",
        "part.notes": [],
        "datum.A": [4],
        "part.material": "Nonel steel"
    }))
    .unwrap();
    let l = edits::from_answers(&answers);
    assert_eq!(
        l.none.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "datum.A.flatness",
            "part.coating",
            "part.heat_treatment",
            "part.notes"
        ]
    );
    assert_eq!(
        l.fill.keys().map(String::as_str).collect::<Vec<_>>(),
        ["datum.A", "hole.position:holes:14", "part.material"]
    );
    assert!(l.change.is_empty() && l.items.change.is_empty() && l.accept_suggestions.is_empty());
    // `read` takes a flat answers file as one.
    let read = edits::read(&Value::Object(answers).to_string()).unwrap();
    assert_eq!(read, l);
}

/// The real saved answers of `tests/fixtures`, against a file that states exactly them: clean,
/// every fill a no-op; one stated differently refuses the whole list, naming it.
#[test]
fn a_real_0_2_answers_file_applies_cleanly_when_it_matches_the_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for name in [
        "revisions/symmetric_tie.answers.json",
        "bom/legacy-plate-answers.json",
    ] {
        let text = std::fs::read_to_string(root.join(name)).unwrap();
        let answers: Map<String, Value> = serde_json::from_str(&text).unwrap();
        let mut file = MemoryState::default();
        for (id, value) in answers.iter().filter(|(id, _)| !id.starts_with('_')) {
            let state = if edits::is_none_answer(value) {
                KindState::DecidedNone {
                    value: value.clone(),
                }
            } else {
                stated(value.clone())
            };
            file.kinds.insert(id.clone(), state);
        }
        let outcome = edits::check(&text, Some(&file));
        assert!(outcome.applicable, "{name}: {}", outcome.message());
        assert_eq!(outcome.unchanged.len(), outcome.entries, "{name}");

        let (changed, _) = answers
            .iter()
            .find(|(id, v)| !id.starts_with('_') && !edits::is_none_answer(v))
            .unwrap();
        file.kinds
            .insert(changed.clone(), stated(json!("something else")));
        let outcome = edits::check(&text, Some(&file));
        assert!(!outcome.applicable);
        assert_eq!(entries(&outcome), [format!("fill[{}]", json!(changed))]);
        assert!(outcome.message().starts_with("Nothing was written."));
    }
}

#[test]
fn an_answers_file_into_a_file_where_its_ids_are_gaps_fills_them() {
    let text = r#"{"hole.position:holes:12": "0.2", "datum.A.flatness": "none"}"#;
    let outcome = edits::check(text, Some(&state()));
    assert!(outcome.applicable, "{}", outcome.message());
    assert!(outcome.unchanged.is_empty());
}

#[test]
fn characteristic_numbers_are_written_with_a_hash() {
    assert_eq!(Number::try_from("#17".to_owned()), Ok(Number(17)));
    assert!(Number::try_from("17".to_owned()).is_err());
    assert!(Number::try_from("#".to_owned()).is_err());
    assert!(Number::try_from("#-1".to_owned()).is_err());
    assert_eq!(Number(5).to_string(), "#5");
}

#[test]
fn a_file_state_is_used_through_the_trait() {
    // The interface the feature table implements: lookup by id and by number.
    let s: &dyn FileState = &state();
    assert_eq!(
        s.kind("part.coating"),
        Some(KindState::DecidedNone {
            value: json!("none")
        })
    );
    assert_eq!(s.kind("nope"), None);
    assert_eq!(s.requirement(30).unwrap().feature, None);
    assert!(s.requirement(31).is_none());
}

fn cli(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_specify-pmi"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn the_cli_checks_an_edit_list_and_writes_nothing() {
    let dir = std::env::temp_dir().join(format!("specify-edits-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let step = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/spindle.step");
    let step = step.to_str().unwrap();

    // Checked against the file's feature table (piece 2): its ids are the file's.
    let good = dir.join("good.json");
    std::fs::write(
        &good,
        r#"{"fill": {"part.material": "6082-T6"}, "none": ["part.coating"]}"#,
    )
    .unwrap();
    let (code, stdout, stderr) = cli(&["write", step, good.to_str().unwrap(), "--check"]);
    assert_eq!(code, 0, "{stderr}");
    let shown: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(shown["well_formed"], json!(true));
    assert_eq!(shown["checked_against_file"], json!(true));
    assert_eq!(shown["applicable"], json!(true));
    assert_eq!(shown["written"], json!(false));
    let unknown = dir.join("unknown.json");
    std::fs::write(&unknown, r#"{"fill": {"a.b": "1"}}"#).unwrap();
    let (code, _, stderr) = cli(&["write", step, unknown.to_str().unwrap(), "--check"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("no feature-and-kind"), "{stderr}");

    let bad = dir.join("bad.json");
    std::fs::write(
        &bad,
        r#"{"fill": {"a.b": "1"}, "none": ["a.b"], "items": {"remove": [3]}}"#,
    )
    .unwrap();
    let (code, stdout, stderr) = cli(&["write", step, bad.to_str().unwrap(), "--check"]);
    assert_eq!(code, 2);
    let shown: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(shown["well_formed"], json!(false));
    assert!(stderr.contains("Nothing was written."), "{stderr}");
    assert!(stderr.contains("items.remove[0]"), "{stderr}");

    let contradictory = dir.join("contradictory.json");
    std::fs::write(
        &contradictory,
        r#"{"fill": {"part.material": "1"}, "none": ["part.material"]}"#,
    )
    .unwrap();
    let (code, _, stderr) = cli(&["write", step, contradictory.to_str().unwrap(), "--check"]);
    assert_eq!(code, 2);
    assert!(
        stderr.contains(r#"fill["part.material"] and none["part.material"]"#),
        "{stderr}"
    );

    // --check takes one list, and no output.
    let (code, _, _) = cli(&["write", step, "a", "b", "--check"]);
    assert_eq!(code, 2);
    let (code, _, _) = cli(&["write", step, good.to_str().unwrap(), "--check", "-o", "x"]);
    assert_eq!(code, 2);
    let _ = std::fs::remove_dir_all(&dir);
}
