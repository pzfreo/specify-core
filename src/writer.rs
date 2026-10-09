//! Intent written into the STEP as AP242 semantic PMI through haecceity, and verified by reading
//! back (port of `writer.py`, and of what `requirements.py` and `merge.py` achieve).
//!
//! Each part's intent becomes one [`PartPmi`] (`part_pmi`): datums A–H on their datum
//! features (coplanar faces, or a hole's cylinders for an axis datum), sizes (an ISO 286 fit as
//! the fit, `±` and limits as deviations), locations (basic, or deviations) from the reference
//! faces to the feature, geometric tolerances with their zone, modifier and datum system,
//! material, the general tolerance as a class, threads (with their manufacturing data as a
//! user defined attribute set, upstream need U8) and knurls. Every part is written in one
//! `haecceity::pmi::write` edit of the file, which validates every instance it makes against
//! the AP242 schema before anything is returned.
//!
//! **Modes.** A part with no PMI, or one specify-core wrote (answers stored that match its
//! faces, [`resume`]), is written by `Mode::Replace`: afterwards its PMI is exactly the
//! intent's. A part with PMI from elsewhere is written by `Mode::Add`: every original byte is
//! kept, the new items are written beside the file's own, a datum letter the file has is the
//! file's datum, and no answers are stored (its PMI could not be told apart from what was
//! added, as in Python). An assembly follows Python's rules (each part written with its answers;
//! every part with answers stored written again), except that an assembly with PMI from
//! elsewhere, which Python refuses because of OpenCascade, is written by `Mode::Add` (question
//! Q6, a deliberate difference).
//!
//! **What is not written.** haecceity writes notes only in a standard form, and none exists
//! yet (upstream need U3, question Q3): a surface finish on faces and the part's notes
//! (surface-finish default, coating, heat treatment, edges) are listed in the report's
//! `not_written`, each saying why, never dropped silently. So is a requirement that names no
//! faces (`the size on holes:16`), having nothing in the file to be written on; the rest of the
//! intent is written (question Q17). A knurl is written as AP242's `turned_knurl`, the
//! parameters it needs beyond the intent's diameter and pitch derived from the knurling
//! standard where the intent does not state them, and the warnings say so (questions Q4, Q15;
//! `knurl`); one whose pattern or pitch the standard does not give is listed in `not_written`.
//!
//! **haecceity's gaps** (upstream need U14): a replace of a part with a thread haecceity wrote
//! is refused by its removal plan, and the thread's runout aspect reads back unconsumed; both
//! are worked round here, narrowly and checked (`clear`, `runout_gap`).
//!
//! **What is not ported**, because it only works round OpenCascade: the text fixes
//! (`_drop_runout_zones`, `_type_limits`, `_simple_tolerances`, `_part_datums`), the metre
//! scale for tolerances, fits written as bare limits, one datum label per tolerance, the text
//! route for threads, knurls, notes and the general tolerance, the leaders, and `merge.py`'s
//! transplant (haecceity's add mode keeps the original bytes itself).
//!
//! **Atomic output.** The result is written beside the destination (`.name.pid.rand.part`),
//! read back from there and verified (`verify`, the stand-in for upstream need U7), and only
//! then renamed over the destination: a symbolic link is written through to its target, an
//! existing file keeps its permissions, and any failure leaves the destination and the source
//! as they were.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use haecceity::p21::{self, Attribute, Document, RawEntity};
use haecceity::pmi::write::{Mode, PresentationPolicy, differences_as_stated};
use haecceity::pmi::{
    self, Anchor, Bounds, Compartment, Datum, DatumLabel, DatumReference, DatumSystem, Decimal,
    DimTolerance, Dimension, DimensionKind, FaceIndex, Feature, FeatureId, FundamentalDeviation,
    GeneralTolerance, GeometricTolerance, Hand, Iso286Class, Length, LengthUnit, LocationKind,
    Material, NoteOwner, PartId, PartPmi, PmiRead, Ratio, SizeKind, Thread, ThreadSide,
    ToleranceGrade, ToleranceKind, ToleranceModifier, ToleranceTarget, Value, Zone, ZoneForm,
};
use haecceity::pmi::{AttributeSet, AttributeValue};
use haecceity::step::{PartDefinition, read_part_definitions};
use serde::Serialize;
use serde_json::{Map, Value as Json, json};

use crate::part::{self, LoadedPart};
use crate::pyfmt;
use crate::pyjson;
use crate::resume;
use crate::rules::Intent;

/// What a write did (Python's `WriteReport`; `to_dict`'s keys and order).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WriteReport {
    /// The destination as given (a symbolic link stays the link's path).
    pub output: PathBuf,
    /// One line per item written, in intent order (Python's lines).
    pub written: Vec<String>,
    /// Intent that has no place in the file yet, each saying what and why; it must reach the
    /// drawing another way.
    pub not_written: Vec<String>,
    pub warnings: Vec<String>,
}

impl WriteReport {
    /// Python's `to_dict`: `{output, written, not_written, warnings}`.
    pub fn to_dict(&self) -> Json {
        json!({
            "output": self.output.to_string_lossy(),
            "written": self.written,
            "not_written": self.not_written,
            "warnings": self.warnings,
        })
    }
}

/// The file written does not read back as what was written (Python's `VerificationError`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationError(pub String);

impl fmt::Display for VerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for VerificationError {}

/// Why nothing was written. The destination and the source are as they were in every case.
#[derive(Clone, Debug, PartialEq)]
pub enum WriteError {
    /// What was asked cannot be written as asked (Python's `ValueError`, with its message): the
    /// command line's exit 2, serve's `invalid`.
    Invalid(String),
    /// haecceity would not write it: an AP214/AP203 file whose instances do not all validate
    /// against AP242 (named), or an item it refuses (named). Exit 1, serve's `failed`.
    Refused(String),
    /// The file written does not read back as written. Exit 1.
    Verification(VerificationError),
    /// The file could not be read, loaded or written. Exit 1.
    Failed(String),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Invalid(why) | WriteError::Failed(why) => f.write_str(why),
            WriteError::Refused(why) => write!(f, "haecceity will not write it: {why}"),
            WriteError::Verification(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<VerificationError> for WriteError {
    fn from(e: VerificationError) -> Self {
        WriteError::Verification(e)
    }
}

fn invalid<T>(why: impl Into<String>) -> Result<T, WriteError> {
    Err(WriteError::Invalid(why.into()))
}

/// A part to write: the part, its intent, and the answers it came from (Python's `Job`).
pub type Job<'a> = (&'a LoadedPart, &'a Intent, Option<&'a Map<String, Json>>);

/// Writes `loaded` with `intent` added as AP242 to `output`, and verifies it (Python's
/// `write`). Given `answers`, a part written from a bare file or one specify-core wrote stores
/// them, to be resumed; one with PMI from elsewhere does not.
pub fn write(
    loaded: &LoadedPart,
    intent: &Intent,
    output: &Path,
    answers: Option<&Map<String, Json>>,
) -> Result<WriteReport, WriteError> {
    write_parts(&[(loaded, intent, answers)], output)
}

/// Writes each part of one file with its intent, all in one edit (Python's `write_parts`):
/// `jobs` are (part, its intent, the answers it came from). Over references, so a caller holds
/// its parts (from `part::load_all`) and composes the jobs from them.
pub fn write_parts(jobs: &[Job<'_>], output: &Path) -> Result<WriteReport, WriteError> {
    let Some(&(first, _, _)) = jobs.first() else {
        return invalid("nothing to write");
    };
    if jobs.iter().any(|(l, _, _)| l.path != first.path) {
        return invalid("the parts written together must be of one file");
    }
    let parts: BTreeSet<usize> = jobs.iter().map(|(l, _, _)| l.binding.part).collect();
    if parts.len() != jobs.len() {
        return invalid("a part is written once");
    }
    for (loaded, intent, _) in jobs {
        check_binding(loaded, intent)?;
    }

    // Written and verified beside the destination, which is replaced only once all is well.
    let target = match std::fs::symlink_metadata(output) {
        Ok(m) if m.file_type().is_symlink() => std::fs::canonicalize(output)
            .map_err(|e| WriteError::Failed(format!("{}: {e}", output.display())))?,
        _ => output.to_path_buf(),
    };
    let Some(name) = target.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Err(WriteError::Failed(format!(
            "{}: not a file name",
            output.display()
        )));
    };
    let staged = target.with_file_name(format!(".{name}.{}.{}.part", std::process::id(), tag()));
    let result = write_verified(jobs, &staged).and_then(|report| {
        if let Ok(m) = std::fs::metadata(&target) {
            std::fs::set_permissions(&staged, m.permissions())
                .map_err(|e| WriteError::Failed(format!("{}: {e}", output.display())))?;
        }
        std::fs::rename(&staged, &target)
            .map_err(|e| WriteError::Failed(format!("{}: {e}", output.display())))?;
        Ok(report)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    let mut report = result?;
    report.output = output.to_path_buf();
    Ok(report)
}

/// Eight hex digits that differ between writes, for the staged file's name (Python's
/// `uuid4().hex[:8]`); nothing written depends on them.
fn tag() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNT: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    format!(
        "{:08x}",
        nanos ^ COUNT.fetch_add(1, Ordering::Relaxed).rotate_left(16)
    )
}

/// Python's `_check_binding`: the intent must be for these bytes, this part, its face count
/// and this loader, with Python's messages. Python compares with `==`, so `2` and `2.0` agree.
fn check_binding(loaded: &LoadedPart, intent: &Intent) -> Result<(), WriteError> {
    let binding = &intent.binding;
    let get = |key: &str| binding.get(key);
    if get("source_sha256") != Some(&json!(loaded.binding.source_sha256)) {
        return invalid("intent was made for a different file");
    }
    // An analysis made before parts were told apart is of a file of one part.
    let part = get("part").cloned().unwrap_or(json!(0));
    if !py_eq(&part, &json!(loaded.binding.part)) {
        return invalid(format!(
            "intent was made for part {}, not part {}",
            py_str(&part),
            loaded.binding.part
        ));
    }
    if !get("face_count").is_some_and(|v| py_eq(v, &json!(loaded.binding.face_count))) {
        return invalid("intent face count does not match the file");
    }
    // A loader change may number the faces differently for the same bytes.
    let version = get("loader_version").cloned().unwrap_or(Json::Null);
    if !py_eq(&version, &json!(loaded.binding.loader_version)) {
        return invalid(format!(
            "intent was made by loader version {}, not {}: analyse the file again",
            py_repr(&version),
            loaded.binding.loader_version
        ));
    }
    Ok(())
}

/// Python's `==` of two JSON values as far as the binding needs it: numbers by value (a bool is
/// 0 or 1), anything else structurally.
fn py_eq(a: &Json, b: &Json) -> bool {
    let num = |v: &Json| match v {
        Json::Number(n) => n.as_f64(),
        Json::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    };
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// Python's `repr()` of a JSON value, for messages (`None`, `True`, `1`, `'2'`).
fn py_repr(v: &Json) -> String {
    match v {
        Json::Null => "None".into(),
        Json::Bool(true) => "True".into(),
        Json::Bool(false) => "False".into(),
        Json::Number(n) if n.is_f64() => pyfmt::repr(n.as_f64().unwrap_or(f64::NAN)),
        Json::Number(n) => n.to_string(),
        Json::String(s) => pyfmt::repr_str(s),
        Json::Array(items) => {
            let items: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", items.join(", "))
        }
        Json::Object(map) => {
            let items: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", pyfmt::repr_str(k), py_repr(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}

/// Python's `str()` of a JSON value: a string as it is, anything else its `repr`.
fn py_str(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => py_repr(other),
    }
}

/// The file as read once: its document, its parts and their PMI.
struct Source {
    doc: Document,
    defs: Vec<PartDefinition>,
    read: PmiRead,
}

impl Source {
    fn of(bytes: &[u8]) -> Result<Source, WriteError> {
        let failed = |e: String| WriteError::Failed(format!("cannot read the file: {e}"));
        let defs = read_part_definitions(bytes).map_err(|e| failed(e.to_string()))?;
        let doc = Document::parse(bytes.to_vec()).map_err(|e| failed(e.to_string()))?;
        let read = pmi::read(&doc, &defs).map_err(|e| failed(e.to_string()))?;
        if read.parts.len() != defs.len() {
            return Err(failed(format!(
                "{} parts read with PMI, {} in the file",
                read.parts.len(),
                defs.len()
            )));
        }
        Ok(Source { doc, defs, read })
    }

    /// Whether the part has PMI other than stored answers.
    fn has_pmi(&self, part: usize) -> bool {
        let mut p = self.read.parts[part].clone();
        p.attributes.retain(|a| !resume::is_answers(a));
        p != PartPmi::default()
    }
}

/// One part to write, as planned.
struct Planned<'a> {
    loaded: &'a LoadedPart,
    pmi: PartPmi,
    /// The payload stored with it, if answers are stored.
    payload: Option<Json>,
    lines: Lines,
}

/// The report's lines for one part, before any part-name prefix.
#[derive(Default)]
struct Lines {
    written: Vec<String>,
    not_written: Vec<String>,
    warnings: Vec<String>,
}

fn write_verified(jobs: &[Job<'_>], staged: &Path) -> Result<WriteReport, WriteError> {
    let first = jobs[0].0;
    let bytes: &[u8] = &first.bytes;
    let source = Source::of(bytes)?;
    let bare = (0..source.defs.len()).all(|k| !source.has_pmi(k));
    let failed = |e: crate::features::FeatureError| WriteError::Failed(e.to_string());

    let resumed = if first.part_count > 1 {
        // Stored answers are how a later write knows the PMI is specify-core's.
        if jobs.iter().any(|(_, _, answers)| answers.is_none()) {
            return invalid("a part of an assembly is written with the answers it came from");
        }
        let stored = resume::stored(bytes);
        let mut stale = Vec::new();
        for &k in stored.keys() {
            let current = if k >= first.part_count {
                None
            } else if let Some((l, _, _)) = jobs.iter().find(|(l, _, _)| l.binding.part == k) {
                resume::answers_for(l).map_err(failed)?
            } else {
                let l = part::load(&first.path, Some(k))
                    .map_err(|e| WriteError::Failed(e.to_string()))?;
                resume::answers_for(&l).map_err(failed)?
            };
            if current.is_none() {
                stale.push(k.to_string());
            }
        }
        if !stale.is_empty() {
            return invalid(format!(
                "the answers stored for part(s) {} are not for this assembly's parts: it has \
                 changed since specify-core wrote it",
                stale.join(", ")
            ));
        }
        let unwritten: Vec<String> = stored
            .keys()
            .filter(|k| !jobs.iter().any(|(l, _, _)| l.binding.part == **k))
            .map(ToString::to_string)
            .collect();
        if !unwritten.is_empty() {
            return invalid(format!(
                "part(s) {} have answers stored in this file; write them again with these, or \
                 their PMI would be lost",
                unwritten.join(", ")
            ));
        }
        !stored.is_empty()
    } else {
        resume::answers_for(first).map_err(failed)?.is_some()
    };
    // Python writes a bare or resumed file from its geometry alone, and adds to any other
    // (its `merge`). Replace is the former: the part's PMI becomes exactly the intent's. A bare
    // part is Replace rather than design.md's Add so that Python-form answers that no longer
    // resume (not PMI to haecceity) go, as from Python's rewrite; question Q18.
    let mode = if bare || resumed {
        Mode::Replace
    } else {
        Mode::Add
    };

    let mut planned = Vec::new();
    for &(loaded, intent, answers) in jobs {
        let before = &source.read.parts[loaded.binding.part];
        let (mut pmi, lines) = part_pmi(intent, before, mode)?;
        let payload = match answers {
            Some(answers) if mode == Mode::Replace => {
                let payload = resume::payload(loaded, answers).map_err(failed)?;
                pmi.attributes.push(resume::attribute_set(&payload));
                Some(payload)
            }
            _ => None,
        };
        planned.push(Planned {
            loaded,
            pmi,
            payload,
            lines,
        });
    }

    let items: Vec<(PartId, PartPmi)> = planned
        .iter()
        .map(|p| (PartId(p.loaded.binding.part), p.pmi.clone()))
        .collect();
    let policy = PresentationPolicy::RemovePresentation;
    let refused = |e: pmi::write::WriteError| match e {
        pmi::write::WriteError::Edition { .. } | pmi::write::WriteError::Refused(_) => {
            WriteError::Refused(e.to_string())
        }
        pmi::write::WriteError::Invalid { .. } => WriteError::Invalid(e.to_string()),
        other => WriteError::Failed(format!("haecceity could not write the PMI: {other}")),
    };
    // The document the PMI is written into: the file's, or (upstream need U14's stand-in for
    // a replace haecceity's removal plan refuses) the file with the part's PMI removed first.
    let cleared: Option<(Document, Vec<PartDefinition>)>;
    let (doc, defs, mut edit) = match pmi::write(&source.doc, &source.defs, &items, mode, policy) {
        Ok((edit, _)) => (&source.doc, source.defs.as_slice(), edit),
        Err(pmi::write::WriteError::Removal(r))
            if mode == Mode::Replace && feature_shapes_only(&r, &source.defs) =>
        {
            let parts: Vec<usize> = planned.iter().map(|p| p.loaded.binding.part).collect();
            cleared = Some(clear(&source, &parts, &r)?);
            let (d, p) = cleared.as_ref().expect("just set");
            let (edit, _) = pmi::write(d, p, &items, mode, policy).map_err(refused)?;
            (d, p.as_slice(), edit)
        }
        Err(e) => return Err(refused(e)),
    };
    if mode == Mode::Replace {
        // Python's own form of stored answers is not PMI haecceity reads (it reports it as not
        // modelled and keeps it), so a replace takes it out here: the answers stored now are
        // the only ones.
        for p in &planned {
            for id in python_answers(doc, &defs[p.loaded.binding.part]) {
                edit.remove(id);
            }
        }
    }
    let written = doc
        .apply(&edit)
        .map_err(|e| WriteError::Failed(format!("haecceity could not apply the edit: {e}")))?;

    std::fs::write(staged, &written.bytes)
        .map_err(|e| WriteError::Failed(format!("{}: {e}", staged.display())))?;
    let back = std::fs::read(staged)
        .map_err(|e| WriteError::Failed(format!("{}: {e}", staged.display())))?;
    if back != written.bytes {
        return Err(VerificationError("the file on disk is not what was written".into()).into());
    }
    verify(&source, doc, &items, mode, &back)?;
    let stored = resume::stored(&back);
    for p in &planned {
        let k = p.loaded.binding.part;
        if let Some(payload) = &p.payload
            && stored.get(&k).map(|m| Json::Object(m.clone())).as_ref() != Some(payload)
        {
            return Err(VerificationError(format!(
                "the answers stored for part {k} do not read back"
            ))
            .into());
        }
    }

    // In an assembly written together, each line says which part it is of.
    let mut report = WriteReport {
        output: staged.to_path_buf(),
        written: Vec::new(),
        not_written: Vec::new(),
        warnings: Vec::new(),
    };
    for p in planned {
        let prefix = if jobs.len() > 1 {
            if p.loaded.name.is_empty() {
                format!("part {}: ", p.loaded.binding.part)
            } else {
                format!("{}: ", p.loaded.name)
            }
        } else {
            String::new()
        };
        let with = |lines: Vec<String>| lines.into_iter().map(|l| format!("{prefix}{l}"));
        report.written.extend(with(p.lines.written));
        report.not_written.extend(with(p.lines.not_written));
        report.warnings.extend(with(p.lines.warnings));
    }
    Ok(report)
}

/// Whether a replace was refused only because haecceity's removal plan will not remove the
/// `product_definition_shape` of a thread's feature definition, which its reader lists among
/// the part's PMI (upstream need U14): every refused seed is such a shape (none is a part's own
/// shape) and nothing else blocks.
fn feature_shapes_only(r: &haecceity::removal::RemovalRefusal, defs: &[PartDefinition]) -> bool {
    !r.infrastructure.is_empty()
        && r.infrastructure.iter().all(|(id, entity)| {
            entity == "product_definition_shape" && !defs.iter().any(|d| d.shape == *id)
        })
        && r.missing.is_empty()
        && r.blockers.is_empty()
        && r.emptied.is_empty()
        && r.anchored.is_empty()
}

/// The stand-in for upstream need U14: the file with `parts`' PMI removed, as haecceity's
/// replace would remove it, so that the replace that follows has nothing to remove. The feature
/// definitions' `product_definition_shape`s the removal plan refuses as shared infrastructure
/// (`r.infrastructure`) are first restated as the `property_definition` they are a subtype of,
/// in a scratch copy only, so the plan can take them with the PMI they belong to; they are
/// removed, so nothing of the restatement reaches the file. Then the plan runs as haecceity's
/// writer runs it (presentation the removal empties goes with it). Anything else that blocks
/// refuses the write, naming it.
fn clear(
    source: &Source,
    parts: &[usize],
    r: &haecceity::removal::RemovalRefusal,
) -> Result<(Document, Vec<PartDefinition>), WriteError> {
    use haecceity::removal;
    let failed = |e: String| WriteError::Failed(format!("cannot clear the part's PMI: {e}"));
    let mut restate = p21::Edit::new();
    for (id, _) in &r.infrastructure {
        let Some(RawEntity::Simple { attributes, .. }) = source.doc.get(*id) else {
            return Err(failed(format!("#{id} is not a simple instance")));
        };
        restate.replace(*id, p21::simple("PROPERTY_DEFINITION", attributes.clone()));
    }
    let scratch = source
        .doc
        .apply(&restate)
        .and_then(|w| Document::parse(w.bytes))
        .map_err(|e| failed(e.to_string()))?;
    let mut seeds: BTreeSet<u64> = BTreeSet::new();
    for &k in parts {
        let prov = &source.read.provenance.parts[k];
        let geometry: BTreeSet<u64> = prov.geometry.iter().flatten().copied().collect();
        seeds.extend(prov.all().into_iter().filter(|id| !geometry.contains(id)));
    }
    let plan = loop {
        match removal::plan(&scratch, &seeds, PresentationPolicy::RemovePresentation) {
            Ok(plan) => break plan,
            // As haecceity's writer: a presentation model the removal would empty goes too.
            Err(e)
                if !e.emptied.is_empty()
                    && e.blockers.is_empty()
                    && e.missing.is_empty()
                    && e.infrastructure.is_empty()
                    && e.anchored.is_empty()
                    && e.emptied.iter().all(|x| !seeds.contains(&x.id)) =>
            {
                seeds.extend(e.emptied.iter().map(|x| x.id));
            }
            // A thread's bare 'thread runout' aspect, which the reader does not consume (the
            // other half of U14), goes with its thread.
            Err(e)
                if !e.blockers.is_empty()
                    && e.blockers.iter().all(|b| is_runout_aspect(&scratch, b.id))
                    && e.missing.is_empty()
                    && e.infrastructure.is_empty()
                    && e.anchored.is_empty() =>
            {
                seeds.extend(e.blockers.iter().map(|b| b.id));
            }
            Err(e) => return Err(WriteError::Refused(e.to_string())),
        }
    };
    let mut edit = p21::Edit::new();
    plan.into_edit(&mut edit);
    let bytes = scratch
        .apply(&edit)
        .map_err(|e| failed(e.to_string()))?
        .bytes;
    let defs = read_part_definitions(&bytes).map_err(|e| failed(e.to_string()))?;
    let doc = Document::parse(bytes).map_err(|e| failed(e.to_string()))?;
    Ok((doc, defs))
}

/// The instances of Python's form of stored answers on `part` (`PROPERTY_DEFINITION('pmi-assist
/// answers', ..., #pd)`, its `PROPERTY_DEFINITION_REPRESENTATION`, the representation and its
/// item), where nothing else refers to them.
fn python_answers(doc: &Document, part: &PartDefinition) -> Vec<u64> {
    let simple = |id: u64| match doc.get(id) {
        Some(RawEntity::Simple {
            name, attributes, ..
        }) => Some((name.as_str(), attributes.as_slice())),
        _ => None,
    };
    let mut out = Vec::new();
    for &pdef in doc.referrers(part.product_definition) {
        let Some(("PROPERTY_DEFINITION", [Attribute::String(n), _, Attribute::EntityRef(pd)])) =
            simple(pdef)
        else {
            continue;
        };
        if n != resume::NAME || *pd != part.product_definition {
            continue;
        }
        let pdrs = doc.referrers(pdef);
        let [pdr] = pdrs else { continue };
        let Some(("PROPERTY_DEFINITION_REPRESENTATION", [_, Attribute::EntityRef(rep)])) =
            simple(*pdr)
        else {
            continue;
        };
        if doc.referrers(*rep) != [*pdr] {
            continue;
        }
        let Some(("REPRESENTATION", [_, Attribute::List(items), _])) = simple(*rep) else {
            continue;
        };
        let items: Vec<u64> = items
            .iter()
            .filter_map(|a| match a {
                Attribute::EntityRef(i) => Some(*i),
                _ => None,
            })
            .collect();
        if items.iter().any(|&i| doc.referrers(i) != [*rep]) {
            continue;
        }
        out.extend([pdef, *pdr, *rep]);
        out.extend(items);
    }
    out
}

/// Checks the file `back` (read from disk) against what was written (the stand-in for upstream
/// need U7, `quiddity pmi write`'s check restated): the parts and their faces are unchanged; a
/// part written by replace reads back as exactly what was written, and nothing the reader
/// consumed for it survives; a part written by add reads back as its PMI before plus exactly
/// the items written (a feature or datum equal to one it had being that one); every other part
/// reads back as before; and no finding is new. `base` is the document the edit was applied to:
/// the source's, or the source with the part's PMI cleared (U14's stand-in), whose largest id
/// is where the instances added are numbered from.
fn verify(
    source: &Source,
    base: &Document,
    written: &[(PartId, PartPmi)],
    mode: Mode,
    back: &[u8],
) -> Result<(), VerificationError> {
    let fail =
        |why: String| VerificationError(format!("the file written does not read back: {why}"));
    let after_defs = read_part_definitions(back).map_err(|e| fail(e.to_string()))?;
    let after_doc = Document::parse(back.to_vec()).map_err(|e| fail(e.to_string()))?;
    let after = pmi::read(&after_doc, &after_defs).map_err(|e| fail(e.to_string()))?;
    let (defs, before) = (&source.defs, &source.read);
    let mut problems: Vec<String> = Vec::new();
    if after_defs.len() != defs.len() || after.parts.len() != defs.len() {
        return Err(fail(format!(
            "{} parts read back, not {}",
            after_defs.len(),
            defs.len()
        )));
    }
    for (i, (d0, d1)) in defs.iter().zip(&after_defs).enumerate() {
        if d0.name != d1.name
            || d0.faces.len() != d1.faces.len()
            || d0.edges.len() != d1.edges.len()
        {
            problems.push(format!("part {i}'s name, faces or edges changed"));
        }
    }
    for (i, read) in after.parts.iter().enumerate() {
        let old = &before.parts[i];
        match written.iter().find(|(p, _)| p.0 == i) {
            None => problems.extend(
                differences_as_stated(old, read)
                    .into_iter()
                    .map(|d| format!("part {i} (not written): {d}")),
            ),
            Some((_, new)) if mode == Mode::Add => {
                let keys = |p: &PartPmi| differences_as_stated(&PartPmi::default(), p);
                let shared: BTreeSet<String> = keys(old)
                    .into_iter()
                    .filter(|k| {
                        k.starts_with("feature only in the second: ")
                            || k.starts_with("datum only in the second: ")
                    })
                    .collect();
                let mut expected: Vec<String> = keys(new)
                    .into_iter()
                    .filter(|k| !shared.contains(k))
                    .collect();
                for d in differences_as_stated(old, read) {
                    if !d.contains(" only in the second: ") {
                        problems.push(format!("part {i}: lost: {d}"));
                    } else if let Some(at) = expected.iter().position(|k| *k == d) {
                        expected.remove(at);
                    } else {
                        problems.push(format!("part {i}: not written: {d}"));
                    }
                }
                problems.extend(
                    expected
                        .into_iter()
                        .map(|k| format!("part {i}: written but not read back: {k}")),
                );
            }
            Some((_, new)) => {
                problems.extend(
                    differences_as_stated(new, read)
                        .into_iter()
                        .map(|d| format!("part {i}: {d}")),
                );
                let prov = &before.provenance.parts[i];
                let geometry: BTreeSet<u64> = prov.geometry.iter().flatten().copied().collect();
                // An instance survives when the edit's base still had it (a cleared part's
                // removed ids are not there, and what the edit added may reuse them) and the
                // file written still has it, unchanged.
                fn text(doc: &Document, id: u64) -> Option<&[u8]> {
                    doc.span(id).map(|s| &doc.bytes()[s])
                }
                for id in prov.all() {
                    let original = text(&source.doc, id);
                    if !geometry.contains(&id)
                        && original.is_some()
                        && text(base, id) == original
                        && text(&after_doc, id) == original
                    {
                        problems.push(format!(
                            "part {i}: #{id}, which the reader consumed, survives"
                        ));
                    }
                }
            }
        }
    }
    for f in &after.findings {
        if !before.findings.contains(f) && !runout_gap(f, &after_doc, base.max_id()) {
            problems.push(format!(
                "a finding the input did not have: {} {:?} {}: {}",
                f.kind.as_str(),
                f.ids,
                f.entity,
                f.detail
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(fail(problems.join("; ")))
    }
}

/// Whether `finding` is haecceity's known reader gap on a thread just written (upstream need
/// U14): `thread` WR16 requires a 'thread runout' shape aspect even when no runout is stated,
/// the writer always writes it, and the reader consumes it only with a usage, so it reports
/// the bare aspect as unconsumed. Only an added instance (above the base's largest id) that
/// is exactly that aspect qualifies; haecceity's own tests make the same exception.
fn runout_gap(finding: &pmi::Finding, doc: &Document, base_max: u64) -> bool {
    let [id] = finding.ids.as_slice() else {
        return false;
    };
    finding.kind == pmi::FindingKind::Unconsumed
        && finding.entity == "shape_aspect"
        && *id > base_max
        && is_runout_aspect(doc, *id)
}

/// Whether `id` is a `SHAPE_ASPECT` described 'thread runout' (thread WR16's aspect).
fn is_runout_aspect(doc: &Document, id: u64) -> bool {
    matches!(
        doc.get(id),
        Some(RawEntity::Simple { name, attributes, .. })
            if name == "SHAPE_ASPECT"
                && matches!(attributes.get(1), Some(Attribute::String(d)) if d == "thread runout")
    )
}

// ---------------------------------------------------------------------------------------------
// Intent → PartPmi
// ---------------------------------------------------------------------------------------------

/// A number of the intent as a decimal in millimetres, written as Part 21 writes a REAL (the
/// shortest text that reads back as the number).
fn mm(x: f64) -> Result<Value, WriteError> {
    Ok(Value::length(decimal(x)?, LengthUnit::Millimetre))
}

fn length(x: f64) -> Result<Length, WriteError> {
    Ok(Length {
        value: decimal(x)?,
        unit: LengthUnit::Millimetre,
    })
}

fn decimal(x: f64) -> Result<Decimal, WriteError> {
    let text = p21::encode_real(x).map_err(|e| WriteError::Invalid(e.to_string()))?;
    Decimal::parse(&text).map_err(|e| WriteError::Invalid(e.0))
}

/// A requirement's number under `key`, refused when it is missing or not a number.
fn number(req: &Map<String, Json>, key: &str) -> Result<f64, WriteError> {
    match req.get(key) {
        Some(Json::Number(n)) => n
            .as_f64()
            .ok_or_else(|| WriteError::Invalid(format!("{key} is not a number"))),
        _ => invalid(format!(
            "a {} requirement has no number '{key}'",
            py_str(req.get("kind").unwrap_or(&Json::Null))
        )),
    }
}

fn text<'a>(req: &'a Map<String, Json>, key: &str) -> Result<&'a str, WriteError> {
    req.get(key).and_then(Json::as_str).ok_or_else(|| {
        WriteError::Invalid(format!(
            "a {} requirement has no '{key}'",
            py_str(req.get("kind").unwrap_or(&Json::Null))
        ))
    })
}

/// Python's truth of an optional value.
fn truthy(v: Option<&Json>) -> bool {
    match v {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(Json::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Json::String(s)) => !s.is_empty(),
        Some(Json::Array(a)) => !a.is_empty(),
        Some(Json::Object(o)) => !o.is_empty(),
    }
}

/// A list of face ids as Python prints a list (`[2, 5]`).
fn face_list(v: Option<&Json>) -> String {
    v.map_or_else(|| "[]".into(), pyjson::dumps)
}

/// The features of the part being built: one per distinct face set (haecceity writes features
/// of equal items as one instance anyway).
struct Features<'p>(&'p mut Vec<Feature>);

/// The face ids a face list names, `None` when it names none (missing, empty or not a list of
/// face ids).
fn face_ids(faces: Option<&Json>) -> Option<Vec<usize>> {
    faces
        .and_then(Json::as_array)
        .and_then(|a| {
            a.iter()
                .map(|x| x.as_u64().map(|i| i as usize))
                .collect::<Option<Vec<_>>>()
        })
        .filter(|ids| !ids.is_empty())
}

/// What of a requirement names no faces (`the size on holes:16`), if anything does: such a
/// requirement has nothing in the file to be written on (question Q17).
fn faceless(kind: &str, req: &Map<String, Json>) -> Option<String> {
    let name = py_str(req.get("feature").unwrap_or(&Json::Null));
    let none = |key: &str| face_ids(req.get(key)).is_none();
    match kind {
        "size" if none("faces") => Some(format!("the size on {name}")),
        "location" if none("faces") => Some(format!("the location of {name}")),
        "location" if none("reference") => Some(format!("the reference of the location of {name}")),
        "thread" if none("faces") => Some(format!("the thread on {name}")),
        "knurl" if none("faces") => Some(format!("the knurl on {name}")),
        other if tolerance_kind(other).is_some() && none("faces") => {
            Some(format!("the {other} tolerance on {name}"))
        }
        _ => None,
    }
}

impl Features<'_> {
    fn of(&mut self, faces: Option<&Json>, what: &str) -> Result<FeatureId, WriteError> {
        let Some(ids) = face_ids(faces) else {
            return invalid(format!("{what} names no faces"));
        };
        let f = Feature::Items(
            ids.into_iter()
                .map(|i| Anchor::Face(FaceIndex(i)))
                .collect(),
        );
        if let Some(i) = self.0.iter().position(|x| *x == f) {
            return Ok(FeatureId(i));
        }
        self.0.push(f);
        Ok(FeatureId(self.0.len() - 1))
    }
}

/// The geometric tolerance kinds of the intent (Python's `_GEOM_TYPES`).
fn tolerance_kind(kind: &str) -> Option<ToleranceKind> {
    Some(match kind {
        "position" => ToleranceKind::Position,
        "flatness" => ToleranceKind::Flatness,
        "perpendicularity" => ToleranceKind::Perpendicularity,
        "parallelism" => ToleranceKind::Parallelism,
        "profile" => ToleranceKind::SurfaceProfile,
        "runout" => ToleranceKind::CircularRunout,
        "total_runout" => ToleranceKind::TotalRunout,
        _ => return None,
    })
}

/// An ISO 286 class from its name (`H7`, `g6`, `js11`), refused when it is not one.
fn fit_class(name: &str) -> Result<Iso286Class, WriteError> {
    let refused = || WriteError::Invalid(format!("{name:?} is not an ISO 286 tolerance class"));
    let split = name
        .find(|c: char| c.is_ascii_digit())
        .ok_or_else(refused)?;
    Ok(Iso286Class {
        deviation: FundamentalDeviation::parse(&name[..split]).map_err(|_| refused())?,
        grade: ToleranceGrade::parse(&name[split..]).map_err(|_| refused())?,
    })
}

/// Signed deviations `upper`/`lower` from nominal, refused unless `upper > lower`.
fn deviations(upper: f64, lower: f64, what: &str) -> Result<DimTolerance, WriteError> {
    let bounds = Bounds::new(mm(upper)?, mm(lower)?)
        .map_err(|e| WriteError::Invalid(format!("{what}: {}", e.0)))?;
    Ok(DimTolerance::Deviations(bounds))
}

/// The intent of one part as haecceity's model, and the report's lines for it. `before` is the
/// part's PMI as read: in add mode a datum it already has is its own (no "datum" line).
fn part_pmi(intent: &Intent, before: &PartPmi, mode: Mode) -> Result<(PartPmi, Lines), WriteError> {
    let mut p = PartPmi::default();
    let mut lines = Lines::default();
    let mut features = Vec::new();
    let mut f = Features(&mut features);

    // Datums, each established by its feature; a tolerance names them by letter.
    let had: BTreeSet<&str> = if mode == Mode::Add {
        before.datums.iter().map(|d| d.label().as_str()).collect()
    } else {
        BTreeSet::new()
    };
    let mut letters: Vec<String> = Vec::new();
    for d in &intent.datums {
        let letter = d
            .get("letter")
            .and_then(Json::as_str)
            .ok_or_else(|| WriteError::Invalid("a datum of the intent has no letter".into()))?;
        let feature = f.of(d.get("faces"), &format!("datum {letter}"))?;
        let label = DatumLabel::new(letter)
            .map_err(|e| WriteError::Invalid(format!("datum {letter}: {}", e.0)))?;
        let datum =
            Datum::new(label, Some(feature), Vec::new()).map_err(|e| WriteError::Invalid(e.0))?;
        p.datums.push(datum);
        letters.push(letter.to_string());
        if !had.contains(letter) {
            lines.written.push(format!("datum {letter}"));
        }
    }
    // The datums a tolerance written cites (one on no faces is not written).
    let referenced: BTreeSet<String> = intent
        .requirements
        .iter()
        .filter(|r| faceless(r.get("kind").and_then(Json::as_str).unwrap_or(""), r).is_none())
        .filter_map(|r| r.get("datums").and_then(Json::as_array))
        .flatten()
        .filter_map(|l| l.as_str().map(str::to_string))
        .collect();
    let system = |req: &Map<String, Json>| -> Result<Option<DatumSystem>, WriteError> {
        let Some(list) = req.get("datums").and_then(Json::as_array) else {
            return Ok(None);
        };
        if list.is_empty() {
            return Ok(None);
        }
        let mut compartments = Vec::new();
        for l in list {
            let at = l
                .as_str()
                .and_then(|l| letters.iter().position(|x| x == l))
                .ok_or_else(|| {
                    WriteError::Invalid(format!(
                        "a tolerance cites datum {}, which the intent does not define",
                        py_str(l)
                    ))
                })?;
            compartments.push(Compartment {
                references: vec![DatumReference {
                    datum: pmi::DatumId(at),
                    modifiers: Vec::new(),
                }],
                modifiers: Vec::new(),
            });
        }
        DatumSystem::new(compartments)
            .map(Some)
            .map_err(|e| WriteError::Invalid(e.0))
    };

    for req in &intent.requirements {
        let kind = req.get("kind").and_then(Json::as_str).unwrap_or("");
        let feature_name = || py_str(req.get("feature").unwrap_or(&Json::Null));
        // A requirement on no faces (a hole whose bores the rules keep no cylinder for) is
        // listed and the rest written (question Q17).
        if let Some(what) = faceless(kind, req) {
            lines.not_written.push(format!(
                "{what} names no faces, so nothing in the file can carry it (question Q17)"
            ));
            continue;
        }
        match kind {
            "size" => {
                let feature = f.of(req.get("faces"), &format!("the size on {}", feature_name()))?;
                let nominal = number(req, "nominal")?;
                let fit = req.get("fit");
                let tolerance = match fit {
                    Some(Json::String(name)) if !name.is_empty() => {
                        // The class alone is written, so it must mean what the intent's
                        // deviations say: an intent whose limits are not its class's is refused
                        // rather than silently changed.
                        let what = format!("the {name} size on {}", feature_name());
                        let (upper, lower) = (number(req, "upper")?, number(req, "lower")?);
                        let (u, l) = crate::standards::fit_limits(name, nominal)
                            .map_err(|e| WriteError::Invalid(format!("{what}: {e}")))?;
                        if (u - upper).abs() > 1e-9 || (l - lower).abs() > 1e-9 {
                            return Err(WriteError::Invalid(format!(
                                "{what}: its limits {} / {} are not the class's {} / {}",
                                pyfmt::repr(upper),
                                pyfmt::repr(lower),
                                pyfmt::repr(u),
                                pyfmt::repr(l)
                            )));
                        }
                        DimTolerance::Fit {
                            class: fit_class(name)?,
                            limits: None,
                        }
                    }
                    _ => deviations(
                        number(req, "upper")?,
                        number(req, "lower")?,
                        &format!("size on {}", feature_name()),
                    )?,
                };
                p.dimensions.push(Dimension {
                    kind: DimensionKind::Size {
                        feature,
                        kind: SizeKind::Diameter,
                        path: None,
                        angle: None,
                    },
                    nominal: Some(mm(nominal)?),
                    tolerance,
                    qualifier: None,
                    modifiers: Vec::new(),
                    principle: None,
                });
                lines.written.push(format!(
                    "Ø{} {} on {}",
                    pyfmt::g(nominal),
                    fit.map_or(String::new(), py_str),
                    feature_name()
                ));
            }
            "location" => {
                let to = f.of(
                    req.get("faces"),
                    &format!("the location of {}", feature_name()),
                )?;
                let from = f.of(
                    req.get("reference"),
                    &format!("the reference of the location of {}", feature_name()),
                )?;
                let distance = number(req, "distance")?;
                let basic = truthy(req.get("basic"));
                let (tolerance, how) = if basic {
                    (DimTolerance::Basic, "basic".to_string())
                } else {
                    let (upper, lower) = (number(req, "upper")?, number(req, "lower")?);
                    let how = if upper == -lower {
                        format!("±{}", pyfmt::g(upper))
                    } else {
                        format!("+{}/{}", pyfmt::g(upper), pyfmt::signed_g(lower))
                    };
                    let what = format!("location to {}", feature_name());
                    (deviations(upper, lower, &what)?, how)
                };
                p.dimensions.push(Dimension {
                    kind: DimensionKind::Location {
                        from,
                        to,
                        kind: LocationKind::LinearDistance,
                        path: None,
                        directed: false,
                        angle: None,
                    },
                    nominal: Some(mm(distance)?),
                    tolerance,
                    qualifier: None,
                    modifiers: Vec::new(),
                    principle: None,
                });
                lines.written.push(format!(
                    "location {} {how} to {} from faces {}",
                    pyfmt::g(distance),
                    feature_name(),
                    face_list(req.get("reference"))
                ));
            }
            "thread" => {
                let feature = f.of(
                    req.get("faces"),
                    &format!("the thread on {}", feature_name()),
                )?;
                let (pmi_thread, attributes) = thread(req, feature)?;
                p.threads.push(pmi_thread);
                p.attributes.push(attributes);
                lines.written.push(format!(
                    "{} thread {}-{} on {}",
                    text(req, "side")?,
                    text(req, "spec")?,
                    text(req, "class")?,
                    feature_name()
                ));
            }
            "knurl" => {
                let pattern = text(req, "pattern")?;
                match knurl(req, pattern, &feature_name())? {
                    Err(why) => lines.not_written.push(format!(
                        "{pattern} knurl on {}: {why}, so AP242's turned_knurl cannot be \
                         completed (it needs at least three of the number of teeth, tooth \
                         depth, root fillet, helix angle and hand, WR2); state them in the \
                         intent (questions Q4, Q15)",
                        feature_name()
                    )),
                    Ok((mut k, filled)) => {
                        k.feature = f.of(
                            req.get("faces"),
                            &format!("the knurl on {}", feature_name()),
                        )?;
                        p.knurls.push(k);
                        lines.warnings.extend(filled);
                        lines
                            .written
                            .push(format!("{pattern} knurl on {}", feature_name()));
                    }
                }
            }
            "finish" => {
                lines.not_written.push(format!(
                    "surface finish {} on faces {}: haecceity writes no surface texture on faces \
                     yet, having no standard form for it (upstream need U3, question Q3)",
                    py_str(req.get("value").unwrap_or(&Json::Null)),
                    face_list(req.get("faces"))
                ));
            }
            other => {
                let Some(tolerance_kind) = tolerance_kind(other) else {
                    return invalid(format!(
                        "unknown requirement kind {}",
                        pyfmt::repr_str(other)
                    ));
                };
                let target = f.of(
                    req.get("faces"),
                    &format!("the {other} tolerance on {}", feature_name()),
                )?;
                let value = number(req, "tolerance")?;
                let mmc = truthy(req.get("mmc"));
                p.tolerances.push(GeometricTolerance {
                    kind: tolerance_kind,
                    target: ToleranceTarget::Feature(target),
                    magnitude: Some(mm(value)?),
                    zone: truthy(req.get("diametral")).then_some(Zone {
                        form: ZoneForm::CylindricalOrCircular,
                        projected: None,
                        non_uniform: None,
                        runout_angle: None,
                        affected_plane: None,
                    }),
                    modifiers: if mmc {
                        vec![ToleranceModifier::MaximumMaterialRequirement]
                    } else {
                        Vec::new()
                    },
                    unit_basis: None,
                    maximum: None,
                    unequal: None,
                    datums: system(req)?,
                    auxiliary: Vec::new(),
                    description: None,
                });
                let on = if truthy(req.get("feature")) {
                    feature_name()
                } else {
                    face_list(req.get("faces"))
                };
                lines.written.push(format!(
                    "{other} {}{} on {on}",
                    pyfmt::g(value),
                    if mmc { " MMC" } else { "" }
                ));
            }
        }
    }
    for d in &intent.datums {
        let letter = d.get("letter").and_then(Json::as_str).unwrap_or("");
        if !had.contains(letter) && !referenced.contains(letter) {
            lines.warnings.push(format!(
                "datum {letter} is in the file but no tolerance references it; OCCT-based \
                 readers (including Draftwright) will not see it"
            ));
        }
    }
    // The material and the general tolerance are one each per part. In add mode a part that
    // already states one keeps it: the same again is not written twice (a warning says so),
    // another is refused, as add cannot replace it (Python adds a second; question Q18). (The
    // rules do not ask for what a part states, but a part of an assembly reports no settings,
    // as in Python.)
    let stated = |what: &str, old: Option<&str>, new: &str, lines: &mut Lines| match old {
        Some(old) if old == new => {
            lines.warnings.push(format!(
                "{what} {new} is already stated in the file; not written again"
            ));
            Ok(false)
        }
        Some(old) => invalid(format!(
            "the part already states {what} {old}; adding {new} would give it two (it was \
             not written by specify-core, so it is added to, not replaced)"
        )),
        None => Ok(true),
    };
    let add = mode == Mode::Add;
    if let Some(material) = intent.part.get("material").filter(|v| truthy(Some(v))) {
        let name = py_str(material);
        let old = before
            .material
            .as_ref()
            .map(|m| m.id.as_str())
            .filter(|_| add);
        if stated("material", old, &name, &mut lines)? {
            p.material = Some(Material {
                id: name.clone(),
                name: None,
                density: None,
            });
            lines.written.push(format!("material {name}"));
        }
    }
    if let Some(general) = intent
        .part
        .get("general_tolerance")
        .filter(|v| truthy(Some(v)))
    {
        let text = py_str(general);
        let old = before
            .general
            .iter()
            .find_map(|g| match g {
                GeneralTolerance::Class { text, .. } => Some(text.as_str()),
                GeneralTolerance::Table { .. } => None,
            })
            .filter(|_| add);
        if stated("general tolerance", old, &text, &mut lines)? {
            p.general.push(GeneralTolerance::Class {
                standard: pmi::standards::Iso2768::recognise(&text),
                text: text.clone(),
            });
            lines.written.push(format!("general tolerance {text}"));
        }
    }
    for key in crate::rules::NOTES {
        if let Some(value) = intent.part.get(key).filter(|v| truthy(Some(v))) {
            lines.not_written.push(format!(
                "{} {}: haecceity writes no notes yet, having no standard form for them \
                 (upstream need U3, question Q3)",
                key.replace('_', " "),
                py_str(value)
            ));
        }
    }
    p.features = features;
    Ok((p, lines))
}

/// A thread of the intent as haecceity's `Thread`, with what is not thread semantics (pitch,
/// tapping drill, depths, through) as a user defined attribute set on its feature. haecceity's
/// own mapping of specify-core's threads (its `pmi_write.rs` test): one thread ('number of
/// threads' 1), form "M", the designation as the qualifier, the major diameter the nominal
/// (upstream need U8: what 'number of threads' counts and where the pitch goes are open). The
/// attribute names are Python's UDA names; 'through' is the string "true" as Python writes it
/// (haecceity refuses booleans in attribute sets: its reader cannot read them back).
fn thread(
    req: &Map<String, Json>,
    feature: FeatureId,
) -> Result<(Thread, AttributeSet), WriteError> {
    let side = text(req, "side")?;
    let internal = match side {
        "internal" => true,
        "external" => false,
        other => {
            return invalid(format!(
                "thread side {} is not internal or external",
                pyfmt::repr_str(other)
            ));
        }
    };
    let major = match req.get("nominal") {
        Some(Json::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        _ => {
            let designation = text(req, "designation")?;
            designation
                .strip_prefix('M')
                .and_then(pyfmt::parse_float)
                .ok_or_else(|| {
                    WriteError::Invalid(format!(
                        "thread designation {} is not a metric size",
                        pyfmt::repr_str(designation)
                    ))
                })?
        }
    };
    let major = length(major)?;
    let thread = Thread {
        feature,
        partial_area: Some(feature),
        side: if internal {
            ThreadSide::Internal
        } else {
            ThreadSide::External
        },
        major_diameter: major.clone(),
        minor_diameter: None,
        pitch_diameter: None,
        number_of_threads: Ratio(Decimal::parse("1.").expect("a decimal")),
        form: "M".into(),
        fit_class: text(req, "class")?.into(),
        fit_class_2: None,
        hand: if req.get("hand").and_then(Json::as_str) == Some("LH") {
            Hand::Left
        } else {
            Hand::Right
        },
        crest: None,
        qualifier: Some(text(req, "spec")?.into()),
        nominal_size: Some(major),
        runout: None,
    };
    let mut items = vec![(
        "pitch".to_string(),
        AttributeValue::Measure(mm(number(req, "pitch")?)?),
    )];
    let mut measure = |key: &str, name: &str| -> Result<(), WriteError> {
        items.push((name.into(), AttributeValue::Measure(mm(number(req, key)?)?)));
        Ok(())
    };
    if internal {
        measure("drill_diameter", "tapping drill diameter")?;
        if truthy(req.get("through")) {
            items.push(("through".into(), AttributeValue::Text("true".into())));
        } else {
            measure("drill_depth", "tapping drill depth")?;
            measure("full_thread", "minimum full thread")?;
        }
    } else if req.get("length").is_some_and(|v| !v.is_null()) {
        measure("length", "thread length")?;
    }
    Ok((
        thread,
        AttributeSet {
            name: "thread manufacturing".into(),
            on: Some(NoteOwner::Feature(feature)),
            items,
        },
    ))
}

/// A knurl of the intent as haecceity's `Knurl` (its feature left for the caller to set), with a
/// warning for each value filled in, or why it cannot be written. AP242's `turned_knurl`
/// states six to nine parameters (WR2): the major and nominal diameters, the diametral pitch,
/// and at least three of the number of teeth, tooth depth, root fillet, helix angle and hand;
/// a diamond knurl needs the helix angle (WR9), a diagonal one the hand too (WR10). The
/// intent's diameter is the finished knurl's (DIN 82's nominal diameter d1), so it is both the
/// major and the nominal diameter; its pitch is the diametral pitch. The rest is the intent's
/// where it states them (editable), else derived from the knurling standard as the rules derive
/// them (`standards::knurl_parameters`), for an intent made without them (Python's, or one
/// stored before; questions Q4, Q15), and the report's warnings say so. A diamond knurl states
/// no hand: it is two crossed helices.
fn knurl(
    req: &Map<String, Json>,
    pattern: &str,
    name: &str,
) -> Result<Result<(pmi::Knurl, Vec<String>), String>, WriteError> {
    let diamond = match pattern {
        "straight" => false,
        "diamond" => true,
        other => {
            return Ok(Err(format!(
                "the pattern {} is not straight or diamond",
                pyfmt::repr_str(other)
            )));
        }
    };
    let (pitch, diameter) = (number(req, "pitch")?, number(req, "diameter")?);
    let stated = |key: &str| req.get(key).filter(|v| !v.is_null());
    let mut teeth = match stated("teeth") {
        None => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|t| u32::try_from(t).ok())
                .filter(|t| *t > 0)
                .ok_or_else(|| {
                    WriteError::Invalid(format!(
                        "the knurl on {name}: teeth {} is not a whole number of teeth",
                        py_str(v)
                    ))
                })?,
        ),
    };
    let optional = |key: &str| match stated(key) {
        None => Ok(None),
        Some(_) => number(req, key).map(Some),
    };
    let mut tooth_depth = optional("tooth_depth")?;
    let mut root_fillet = optional("root_fillet")?;
    let mut helix_angle = optional("helix_angle")?;
    let mut missing = Vec::new();
    for (absent, what) in [
        (teeth.is_none(), "number of teeth"),
        (tooth_depth.is_none(), "tooth depth"),
        (root_fillet.is_none(), "root fillet"),
        (diamond && helix_angle.is_none(), "helix angle"),
    ] {
        if absent {
            missing.push(what);
        }
    }
    let mut warnings = Vec::new();
    if !missing.is_empty() {
        let k = match crate::standards::knurl_parameters(pattern, pitch, diameter) {
            Ok(k) => k,
            Err(why) => return Ok(Err(why)),
        };
        let mut filled = Vec::new();
        if teeth.is_none() {
            teeth = Some(k.teeth);
            filled.push(format!("{} teeth", k.teeth));
        }
        if tooth_depth.is_none() {
            tooth_depth = Some(k.tooth_depth);
            filled.push(format!("tooth depth {}", pyfmt::g(k.tooth_depth)));
        }
        if root_fillet.is_none() {
            root_fillet = Some(k.root_fillet);
            filled.push(format!("root fillet {}", pyfmt::g(k.root_fillet)));
        }
        if diamond && helix_angle.is_none() {
            helix_angle = k.helix_angle;
            filled.push(format!(
                "helix angle {}° and no hand",
                pyfmt::g(crate::standards::KNURL_HELIX_ANGLE)
            ));
        }
        warnings.push(format!(
            "{pattern} knurl on {name}: the intent states no {}; written with {} from {} \
             (questions Q4, Q15)",
            missing.join(", "),
            filled.join(", "),
            crate::standards::KNURL_STANDARD
        ));
    }
    let optional_length = |x: Option<f64>| x.map(length).transpose();
    let k = pmi::Knurl {
        feature: FeatureId(0),
        pattern: if diamond {
            pmi::KnurlPattern::Diamond
        } else {
            pmi::KnurlPattern::Straight
        },
        major_diameter: length(diameter)?,
        nominal_diameter: length(diameter)?,
        diametral_pitch: length(pitch)?,
        number_of_teeth: teeth.map(pmi::Count),
        tooth_depth: optional_length(tooth_depth)?,
        root_fillet: optional_length(root_fillet)?,
        helix_angle: helix_angle
            .map(|a| {
                Ok::<_, WriteError>(pmi::Angle {
                    value: decimal(a)?,
                    unit: pmi::AngleUnit::Degree,
                })
            })
            .transpose()?,
        helix_hand: None,
    };
    Ok(Ok((k, warnings)))
}
