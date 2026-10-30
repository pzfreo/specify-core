//! `specify_core._native`: the Rust engine behind the Python package `specify_core`.
//!
//! The package's `api.py` is a thin layer over these functions, with the names, signatures and
//! return shapes of specify-core's own `api.py`. Values cross as JSON text, as they do in the
//! WASM binding and `serve`: an analysis is a stored snapshot, read by the rules as JSON, and
//! serde_json's text round-trips every number through Python's `json` exactly.
//!
//! A part loaded once is a [`LoadedPart`] handle (Python's `LoadedPart`, opaque here): `analyse`
//! takes it or a path, so a caller that reuses a part, as `serve` does, reads the file once.
//!
//! Errors are raised as Python raises them: answers that leave questions open are the
//! package's `IncompleteError` (a `ValueError`, with `missing` and `part`); a file or part that
//! cannot be loaded, or an answer the rules refuse, is a `ValueError`; anything else (a part
//! that cannot be analysed honestly) is a `RuntimeError`.

use std::path::PathBuf;

use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use serde_json::{Map, Value, json};
use specify_core::api::{self, Error};
use specify_core::part;
use specify_core::rules::{self, Question};
use specify_core::writer::WriteError;

/// Python's exception for an engine error (see the module's documentation).
fn raised(error: Error) -> PyErr {
    if let Error::Rules(rules::Error::Incomplete(e)) = &error {
        return incomplete(e);
    }
    match error {
        Error::Load(_)
        | Error::Rules(rules::Error::Invalid(_))
        | Error::Write(WriteError::Invalid(_)) => PyValueError::new_err(error.to_string()),
        _ => PyRuntimeError::new_err(error.to_string()),
    }
}

/// The package's `IncompleteError(missing, part)`, as specify-core's `rules` raises it.
fn incomplete(e: &rules::IncompleteError) -> PyErr {
    Python::attach(|py| {
        let made = py
            .import("specify_core.rules")
            .and_then(|rules| rules.getattr("IncompleteError"))
            .and_then(|class| class.call1((e.missing.clone(), e.part)));
        match made {
            Ok(error) => PyErr::from_value(error),
            Err(error) => error,
        }
    })
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

/// One part of a STEP file, loaded once (Python's `LoadedPart`): the part and the file it is
/// of, so a part of an assembly is analysed with the other parts its mates need, from the same
/// bytes. Its geometry stays in the engine; Python sees what identifies it.
#[pyclass(frozen, module = "specify_core._native")]
struct LoadedPart {
    file: part::File,
    loaded: part::LoadedPart,
}

#[pymethods]
impl LoadedPart {
    /// The file it was read from.
    #[getter]
    fn path(&self) -> PathBuf {
        self.loaded.path.clone()
    }

    /// What face indices are only valid against, as JSON (`Binding.to_dict()`).
    #[getter]
    fn binding(&self) -> PyResult<String> {
        serde_json::to_string(&self.loaded.binding)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// How many parts the file holds; this is part `binding.part` of them.
    #[getter]
    fn part_count(&self) -> usize {
        self.loaded.part_count
    }

    /// The part's name in the file, if it has one.
    #[getter]
    fn name(&self) -> &str {
        &self.loaded.name
    }
}

/// The `part`-th part of `step` (any part of a file of one part; a file of several must say
/// which), loaded (`load.load`).
#[pyfunction]
#[pyo3(signature = (step, part=None))]
fn load(py: Python<'_>, step: PathBuf, part: Option<usize>) -> PyResult<LoadedPart> {
    let (file, loaded) = py
        .detach(|| {
            let file = part::File::read(&step)?;
            let loaded = file.load(part)?;
            Ok::<_, part::LoadError>((file, loaded))
        })
        .map_err(|e| raised(Error::Load(e)))?;
    Ok(LoadedPart { file, loaded })
}

/// The analysis of `step` (its `part`-th part, in an assembly), or of a part already loaded
/// (`part` must then be None), as JSON (`api.analyse`).
#[pyfunction]
#[pyo3(signature = (step, part=None))]
fn analyse(py: Python<'_>, step: &Bound<'_, PyAny>, part: Option<usize>) -> PyResult<String> {
    if let Ok(handle) = step.cast::<LoadedPart>() {
        if part.is_some() {
            return Err(PyTypeError::new_err(
                "analyse of a loaded part takes no part",
            ));
        }
        let handle = handle.get();
        let (analysis, _) = py
            .detach(|| api::analyse_reporting_in(&handle.file, &handle.loaded))
            .map_err(raised)?;
        return Ok(analysis.to_string());
    }
    let path: PathBuf = step.extract()?;
    let analysis = py
        .detach(|| api::analyse_file(&path, part))
        .map_err(raised)?;
    Ok(analysis.to_string())
}

/// Every question the part needs, given the answers so far (`api.questions`): a JSON list of
/// every field of each `Question`, for the package to build Python's `Question` from. Not
/// `to_dict`, which leaves out what Python's dataclass carries beside it (each part's faces,
/// `diameter`, `depth`, `mate`).
#[pyfunction]
fn questions(py: Python<'_>, analysis: &str, answers: &str) -> PyResult<String> {
    let (analysis, answers) = parse_both(analysis, answers)?;
    let questions = py
        .detach(|| api::questions(&analysis, &answers))
        .map_err(raised)?;
    Ok(Value::Array(questions.iter().map(record).collect()).to_string())
}

/// The intent `answers` amount to, as JSON (`Intent.to_dict()`); with `accept_defaults`,
/// unanswered defaults are taken first, to a fixed point (`api.apply`).
#[pyfunction]
#[pyo3(signature = (analysis, answers, accept_defaults=false))]
fn apply(py: Python<'_>, analysis: &str, answers: &str, accept_defaults: bool) -> PyResult<String> {
    let (analysis, answers) = parse_both(analysis, answers)?;
    let intent = py
        .detach(|| api::apply(&analysis, &answers, accept_defaults))
        .map_err(raised)?;
    Ok(intent.to_dict().to_string())
}

/// What the answers so far amount to, as JSON (`api.preview`).
#[pyfunction]
fn preview(py: Python<'_>, analysis: &str, answers: &str) -> PyResult<String> {
    let (analysis, answers) = parse_both(analysis, answers)?;
    let intent = py
        .detach(|| api::preview(&analysis, &answers))
        .map_err(raised)?;
    Ok(intent.to_dict().to_string())
}

fn parse_both(analysis: &str, answers: &str) -> PyResult<(Value, Map<String, Value>)> {
    let analysis = parse("analysis", analysis)?;
    let Value::Object(answers) = parse("answers", answers)? else {
        return Err(PyValueError::new_err("Invalid answers: not an object"));
    };
    Ok((analysis, answers))
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
    m.add_class::<LoadedPart>()?;
    m.add_function(wrap_pyfunction!(load, m)?)?;
    m.add_function(wrap_pyfunction!(parts, m)?)?;
    m.add_function(wrap_pyfunction!(analyse, m)?)?;
    m.add_function(wrap_pyfunction!(questions, m)?)?;
    m.add_function(wrap_pyfunction!(apply, m)?)?;
    m.add_function(wrap_pyfunction!(preview, m)?)?;
    m.add("ANALYSIS_SCHEMA", api::ANALYSIS_SCHEMA)?;
    m.add("LOADER_VERSION", part::LOADER_VERSION)?;
    m.add("RULES_VERSION", rules::RULES_VERSION)?;
    Ok(())
}
