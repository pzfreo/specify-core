//! The rules against the Python oracle: on every captured case and scenario, Python's analysis
//! and answers go into the Rust rules, and their questions (unanswered, and with every
//! default), open questions, problems, preview and applied intent, or the refusal, are
//! compared with Python's own.
//!
//! The inputs are Python's (its analysis, its answers), so the rules are checked independently
//! of recognition: the corpus cases need no corpus here, only their captured analyses.
//! Strings must be byte-identical, numbers equal within 1e-12, and objects' keys in Python's
//! order (a difference in order is reported at `<path>.(key order)`). Every difference is in
//! `tests/fixtures/known_differences/rules.json` with its verdict.
//!
//! Refusals compare by kind and message. Python's `ValueError` and its `SpecError` subclass
//! are both [`Error::Invalid`] here (serve and the command line report both as `invalid`), so
//! both read as `invalid`; an `IncompleteError` keeps its `missing` and `part`.

mod common;

use std::time::Instant;

use common::{
    Difference, Tolerances, cases, check_known, compare, is_error, scenarios,
    without_threads_beyond_python,
};
use serde_json::{Map, Value, json};
use specify_core::problems::problems;
use specify_core::rules::{
    self, Error, Intent, Question, open_questions, preview, questions, with_all_defaults,
};

/// What every case's `apply` answers (the capture's `BASE_ANSWERS`).
fn base_answers() -> Map<String, Value> {
    json!({"part.material": "steel"})
        .as_object()
        .cloned()
        .expect("an object")
}

/// A Rust result as the oracle records it: the value, or `{error, message}` (with `missing`
/// and `part` for questions left open).
fn recorded<T>(result: Result<T, Error>, value: impl FnOnce(T) -> Value) -> Value {
    match result {
        Ok(v) => value(v),
        Err(Error::Invalid(message)) => json!({"error": "invalid", "message": message}),
        Err(Error::Incomplete(e)) => json!({
            "error": "IncompleteError",
            "message": e.to_string(),
            "missing": e.missing,
            "part": e.part,
        }),
    }
}

/// Python's value with each recorded failure's class read as Rust's kind (`ValueError` and
/// `SpecError` are both `invalid`).
fn as_rust_reads(value: &Value) -> Value {
    let mut out = value.clone();
    if is_error(value) {
        if matches!(value["error"].as_str(), Some("ValueError" | "SpecError")) {
            out["error"] = json!("invalid");
        }
    } else if let Value::Object(map) = &mut out {
        for v in map.values_mut() {
            *v = as_rust_reads(v);
        }
    }
    out
}

fn dicts(qs: &[Question]) -> Value {
    Value::Array(qs.iter().map(Question::to_dict).collect())
}

fn open_ids(qs: &[Question], answers: &Map<String, Value>) -> Value {
    Value::Array(
        open_questions(qs, answers)
            .iter()
            .map(|q| Value::from(q.id.as_str()))
            .collect(),
    )
}

fn intent(intent: Intent) -> Value {
    intent.to_dict()
}

/// `_questions` of the capture: `{answers, questions, open}`.
fn asked(analysis: &Value, answers: &Map<String, Value>) -> Value {
    recorded(questions(analysis, answers), |qs| {
        json!({
            "answers": answers,
            "questions": dicts(&qs),
            "open": open_ids(&qs, answers),
        })
    })
}

/// The Rust counterpart of a case's artifacts: questions, problems, preview and apply.
fn case_artifacts(analysis: &Value) -> Vec<(&'static str, Value)> {
    let none = Map::new();
    let defaults = recorded(with_all_defaults(analysis, &none), |merged| {
        asked(analysis, &merged)
    });
    vec![
        (
            "questions",
            json!({"none": asked(analysis, &none), "defaults": defaults}),
        ),
        (
            "problems",
            recorded(problems(analysis, &none), Value::Array),
        ),
        ("preview", recorded(preview(analysis, &none), intent)),
        (
            "apply",
            recorded(
                rules::apply_with_defaults(analysis, &base_answers(), true),
                intent,
            ),
        ),
    ]
}

/// The Rust counterpart of a scenario's result.
fn scenario_result(analysis: &Value, answers: &Map<String, Value>, defaults: bool) -> Value {
    let qs = questions(analysis, answers);
    let (listed, open) = match &qs {
        Ok(qs) => (dicts(qs), open_ids(qs, answers)),
        Err(e) => {
            let failed = recorded::<()>(Err(e.clone()), |_| Value::Null);
            (failed.clone(), failed)
        }
    };
    json!({
        "questions": listed,
        "open": open,
        "problems": recorded(problems(analysis, answers), Value::Array),
        "preview": recorded(preview(analysis, answers), intent),
        "apply": recorded(rules::apply_with_defaults(analysis, answers, defaults), intent),
    })
}

fn tolerances() -> Tolerances {
    Tolerances {
        relative: 1e-12,
        absolute: 1e-12,
        ..Tolerances::default()
    }
}

/// Differences in the order of keys of objects that have the same keys, walked as `compare`
/// walks (arrays by index).
fn key_order(case: &str, path: &str, python: &Value, rust: &Value, out: &mut Vec<Difference>) {
    let at = |key: &str| {
        if path.is_empty() {
            key.to_string()
        } else {
            format!("{path}.{key}")
        }
    };
    match (python, rust) {
        (Value::Object(p), Value::Object(r)) => {
            let (pk, rk): (Vec<&String>, Vec<&String>) = (p.keys().collect(), r.keys().collect());
            let mut ps = pk.clone();
            let mut rs = rk.clone();
            ps.sort();
            rs.sort();
            if ps == rs && pk != rk {
                out.push(Difference {
                    case: case.to_string(),
                    path: at("(key order)"),
                    python: Some(json!(pk)),
                    rust: Some(json!(rk)),
                });
            }
            for (key, pv) in p {
                if let Some(rv) = r.get(key) {
                    key_order(case, &at(key), pv, rv, out);
                }
            }
        }
        (Value::Array(p), Value::Array(r)) => {
            for (i, (pv, rv)) in p.iter().zip(r).enumerate() {
                key_order(case, &format!("{path}[{i}]"), pv, rv, out);
            }
        }
        _ => {}
    }
}

/// The differences of `rust` from `python`, once the threads Rust's diameter questions offer
/// beyond Python's are set aside (stream S3; `common::without_threads_beyond_python`): every
/// other option and choice must equal Python's.
fn differences(case: &str, python: &Value, rust: &Value) -> Vec<Difference> {
    let python = as_rust_reads(python);
    let rust = &without_threads_beyond_python(rust);
    let mut out = compare(case, &python, rust, &tolerances());
    key_order(case, "", &python, rust, &mut out);
    out
}

/// The case's analysis, when Python made one.
fn analysis_of(case: &common::Case) -> Option<Value> {
    case.artifact("analysis").filter(|a| !is_error(a))
}

#[test]
fn rules_match_the_oracle() {
    let started = Instant::now();
    let mut all = Vec::new();
    let mut compared = 0;
    for case in cases() {
        let Some(analysis) = analysis_of(&case) else {
            continue;
        };
        let rust = case_artifacts(&analysis);
        for (name, value) in &rust {
            let Some(python) = case.artifact(name) else {
                continue;
            };
            compared += 1;
            all.extend(differences(
                &format!("{}/{name}", case.name),
                &python,
                value,
            ));
        }
    }
    let by_name: std::collections::HashMap<String, common::Case> =
        cases().into_iter().map(|c| (c.name.clone(), c)).collect();
    for scenario in scenarios() {
        let Some(python) = &scenario.result else {
            continue;
        };
        let case = &by_name[&scenario.case];
        let analysis = analysis_of(case).expect("a scenario's case has an analysis");
        let answers = scenario.answers.as_object().cloned().unwrap_or_default();
        let defaults = scenario
            .spec
            .get("accept_defaults")
            .is_none_or(|d| d != false);
        let rust = scenario_result(&analysis, &answers, defaults);
        for key in ["questions", "open", "problems", "preview", "apply"] {
            compared += 1;
            all.extend(differences(
                &format!("scenarios/{}/{key}", scenario.name),
                &python[key],
                &rust[key],
            ));
        }
    }
    println!(
        "rules: {compared} artifact(s) compared in {:.1} s",
        started.elapsed().as_secs_f64()
    );
    // Turned defaults, datum origins, free-text notes and #20's unaccepted suggestions change
    // the Python rules. #20 changes preview and adds pending-frame problems; applied defaults
    // are intact. Keep the
    // complete before/after values, including ordering, under an exact regression guard;
    // the known-difference entries below document the verdict, not a blanket exemption.
    let departures = json!(
        all.iter()
            .map(|d| json!({
                "case": d.case, "path": d.path, "python": d.python, "rust": d.rust,
            }))
            .collect::<Vec<_>>()
    );
    let snapshot = common::fixtures().join("rule_defaults_oracle.json.gz");
    if std::env::var_os("UPDATE_RULE_DEFAULTS").is_some() {
        use std::io::Write;
        let file = std::fs::File::create(&snapshot).unwrap();
        let mut gz = flate2::GzBuilder::new()
            .mtime(0)
            .write(file, flate2::Compression::best());
        gz.write_all(&serde_json::to_vec_pretty(&departures).unwrap())
            .unwrap();
        gz.finish().unwrap();
    }
    assert_eq!(
        common::read_json_gz(&snapshot),
        departures,
        "review rule default oracle departures"
    );
    check_known("rules", &all);
}

/// The same inputs twice give the same outputs, byte for byte.
#[test]
fn rules_are_deterministic() {
    for case in cases() {
        let Some(analysis) = analysis_of(&case) else {
            continue;
        };
        let once = serde_json::to_string(&case_artifacts(&analysis)).expect("JSON");
        let twice = serde_json::to_string(&case_artifacts(&analysis)).expect("JSON");
        assert_eq!(once, twice, "{}", case.name);
    }
}
