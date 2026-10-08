//! The oracle harness itself (`tests/common`), on small hand-made values, and the committed
//! oracle's coherence: every case's input is the file it was captured from.

mod common;

use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};

use common::{
    Difference, KnownDifference, Tolerances, VERDICTS, case, cases, check_known_with, compare,
    decompressed, explained_with, glob_match, is_error, load_known, scenarios, sha256,
};
use serde_json::{Value, json};

fn known(case: &str, path: &str, verdict: &str) -> KnownDifference {
    KnownDifference {
        case: case.into(),
        path: path.into(),
        pattern: "a pattern".into(),
        verdict: verdict.into(),
        evidence: "what was checked".into(),
    }
}

fn difference(case: &str, path: &str) -> Difference {
    Difference {
        case: case.into(),
        path: path.into(),
        python: Some(json!(1)),
        rust: Some(json!(2)),
    }
}

/// The panic message of `f`, which must panic.
fn panics(f: impl FnOnce()) -> String {
    let err = catch_unwind(AssertUnwindSafe(f)).expect_err("expected a panic");
    err.downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_default()
}

fn paths(differences: &[Difference]) -> Vec<&str> {
    differences.iter().map(|d| d.path.as_str()).collect()
}

#[test]
fn globs_match_any_run_of_characters() {
    assert!(glob_match("faces[*].area", "faces[12].area"));
    assert!(glob_match("faces[*].area", "faces[].area"));
    assert!(!glob_match("faces[*].area", "faces[1].centroid"));
    assert!(glob_match("corpus/*", "corpus/mfcadpp/1000"));
    assert!(!glob_match("corpus/*", "parts/plate"));
    assert!(glob_match("*", ""));
    assert!(glob_match("a*b*c", "a-b-b-c"));
    assert!(!glob_match("a*b*c", "a-c-b"));
    assert!(glob_match("parts/assembly#*", "parts/assembly#1"));
    assert!(glob_match("exact", "exact") && !glob_match("exact", "exactly"));
}

#[test]
fn equal_values_have_no_differences_whatever_their_key_order() {
    let python = json!({"a": 1, "b": [1.5, "x", null, true], "c": {"d": []}});
    let rust = json!({"c": {"d": []}, "b": [1.5, "x", null, true], "a": 1});
    assert!(compare("c", &python, &rust, &Tolerances::default()).is_empty());
    // An integer and the same float are one number in JSON.
    assert!(compare("c", &json!(30), &json!(30.0), &Tolerances::default()).is_empty());
}

#[test]
fn numbers_are_equal_within_relative_1e_9_by_default() {
    let t = Tolerances::default();
    assert!(compare("c", &json!(100.0), &json!(100.0 + 5e-8), &t).is_empty());
    let found = compare("c", &json!({"x": 100.0}), &json!({"x": 100.0 + 2e-7}), &t);
    assert_eq!(paths(&found), ["x"]);
    // No absolute floor by default: 0 and 1e-17 differ.
    assert_eq!(compare("c", &json!(0.0), &json!(1e-17), &t).len(), 1);
    assert!(compare("c", &json!(-0.0), &json!(0.0), &t).is_empty());
}

#[test]
fn per_path_overrides_take_the_first_matching_glob() {
    let t = Tolerances::default()
        .with("faces[*].area", 1e-6, 0.0)
        .with("faces[*].*", 0.0, 1e-3);
    let python = json!({"faces": [{"area": 1000.0, "centroid": [0.0, 1.0]}], "area": 1000.0});
    let rust =
        json!({"faces": [{"area": 1000.0005, "centroid": [0.0009, 1.0]}], "area": 1000.0005});
    // faces[0].area within 1e-6 relative; centroid within 1e-3 absolute; the top-level area
    // takes the default.
    assert_eq!(paths(&compare("c", &python, &rust, &t)), ["area"]);
}

#[test]
fn missing_and_extra_keys_and_elements_are_reported() {
    let found = compare(
        "parts/plate",
        &json!({"a": 1, "b": 2, "list": [1, 2, 3]}),
        &json!({"a": 1, "c": 3, "list": [1, 2]}),
        &Tolerances::default(),
    );
    let by_path: Vec<(&str, Option<&Value>, Option<&Value>)> = found
        .iter()
        .map(|d| (d.path.as_str(), d.python.as_ref(), d.rust.as_ref()))
        .collect();
    // Python's keys in its order first, then the keys only Rust has.
    assert_eq!(
        by_path,
        [
            ("b", Some(&json!(2)), None),
            ("list[2]", Some(&json!(3)), None),
            ("c", None, Some(&json!(3))),
        ]
    );
    assert!(found.iter().all(|d| d.case == "parts/plate"));
    // Different types differ as a whole.
    let found = compare(
        "c",
        &json!({"x": [1]}),
        &json!({"x": {"0": 1}}),
        &Tolerances::default(),
    );
    assert_eq!(paths(&found), ["x"]);
}

#[test]
fn arrays_can_be_aligned_by_a_key() {
    let python =
        json!({"features": [{"id": "a", "n": 1}, {"id": "b", "n": 2}, {"id": "c", "n": 3}]});
    let rust = json!({"features": [{"id": "b", "n": 2}, {"id": "a", "n": 9}, {"id": "d", "n": 4}]});
    let by_index = compare("c", &python, &rust, &Tolerances::default());
    assert_eq!(by_index.len(), 6, "{by_index:?}");
    let t = Tolerances::default().align("features", "id");
    let found = compare("c", &python, &rust, &t);
    assert_eq!(
        paths(&found),
        ["features[id=a].n", "features[id=c]", "features[id=d]"]
    );
    // A repeated key falls back to the index.
    let twice = json!([{"id": "a"}, {"id": "a"}]);
    let t = Tolerances::default().align("", "id");
    assert!(compare("c", &twice, &twice, &t).is_empty());
}

#[test]
fn known_differences_explain_by_case_and_path_glob() {
    let entries = [
        known("corpus/*", "features[*].faces", "rust-correct"),
        known("parts/plate", "faces[*].area", "equivalent"),
    ];
    let differences = [
        difference("corpus/cadgenbench/flanged_spool_132", "features[3].faces"),
        difference("parts/plate", "faces[0].area"),
    ];
    // Both matched, both entries used: passes.
    check_known_with("test", &entries, &differences);
}

#[test]
fn an_unmatched_difference_fails_naming_it() {
    let entries = [known("parts/*", "faces[*].area", "equivalent")];
    let message = panics(|| {
        check_known_with(
            "test",
            &entries,
            &[
                difference("parts/pin", "faces[1].area"),
                difference("parts/pin", "features"),
            ],
        )
    });
    assert!(
        message.contains("1 difference(s) with no entry"),
        "{message}"
    );
    assert!(message.contains("parts/pin features"), "{message}");
    assert!(!message.contains("stale"), "{message}");
}

#[test]
fn an_entry_that_matches_nothing_is_stale() {
    let entries = [
        known("parts/*", "faces[*].area", "equivalent"),
        known("nist/*", "existing", "rust-correct"),
    ];
    let message = panics(|| {
        check_known_with(
            "test",
            &entries,
            &[difference("parts/pin", "faces[0].area")],
        )
    });
    assert!(message.contains("1 stale entr(ies)"), "{message}");
    assert!(message.contains("case nist/* path existing"), "{message}");
    // With no differences at all, every entry is stale.
    let message = panics(|| check_known_with("test", &entries, &[]));
    assert!(message.contains("2 stale entr(ies)"), "{message}");
}

#[test]
fn verdicts_and_fields_are_validated() {
    let dir = std::env::temp_dir().join(format!("oracle-harness-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, value: Value| {
        let path = dir.join(name);
        std::fs::File::create(&path)
            .unwrap()
            .write_all(value.to_string().as_bytes())
            .unwrap();
        path
    };
    let good = write(
        "good.json",
        json!(VERDICTS
            .iter()
            .map(|v| json!({"case": "*", "path": "x", "pattern": "p", "verdict": v, "evidence": "e"}))
            .collect::<Vec<_>>()),
    );
    assert_eq!(load_known(&good).len(), 5);
    let bad = write(
        "bad.json",
        json!([
            {"case": "*", "path": "x", "pattern": "p", "verdict": "rust-better", "evidence": "e"},
            {"case": "*", "path": "x", "pattern": "p", "verdict": "equivalent", "evidence": ""},
            {"case": "*", "path": "x", "pattern": "p", "verdict": "equivalent"},
            {"case": "*", "path": "x", "pattern": "p", "verdict": "equivalent", "evidence": "e", "note": 1},
        ]),
    );
    let message = panics(|| {
        load_known(&bad);
    });
    assert!(
        message.contains("entry 0: verdict 'rust-better' is not one of"),
        "{message}"
    );
    assert!(
        message.contains("entry 1: 'evidence' must be a non-empty string"),
        "{message}"
    );
    assert!(
        message.contains("entry 2: 'evidence' must be a non-empty string"),
        "{message}"
    );
    assert!(
        message.contains("entry 3: unknown field 'note'"),
        "{message}"
    );
    // A file not written yet has no entries.
    assert!(load_known(&dir.join("absent.json")).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn explained_filters_without_asserting() {
    let entries = [
        known("parts/*", "faces[*].area", "equivalent"),
        known("nist/*", "existing", "rust-correct"),
    ];
    let differences = [
        difference("parts/pin", "faces[0].area"),
        difference("parts/pin", "features"),
    ];
    // The nist entry matches nothing: no stale failure here.
    let left = explained_with(&entries, &differences);
    assert_eq!(left, [difference("parts/pin", "features")]);
    assert!(explained_with(&[], &differences).len() == 2);
}

#[test]
fn a_gz_input_is_decompressed_once_and_checked() {
    let dir = std::env::temp_dir().join(format!("oracle-harness-gz-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bytes = b"ISO-10303-21;\nEND-ISO-10303-21;\n";
    let gz = dir.join("tiny.step.gz");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    std::fs::write(&gz, encoder.finish().unwrap()).unwrap();
    let out = decompressed(&gz, &sha256(bytes));
    assert_eq!(out.file_name().unwrap(), "tiny.step");
    assert_eq!(std::fs::read(&out).unwrap(), bytes);
    assert_eq!(decompressed(&gz, &sha256(bytes)), out, "reused");
    // Bytes that are not the ones expected are refused (a file not yet decompressed here).
    let other = dir.join("other.step.gz");
    std::fs::copy(&gz, &other).unwrap();
    let mismatch = panics(|| {
        decompressed(&other, &"0".repeat(64));
    });
    assert!(
        mismatch.contains("not those the oracle was captured from"),
        "{mismatch}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn every_case_is_bound_to_its_input() {
    let all = cases();
    assert!(all.len() >= 20, "only {} cases captured", all.len());
    let mut sorted = all.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
    sorted.sort();
    assert_eq!(
        sorted,
        all.iter().map(|c| c.name.clone()).collect::<Vec<_>>()
    );
    for case in &all {
        assert_eq!(case.meta["case"], json!(case.name), "{}", case.name);
        let Some(input) = case.input() else { continue };
        // The analysis binding is of the bytes the engines read (decompressed for a .gz).
        let Some(analysis) = case.artifact("analysis") else {
            continue;
        };
        if is_error(&analysis) {
            continue;
        }
        let sha = sha256(&std::fs::read(&input).unwrap());
        assert_eq!(
            analysis["binding"]["source_sha256"],
            json!(sha),
            "{}",
            case.name
        );
    }
}

#[test]
fn the_gzipped_nist_inputs_decompress_to_their_bindings() {
    let ctc = case("nist/nist_ctc_01_asme1_ap242");
    let input = ctc.input().expect("committed");
    assert!(input.extension().is_some_and(|e| e == "stp"));
    assert_eq!(
        sha256(&std::fs::read(&input).unwrap()),
        ctc.meta["decompressed_sha256"].as_str().unwrap()
    );
}

#[test]
fn plates_readback_has_pythons_pmi_and_answers() {
    let plate = case("parts/plate");
    let readback = plate.artifact("readback").expect("readback");
    let first = &readback[0];
    assert!(
        first["existing"].as_array().is_some_and(|e| !e.is_empty()),
        "{first}"
    );
    assert_eq!(first["resume"], plate.meta["answers"]);
    assert!(plate.written_step().is_some());
}

#[test]
fn every_scenario_names_a_case_and_has_a_result() {
    let all = scenarios();
    assert!(!all.is_empty());
    let names: Vec<String> = cases().into_iter().map(|c| c.name).collect();
    for scenario in &all {
        assert!(
            names.contains(&scenario.case),
            "{}: no case {}",
            scenario.name,
            scenario.case
        );
        assert!(
            !scenario.from.is_empty() && !scenario.why.is_empty(),
            "{}",
            scenario.name
        );
        let result = scenario
            .result
            .as_ref()
            .unwrap_or_else(|| panic!("{}: no result", scenario.name));
        for key in ["questions", "open", "problems", "preview", "apply"] {
            assert!(result.get(key).is_some(), "{}: no {key}", scenario.name);
        }
    }
}
