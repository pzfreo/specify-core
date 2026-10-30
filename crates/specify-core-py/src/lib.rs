//! `specify_core._native`: the Rust engine behind the Python package `specify_core`.
//!
//! The package's `api.py` is a thin layer over these functions, with the names, signatures and
//! return shapes of specify-core's own `api.py`. Values cross as JSON text, as they do in the
//! WASM binding and `serve`: an analysis is a stored snapshot, read by the rules as JSON, and
//! serde_json's text round-trips every number through Python's `json` exactly.
//!
//! Errors are raised as Python raises them: a file or part that cannot be loaded, or an answer
//! the rules refuse, is a `ValueError`; anything else (a part that cannot be analysed
//! honestly) is a `RuntimeError`.

use std::path::PathBuf;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use serde_json::{Map, Value, json};
use specify_core::api::{self, Error};
use specify_core::rules::{self, Question};
use specify_core::writer::WriteError;

/// Python's exception for an engine error (see the module's documentation).
fn raised(error: Error) -> PyErr {
    match error {
        Error::Load(_)
        | Error::Rules(rules::Error::Invalid(_))
        | Error::Write(WriteError::Invalid(_)) => PyValueError::new_err(error.to_string()),
        _ => PyRuntimeError::new_err(error.to_string()),
    }
}

fn parse(what: &str, text: &str) -> PyResult<Value> {
    serde_json::from_str(text).map_err(|e| PyValueError::new_err(format!("Invalid {what}: {e}")))
}

/// The distinct parts of `step`, as a JSON list (`api.parts`).
#[pyfunction]
fn parts(py: Python<'_>, step: PathBuf) -> PyResult<String> {
    let parts = py.detach(|| api::parts(&step)).map_err(raised)?;
    Ok(Value::Array(parts).to_string())
}

/// The analysis of `step` (its `part`-th part, in an assembly), as JSON (`api.analyse`).
#[pyfunction]
#[pyo3(signature = (step, part=None))]
fn analyse(py: Python<'_>, step: PathBuf, part: Option<usize>) -> PyResult<String> {
    let analysis = py
        .detach(|| api::analyse_file(&step, part))
        .map_err(raised)?;
    Ok(analysis.to_string())
}

/// Every question the part needs, given the answers so far (`api.questions`): a JSON list of
/// every field of each `Question`, for the package to build Python's `Question` from. Not
/// `to_dict`, which leaves out what Python's dataclass carries beside it (each part's faces,
/// `diameter`, `depth`, `mate`).
#[pyfunction]
fn questions(py: Python<'_>, analysis: &str, answers: &str) -> PyResult<String> {
    let analysis = parse("analysis", analysis)?;
    let Value::Object(answers) = parse("answers", answers)? else {
        return Err(PyValueError::new_err("Invalid answers: not an object"));
    };
    let questions = py
        .detach(|| api::questions(&analysis, &answers))
        .map_err(raised)?;
    Ok(Value::Array(questions.iter().map(record).collect()).to_string())
}

/// Every field of `question`, under Python's field names.
fn record(question: &Question) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), json!(question.id));
    out.insert("kind".into(), json!(question.kind));
    out.insert("prompt".into(), json!(question.prompt));
    out.insert("faces".into(), json!(question.faces));
    out.insert(
        "default".into(),
        question.default.clone().unwrap_or(Value::Null),
    );
    out.insert("options".into(), json!(question.options));
    out.insert("why".into(), json!(question.why));
    out.insert("feature".into(), json!(question.feature));
    out.insert("frame".into(), json!(question.frame));
    out.insert("attention".into(), json!(question.attention));
    out.insert("reason".into(), json!(question.reason));
    out.insert("parts".into(), json!(question.parts));
    out.insert("suggestions".into(), json!(question.suggestions));
    out.insert("choices".into(), json!(question.choices));
    out.insert("diameter".into(), json!(question.diameter));
    out.insert("depth".into(), json!(question.depth));
    out.insert("basis".into(), json!(question.basis));
    out.insert("mate".into(), json!(question.mate));
    Value::Object(out)
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parts, m)?)?;
    m.add_function(wrap_pyfunction!(analyse, m)?)?;
    m.add_function(wrap_pyfunction!(questions, m)?)?;
    m.add("ANALYSIS_SCHEMA", api::ANALYSIS_SCHEMA)?;
    Ok(())
}
