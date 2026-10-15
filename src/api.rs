//! The steps: parts, analyse, mesh, questions, apply, preview, write (port of `api.py`).
//!
//! A file of several parts is specified a part at a time: [`parts`] lists them, [`analyse`] and
//! [`mesh`] take the one to work on, and [`write_parts`] writes any number of them into the
//! assembly together. The command line (`main.rs`) and `serve` are thin layers over these.
//!
//! Each step that reads a file has two forms, as Python's take a path or a `LoadedPart`: one on
//! a part already loaded ([`analyse`], [`mesh`], [`write`]) and one on a path
//! ([`analyse_file`], [`mesh_file`], [`write_file`]), which loads the part first.
//!
//! **In memory** (for a caller with no filesystem, the editor's browser build): a file's bytes
//! are read with [`part::File::of_bytes`] (any part, [`part::File::load`]) or
//! [`part::load_bytes`] (a file of one part); [`analyse`] and [`mesh`] take the part so loaded,
//! and [`write_bytes`] writes any of the file's parts and returns the bytes written with the
//! report. Nothing in these reads a path.
//!
//! The analysis, answers and intent are JSON values, as in Python: an analysis is a stored
//! snapshot that may come from either engine or an older version, and the rules read it as
//! such.

use std::fmt;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::anchors;
use crate::existing;
use crate::features;
use crate::mates;
use crate::mesh as meshing;
use crate::part::{self, LoadError, LoadedPart};
use crate::resume;
use crate::rules::{self, IncompleteError, Intent, Question};
use crate::writer::{self, WriteError, WriteReport};

/// The analysis format's version (Python's `ANALYSIS_SCHEMA`).
pub const ANALYSIS_SCHEMA: i64 = 1;

/// Why a step failed. The command line and `serve` map each kind to specify-core's exit code
/// and error code (see `main.rs`, `serve.rs`).
#[derive(Debug)]
pub enum Error {
    /// The file or the part could not be loaded (Python's `ValueError` from `load`).
    Load(LoadError),
    /// The part loaded but cannot be analysed honestly: a face haecceity cannot integrate, a
    /// part rebuilt without its record of unresolved geometry (upstream need U1), quiddity
    /// records this port cannot read, or PMI haecceity's reader refuses. The message names the
    /// file and why.
    Analysis(String),
    /// The rules refused: questions left open, or an answer that cannot be used.
    Rules(rules::Error),
    /// Nothing was written (see [`WriteError`] for which kinds are the caller's fault).
    Write(WriteError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Load(e) => e.fmt(f),
            Error::Analysis(why) => f.write_str(why),
            Error::Rules(e) => e.fmt(f),
            Error::Write(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Load(e) => Some(e),
            Error::Analysis(_) => None,
            Error::Rules(e) => Some(e),
            Error::Write(e) => Some(e),
        }
    }
}

impl From<LoadError> for Error {
    fn from(e: LoadError) -> Self {
        Error::Load(e)
    }
}

impl From<rules::Error> for Error {
    fn from(e: rules::Error) -> Self {
        Error::Rules(e)
    }
}

impl From<IncompleteError> for Error {
    fn from(e: IncompleteError) -> Self {
        Error::Rules(rules::Error::Incomplete(e))
    }
}

impl From<WriteError> for Error {
    fn from(e: WriteError) -> Self {
        Error::Write(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The distinct parts of `step`: `{part, name, product_id, faces, instances, placements,
/// written}` each, `written` saying whether specify-core has written the part (answers stored
/// for it). A part is identified by `part`, its index (decision D1); `name` and `product_id`
/// are reported beside it and need not be unique.
pub fn parts(step: &Path) -> Result<Vec<Value>> {
    let file = part::File::read(step)?;
    let stored = resume::stored(file.bytes());
    Ok(file
        .parts()
        .into_iter()
        .map(|p| {
            let written = stored.contains_key(&p.part);
            let mut value = serde_json::to_value(p).expect("a part summary serialises");
            value["written"] = Value::Bool(written);
            value
        })
        .collect())
}

/// The analysis of the `part`-th part of `step` (any part of a file of one part; a file of
/// several must say which). See [`analyse`]. The file is read once, for the part and, in an
/// assembly, the other parts its mates need.
pub fn analyse_file(step: &Path, part: Option<usize>) -> Result<Value> {
    let file = part::File::read(step)?;
    let loaded = file.load(part)?;
    analyse_reporting_in(&file, &loaded).map(|(analysis, _)| analysis)
}

/// Features, faces and existing PMI of `loaded`, as a JSON snapshot (Python's `analyse`):
/// `{schema, binding, faces, features}`, then for a part of an assembly its `mates` and each
/// face's `contact`, then `existing` and `part` (the file's own PMI and settings), or for a file
/// specify-core wrote whose answers fit this part `existing: []`, `part: {}` and `resume:
/// {answers}`. The snapshot is what gets stored: it is never re-derived for the same file.
///
/// Contacts that could not be measured are left out of the snapshot (its format has no place
/// for them); [`analyse_reporting`] says which.
pub fn analyse(loaded: &LoadedPart) -> Result<Value> {
    analyse_reporting(loaded).map(|(analysis, _)| analysis)
}

/// [`analyse`], and a line for each pair of facing flat faces whose contact could not be
/// measured (`mates::contacts_with_refusals`), so a caller can report what the analysis lacks.
/// For a part of an assembly the other parts are loaded from `loaded.bytes`, not from its path,
/// so a part loaded from memory is analysed as one loaded from a file; a caller that holds the
/// [`part::File`] saves that second read with [`analyse_reporting_in`].
pub fn analyse_reporting(loaded: &LoadedPart) -> Result<(Value, Vec<String>)> {
    if loaded.part_count > 1 {
        let file = part::File::of_shared(&loaded.path, std::sync::Arc::clone(&loaded.bytes))?;
        return analyse_reporting_in(&file, loaded);
    }
    analyse_with(loaded, None, None)
}

/// [`analyse_reporting`] of `loaded`, a part of `file`, the other parts of an assembly loaded
/// from `file` (not read again). Refused if `loaded` is not of `file`'s bytes.
pub fn analyse_reporting_in(
    file: &part::File,
    loaded: &LoadedPart,
) -> Result<(Value, Vec<String>)> {
    if loaded.binding.source_sha256 != file.sha256() {
        return Err(Error::Analysis(format!(
            "{}: the part is not of the file given",
            loaded.path.display()
        )));
    }
    let assembly = if loaded.part_count > 1 {
        Some(file.load_all()?)
    } else {
        None
    };
    analyse_with(loaded, assembly.as_deref(), Some(file.definitions()))
}

/// The analysis of `loaded`, with every part of its file (`assembly`) for a part of an
/// assembly and its file's part definitions (`definitions`) when they are already read. The
/// file is parsed as Part 21 once, for its stored answers and its PMI.
fn analyse_with(
    loaded: &LoadedPart,
    assembly: Option<&[LoadedPart]>,
    definitions: Option<&[haecceity::step::PartDefinition]>,
) -> Result<(Value, Vec<String>)> {
    let refused = |e: &dyn fmt::Display| Error::Analysis(format!("{}: {e}", loaded.path.display()));
    // Features first: on a part rebuilt without its record of unresolved geometry (U1),
    // recognition's refusal names the part and why, which is the failure to pass on.
    let features = features::recognise(loaded).map_err(|e| refused(&e))?;
    let faces = features::describe_faces(loaded).map_err(|e| refused(&e))?;
    let mut faces = serde_json::to_value(faces).expect("faces serialise");
    let mut analysis = Map::new();
    analysis.insert("schema".into(), json!(ANALYSIS_SCHEMA));
    analysis.insert(
        "binding".into(),
        serde_json::to_value(&loaded.binding).expect("a binding serialises"),
    );
    let mut warnings = Vec::new();
    let mut mated = None;
    if let Some(assembly) = assembly.filter(|_| loaded.part_count > 1) {
        // How the part fits the others: suggested answers, and the faces it rests on another
        // part by. Every part of the file is loaded for it, as Python's `load_all` does.
        let k = loaded.binding.part;
        mated = Some(mates::mates(assembly, k));
        let (contacts, refusals) = mates::contacts_with_refusals(assembly, k);
        for (index, contact) in contacts {
            let face = index
                .parse::<usize>()
                .ok()
                .and_then(|i| faces.get_mut(i))
                .and_then(Value::as_object_mut)
                .ok_or_else(|| refused(&format!("a contact names face {index}, not a face")))?;
            face.insert("contact".into(), contact);
        }
        warnings = refusals;
    }
    analysis.insert("faces".into(), faces);
    analysis.insert(
        "features".into(),
        serde_json::to_value(features).expect("features serialise"),
    );
    if let Some(mated) = mated {
        analysis.insert("mates".into(), Value::Object(mated));
    }
    let doc = haecceity::p21::Document::parse(loaded.bytes.as_ref().clone());
    match resume::answers_for_parsed(loaded, doc.as_ref().ok()).map_err(|e| refused(&e))? {
        Some(answers) => {
            // A file specify-core wrote, opened to be changed: its PMI is the answers', not
            // the file's to keep, and the answers come back to be edited.
            analysis.insert("existing".into(), json!([]));
            analysis.insert("part".into(), json!({}));
            analysis.insert("resume".into(), json!({ "answers": answers }));
        }
        None => {
            let unreadable =
                |e: &dyn fmt::Display| refused(&existing::ExistingError::Unreadable(e.to_string()));
            let doc = doc.map_err(|e| unreadable(&e))?;
            let read_here;
            let definitions = match definitions {
                Some(d) => d,
                None => {
                    read_here = haecceity::step::read_part_definitions(&loaded.bytes)
                        .map_err(|e| unreadable(&e))?;
                    &read_here
                }
            };
            let reading =
                existing::read_parsed(loaded, doc, definitions).map_err(|e| refused(&e))?;
            let existing = reading.existing(loaded).map_err(|e| refused(&e))?;
            let settings = reading.settings(loaded);
            analysis.insert(
                "existing".into(),
                serde_json::to_value(existing).expect("existing PMI serialises"),
            );
            analysis.insert("part".into(), Value::Object(settings));
        }
    }
    Ok((Value::Object(analysis), warnings))
}

/// Every question the part needs, given the answers so far.
pub fn questions(analysis: &Value, answers: &Map<String, Value>) -> Result<Vec<Question>> {
    Ok(rules::questions(analysis, answers)?)
}

/// Intent from `answers`; with `accept_defaults`, unanswered defaults are taken first, to a
/// fixed point (some questions only exist once an earlier default is taken).
pub fn apply(
    analysis: &Value,
    answers: &Map<String, Value>,
    accept_defaults: bool,
) -> Result<Intent> {
    Ok(rules::apply_with_defaults(
        analysis,
        answers,
        accept_defaults,
    )?)
}

/// What the answers so far amount to, defaults standing in (`rules::preview`).
pub fn preview(analysis: &Value, answers: &Map<String, Value>) -> Result<Intent> {
    Ok(rules::preview(analysis, answers)?)
}

/// Which part of its file an intent was made for. An intent made before parts were told apart
/// has none and is of a file of one part: part 0. A `part` that is not a part index is refused.
pub fn intent_part(intent: &Intent) -> Result<usize> {
    match intent.binding.get("part") {
        None => Ok(0),
        Some(v) => v.as_u64().map(|k| k as usize).ok_or_else(|| {
            Error::Write(WriteError::Invalid(format!(
                "the intent's binding names part {v}, not a part index"
            )))
        }),
    }
}

/// Write `intent` into `loaded`'s file at `output`, verified by reading back. Given the
/// `answers` it came from, they are stored in the file too, so that opening it again resumes
/// them.
pub fn write(
    loaded: &LoadedPart,
    intent: &Intent,
    output: &Path,
    answers: Option<&Map<String, Value>>,
) -> Result<WriteReport> {
    Ok(writer::write(loaded, intent, output, answers)?)
}

/// [`write`] on `step`, loading the part the intent was made for.
pub fn write_file(
    step: &Path,
    intent: &Intent,
    output: &Path,
    answers: Option<&Map<String, Value>>,
) -> Result<WriteReport> {
    let loaded = part::load(step, Some(intent_part(intent)?))?;
    write(&loaded, intent, output, answers)
}

/// Write several parts of the assembly `step` together: each intent on the part it was made
/// for, with the answers it came from (or none).
pub fn write_parts(
    step: &Path,
    intents: &[(Intent, Option<Map<String, Value>>)],
    output: &Path,
) -> Result<WriteReport> {
    let loaded = part::load_all(step)?;
    let mut jobs = Vec::with_capacity(intents.len());
    for (intent, answers) in intents {
        let k = intent_part(intent)?;
        let Some(part) = loaded.get(k) else {
            return Err(Error::Write(WriteError::Invalid(format!(
                "{} has no part {k}",
                step.display()
            ))));
        };
        jobs.push((part, intent, answers.as_ref()));
    }
    Ok(writer::write_parts(&jobs, output)?)
}

/// The name standing for the file's path in what [`write_bytes`] loads and reports (its
/// report's `output`, its errors), there being no path.
pub const IN_MEMORY: &str = "<in memory>";

/// Write parts of the file whose bytes are `bytes`, in memory (the editor's browser build): each
/// intent on the part it was made for (its binding's `part`), with the answers it came from (or
/// none), all in one edit verified by reading back, as [`write_parts`] writes a file. Returns
/// the bytes written and the report (its `output` is [`IN_MEMORY`]). The file's other parts,
/// which the writer consults (answers stored for them), are loaded from `bytes`, never from a
/// path. One part of an assembly may be written alone: the other parts, their PMI and their
/// stored answers are left as they are (decision D3).
pub fn write_bytes(
    bytes: Vec<u8>,
    jobs: &[(Intent, Option<Map<String, Value>>)],
) -> Result<(Vec<u8>, WriteReport)> {
    let file = part::File::of_bytes(Path::new(IN_MEMORY), bytes)?;
    let mut loaded = Vec::with_capacity(jobs.len());
    for (intent, _) in jobs {
        let k = intent_part(intent)?;
        if k >= file.definitions().len() {
            return Err(Error::Write(WriteError::Invalid(format!(
                "{IN_MEMORY} has no part {k}"
            ))));
        }
        loaded.push(file.load(Some(k))?);
    }
    let writes: Vec<writer::Job<'_>> = loaded
        .iter()
        .zip(jobs)
        .map(|(l, (intent, answers))| (l, intent, answers.as_ref()))
        .collect();
    Ok(writer::write_parts_bytes(&writes)?)
}

/// Triangles of `loaded` ranged by the face indices its analysis uses, in the part's own frame
/// (the `pmi-assist-mesh` format; faces that did not triangulate are in `missing`).
pub fn mesh(loaded: &LoadedPart) -> Value {
    meshing::mesh(loaded)
}

/// [`mesh`] of the `part`-th part of `step`.
pub fn mesh_file(step: &Path, part: Option<usize>) -> Result<Value> {
    Ok(mesh(&part::load(step, part)?))
}

/// Where a label for each group of faces lands, and which way its leader leaves: `{key:
/// {at, out, clear}}` (see `anchors`), the reach scaled by the part's exact diagonal.
pub fn anchors(loaded: &LoadedPart, groups: &[Vec<usize>]) -> Map<String, Value> {
    anchors::anchors(loaded, groups, anchors::diagonal(loaded))
}
