//! `carry OLD NEW -o OUT` (Specify 0.3.0, piece 4; ADR P11 "Carry forward"): an older
//! revision's requirements copied onto a new revision of the geometry, file to file.
//!
//! OLD's features are matched to NEW's with the revision matcher `diff` uses (its face
//! correspondence and feature changes). Then, for each feature-and-kind OLD's feature table
//! states:
//!
//! - NEW has a gap: **carried**, as a fill applied by the in-place writer
//!   ([`crate::writer::apply`]), so every other entity of NEW keeps its bytes and the whole
//!   carry is applied or nothing is;
//! - NEW states it (or decided it none): NEW stands, and where the two differ both are reported
//!   (**kept**);
//! - the feature moved or changed size: carried, and listed **to check**;
//! - the feature is not found, or its match is ambiguous: **not carried**, and reported.
//!
//! Decided-none carries the same way. A fill is an answer, so a stated feature-and-kind carries
//! only when an answer restates it exactly on OLD (its requirements, values included, are the
//! ones the answer plans): the kinds of requirement Specify understands. Anything else in OLD
//! (PMI on no recognised feature-and-kind, or a requirement no answer restates) is reported
//! **not carried**, with its kind and faces. Nothing is dropped silently.
//!
//! **Numbers carry**: a requirement matched to OLD keeps OLD's characteristic number, and OLD's
//! number history (its retired numbers) carries too; in an assembly, a part matched to OLD keeps
//! its BOM item number and BOM answers. New features with nothing stated are listed as
//! information (diff's `new_unstated`). There is no three-way `--base` in this release.

use std::collections::{BTreeMap, BTreeSet};

use quiddity::correspondence::Class;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::edits::{EditList, Failure};
use crate::feature_table::{FeatureTable, Row, State};
use crate::part::{File, LoadedPart};
use crate::revision::{self, Revised, Unstated};

/// A feature-and-kind carried (or decided none carried) onto NEW.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Carried {
    /// Its id in OLD.
    pub old: String,
    /// Its id in NEW.
    pub new: String,
    /// The value written (a fill's answer, or the none recorded).
    pub value: Value,
    /// What OLD's file states for it, as its table shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stated: Option<Value>,
    /// Why it is to be checked: the feature moved or changed size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
    /// Carried with another feature-and-kind, by the requirement that states both (a hole's
    /// thread callout states its thread depth too): nothing is filled for it of its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with: Option<String>,
}

/// A feature-and-kind NEW states (or decided none) differently from OLD: NEW's stands.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Kept {
    pub old: String,
    pub new: String,
    /// OLD's value, as its table shows it (a decided none shows the none).
    pub old_value: Value,
    /// NEW's value, which stands.
    pub new_value: Value,
}

/// Something OLD states that was not carried, with its kind and faces and why.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NotCarried {
    /// The feature-and-kind in OLD, when it is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Its characteristic number in OLD, when it is one requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    /// `dimension`, `geometric_tolerance`, `note`, `setting` ... or a feature-and-kind's kind.
    pub kind: String,
    /// The requirement's type, when it is one requirement.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub r#type: String,
    /// OLD's faces.
    pub faces: Vec<usize>,
    /// What OLD states, as its table shows it.
    pub text: String,
    /// The part of an assembly it is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    pub reason: String,
}

/// A carried requirement that has a new number: OLD's is used or retired in NEW.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Renumbered {
    /// Its number in OLD.
    pub old: u64,
    /// Its number in OUT, above both files' high-water marks.
    pub new: u64,
    pub kind: String,
    pub r#type: String,
    /// Its faces in OUT.
    pub faces: Vec<usize>,
}

/// What carried, from what, and whether it was written.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    /// OUT was written. False when the carry was refused (see `refused`): nothing is written.
    pub written: bool,
    /// Carried onto a gap of NEW.
    pub carried: Vec<Carried>,
    /// Carried, on a feature that moved or changed size.
    pub to_check: Vec<Carried>,
    /// NEW states it differently: NEW's stands.
    pub kept: Vec<Kept>,
    pub not_carried: Vec<NotCarried>,
    /// OLD's decided-none carried onto a gap of NEW.
    pub decided_none: Vec<Carried>,
    /// Information only: features new in NEW that NEW states nothing on.
    pub new_unstated: Vec<Unstated>,
    /// At least one carried requirement kept OLD's characteristic number (OLD's number history
    /// went with them).
    pub numbers_carried: bool,
    /// Carried requirements that could not keep OLD's number (the new file uses it or retired
    /// it), each with the number it has now.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub renumbered: Vec<Renumbered>,
    /// OLD's BOM history went with the assembly's parts.
    pub bom_carried: bool,
    /// The characteristic numbers the carry retired (none, ordinarily).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub retired: Vec<u64>,
    /// Why nothing was written: the carry's edit list could not be applied (all or nothing).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refused: Vec<Failure>,
    /// The writer's lines (what was written, not written again, warnings).
    #[serde(skip_serializing_if = "Value::is_null")]
    pub write: Value,
}

/// A carry's result: the file to write (when it could be applied) and its report.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub bytes: Option<Vec<u8>>,
    pub report: Report,
}

type Error = String;

/// The part of `file` a single-part carry compares: its only part.
fn single(file: &File) -> Result<LoadedPart, Error> {
    file.load(None).map_err(|e| e.to_string())
}

/// `carry OLD NEW`: OLD's requirements carried onto NEW (see the module). Nothing is written
/// here; [`Outcome::bytes`] is NEW with the carry applied, or `None` when it was refused.
pub fn carry(old: &File, new: &File) -> Result<Outcome, Error> {
    if revision::assembly::is_assembly(old) || revision::assembly::is_assembly(new) {
        return carry_assembly(old, new);
    }
    carry_part(&single(old)?, &single(new)?)
}

/// [`carry`] from the files' bytes (`old_name`, `new_name` name them in messages).
pub fn carry_bytes(
    (old_name, old): (&str, Vec<u8>),
    (new_name, new): (&str, Vec<u8>),
) -> Result<Outcome, Error> {
    let read = |name: &str, bytes| {
        File::of_bytes(std::path::Path::new(name), bytes).map_err(|e| e.to_string())
    };
    carry(&read(old_name, old)?, &read(new_name, new)?)
}

/// A part's analysis and its feature table.
fn analysed(part: &LoadedPart) -> Result<(Value, FeatureTable), Error> {
    let analysis = crate::api::analyse(part).map_err(|e| e.to_string())?;
    let table = serde_json::from_value(analysis["table"].clone())
        .map_err(|e| format!("the feature table does not read: {e}"))?;
    Ok((analysis, table))
}

/// The stored answers of `analysis` that are not requirements (how holes are located).
fn choices(analysis: &Value) -> Map<String, Value> {
    analysis["resume"]["answers"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(id, _)| {
            !crate::feature_table::is_requirement(id)
                && !id.starts_with('_')
                && !id.starts_with("bom.")
        })
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect()
}

/// The questions of `analysis` with its PMI set aside but for its datums, as the feature table
/// asks them, by id.
fn questions(analysis: &Value) -> BTreeMap<String, crate::rules::Question> {
    let mut bare = analysis.clone();
    let mut given = Map::new();
    for item in analysis["existing"].as_array().into_iter().flatten() {
        if item["kind"] == "datum"
            && let Some(letter) = item["type"].as_str()
        {
            given.insert(format!("datum.{letter}"), item["faces"].clone());
        }
    }
    if let Some(object) = bare.as_object_mut() {
        object.insert("existing".into(), Value::Array(Vec::new()));
        object.insert("part".into(), Value::Object(Map::new()));
        for key in ["resume", "table", "dimensioning_stated", "process"] {
            object.remove(key);
        }
    }
    let all = crate::rules::with_all_defaults(&bare, &given).unwrap_or(given);
    crate::rules::questions(&bare, &all)
        .unwrap_or_default()
        .into_iter()
        .map(|q| (q.id.clone(), q))
        .collect()
}

/// A stated datum row's faces.
fn datum_faces(row: &Row) -> Vec<usize> {
    crate::edits::stated_value(row)
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .map(|f| f as usize)
        .collect()
}

/// The datum a row is on (`datum.A.flatness` → `A`), if it is a datum's own control.
fn own_datum(kind: &str) -> Option<&str> {
    let rest = kind.strip_prefix("datum.")?;
    let (letter, control) = rest.split_once('.')?;
    (!control.is_empty()).then_some(letter)
}

/// What became of OLD's datum `X` in NEW, for the requirements that cite it or sit on it.
#[derive(Clone, Debug)]
enum Datum {
    /// NEW states it on the faces OLD's are matched to, or it is carried there.
    Same,
    /// NEW states it on other faces: why, in words.
    Differs(String),
    /// Not in NEW, and not carried: why.
    Missing(String),
}

fn is_datum_letter(kind: &str) -> bool {
    kind.strip_prefix("datum.")
        .is_some_and(|l| !l.contains('.'))
}

/// OLD's part, read once for the answers that restate its rows.
struct Old<'a> {
    part: &'a LoadedPart,
    analysis: Value,
    table: FeatureTable,
    choices: Map<String, Value>,
    questions: BTreeMap<String, crate::rules::Question>,
    stored: Map<String, Value>,
    stated: crate::writer::apply::Stated,
}

/// What `row` of a file states, value included (`Size_Diameter 5 H7`), else as its table
/// shows it.
fn text_of(stated: &crate::writer::apply::Stated, row: &Row) -> Value {
    if is_datum_letter(&row.kind) && row.state == State::Stated {
        let letter = row.kind.trim_start_matches("datum.");
        return Value::from(format!("datum {letter} on faces {:?}", datum_faces(row)));
    }
    if row.id.starts_with("part.") || is_datum_letter(&row.kind) {
        return shown(row);
    }
    match stated.of_row(row) {
        Some(said) if !said.is_empty() => {
            Value::from(said.iter().map(|s| s.text()).collect::<Vec<_>>().join("; "))
        }
        _ => shown(row),
    }
}

/// Whether two rows state the same values, wherever they sit.
fn same_values(
    (a, row_a): (&crate::writer::apply::Stated, &Row),
    (b, row_b): (&crate::writer::apply::Stated, &Row),
) -> bool {
    match (a.of_row(row_a), b.of_row(row_b)) {
        (Some(x), Some(y)) if !x.is_empty() || !y.is_empty() => {
            let mut used = vec![false; y.len()];
            x.len() == y.len()
                && x.iter().all(|s| {
                    let found = (0..y.len()).find(|&j| !used[j] && s.same_value(y[j]));
                    if let Some(j) = found {
                        used[j] = true;
                    }
                    found.is_some()
                })
        }
        _ => shown(row_a) == shown(row_b),
    }
}

impl Old<'_> {
    fn text(&self, row: &Row) -> Value {
        text_of(&self.stated, row)
    }

    /// The answer that restates `row` exactly on OLD, if any: tried in order, the row's value
    /// in a fill's form, the answer OLD stores for it (a file written by 0.2), the question's
    /// default, its options and its suggestions.
    fn answer(&self, row: &Row) -> Option<Value> {
        if is_datum_letter(&row.kind) || row.id.starts_with("part.") {
            // A datum's faces (mapped onto NEW by the caller) and a part setting's text are
            // their answers; the writer checks each as it writes it.
            let value = crate::edits::stated_value(row);
            return (!value.is_null()).then_some(value);
        }
        let mut candidates: Vec<Value> = Vec::new();
        let mut push = |v: Value| {
            if !v.is_null() && !candidates.contains(&v) {
                candidates.push(v);
            }
        };
        if crate::edits::answered_by_tolerance(&row.kind) {
            push(crate::edits::stated_value(row));
        }
        if let Some(v) = self.stored.get(&row.id) {
            push(v.clone());
        }
        if let Some(q) = self.questions.get(&row.id) {
            if let Some(d) = &q.default {
                push(d.clone());
            }
            for o in &q.options {
                push(Value::from(o.as_str()));
            }
            for s in &q.suggestions {
                push(s.get("value").cloned().unwrap_or_else(|| s.clone()));
            }
        }
        candidates
            .into_iter()
            .filter(|v| !crate::feature_table::is_none_value(v))
            .find(|v| {
                crate::writer::apply::restates(
                    self.part,
                    &self.stated,
                    (&self.analysis, &self.table, &self.choices),
                    row,
                    v,
                )
            })
    }
}

/// Each OLD face's match in NEW: `Ok(face)` carried or adapted, `Err(why)` gone or ambiguous.
type FaceMap = BTreeMap<usize, Result<usize, &'static str>>;

fn face_map(revised: &Revised) -> FaceMap {
    let mut out = FaceMap::new();
    if let Some(corr) = &revised.correspondence {
        for e in &corr.faces {
            let to = match (e.class, e.new) {
                (Class::Carried | Class::Adapted, Some(n)) => Ok(n),
                (Class::Ambiguous, _) => Err("its match in the new revision is ambiguous"),
                _ => Err("it is not found in the new revision"),
            };
            out.insert(e.old, to);
        }
    }
    out
}

/// `faces` mapped onto NEW, or why they cannot be.
fn mapped(faces: &[usize], map: &FaceMap) -> Result<Vec<usize>, &'static str> {
    let mut out = faces
        .iter()
        .map(|f| {
            map.get(f)
                .copied()
                .unwrap_or(Err("it is not found in the new revision"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// Why a row on `faces` is to be checked: a feature on them moved or changed size.
fn moved(
    faces: &[usize],
    revised: &Revised,
    old_features: &BTreeMap<String, Vec<usize>>,
) -> Option<String> {
    let faces: BTreeSet<usize> = faces.iter().copied().collect();
    revised
        .features
        .iter()
        .filter(|c| c.class == "adapted")
        .find(|c| {
            c.old
                .as_ref()
                .and_then(|id| old_features.get(id))
                .is_some_and(|on| on.iter().any(|f| faces.contains(f)))
        })
        .map(|c| {
            let what: Vec<&str> = c.changes.keys().map(String::as_str).collect();
            let feature = if c.what.is_empty() {
                c.old.clone().unwrap_or_default()
            } else {
                c.what.clone()
            };
            format!("{feature} changed ({})", what.join(", "))
        })
}

/// The row of NEW's table that is OLD's `row`: the same id for a part setting or a datum, else
/// the same kind on OLD's faces matched in NEW.
fn new_row<'a>(row: &Row, table: &'a FeatureTable, map: &FaceMap) -> Result<&'a Row, String> {
    if row.id.starts_with("part.") || row.id.starts_with("datum.") {
        return table
            .row(&row.id)
            .ok_or_else(|| format!("the new revision has no {}", row.id));
    }
    let faces = mapped(&row.faces, map).map_err(|why| format!("its feature: {why}"))?;
    let found: Vec<&Row> = table
        .rows
        .iter()
        .filter(|r| r.kind == row.kind && r.faces == faces)
        .collect();
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(
            "its feature is not found in the new revision (its faces match no one feature there)"
                .into(),
        ),
        _ => Err("its match in the new revision is ambiguous".into()),
    }
}

fn shown(row: &Row) -> Value {
    row.value.clone().unwrap_or(Value::Null)
}

fn not_carried_row(row: &Row, reason: impl Into<String>) -> NotCarried {
    NotCarried {
        id: Some(row.id.clone()),
        number: None,
        kind: row.kind.clone(),
        r#type: String::new(),
        faces: row.faces.clone(),
        text: shown(row)
            .as_str()
            .map_or_else(|| shown(row).to_string(), str::to_owned),
        part: None,
        reason: reason.into(),
    }
}

/// [`carry`] of a file of one part onto another.
fn carry_part(old: &LoadedPart, new: &LoadedPart) -> Result<Outcome, Error> {
    let revised = revision::diff(old, new).map_err(|e| e.to_string())?;
    let map = face_map(&revised);
    let (old_analysis, old_table) = analysed(old)?;
    let (_, new_table) = analysed(new)?;
    let new_stated = crate::writer::apply::Stated::of(new)?;
    let old_features: BTreeMap<String, Vec<usize>> = old_analysis["features"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let faces = f["faces"]
                .as_array()?
                .iter()
                .filter_map(Value::as_u64)
                .map(|x| x as usize)
                .collect();
            Some((f["id"].as_str()?.to_owned(), faces))
        })
        .collect();
    let reader = Old {
        part: old,
        choices: choices(&old_analysis),
        questions: questions(&old_analysis),
        stored: crate::resume::answers_for(old)
            .map_err(|e| e.to_string())?
            .unwrap_or_default(),
        stated: crate::writer::apply::Stated::of(old)?,
        analysis: old_analysis,
        table: old_table,
    };

    let mut report = Report {
        new_unstated: revised.new_unstated.clone(),
        ..Report::default()
    };
    let mut list = EditList::default();

    // OLD's datums first: what each became in NEW decides what the requirements citing it, or
    // sitting on it, can carry.
    let mut datums: BTreeMap<String, Datum> = BTreeMap::new();
    for row in reader
        .table
        .rows
        .iter()
        .filter(|r| is_datum_letter(&r.kind) && r.state == State::Stated)
    {
        let letter = row.kind.trim_start_matches("datum.").to_string();
        let old_faces = datum_faces(row);
        let state = match (new_row(row, &new_table, &map), mapped(&old_faces, &map)) {
            (Err(why), _) => Datum::Missing(why),
            (_, Err(why)) => Datum::Missing(format!("its faces: {why}")),
            (Ok(target), Ok(on)) => match target.state {
                State::Stated if datum_faces(target) == on => Datum::Same,
                State::Stated => Datum::Differs(format!(
                    "the new file states datum {letter} on faces {:?}; the old file's is on \
                     faces {old_faces:?}, faces {on:?} in the new revision",
                    datum_faces(target)
                )),
                State::DecidedNone => {
                    Datum::Missing(format!("the new file decided datum {letter} none"))
                }
                State::Gap => match new_table.rows.iter().find(|other| {
                    is_datum_letter(&other.kind)
                        && other.state == State::Stated
                        && datum_faces(other).iter().any(|f| on.contains(f))
                }) {
                    // A second datum on the same faces: the new file's stands.
                    Some(other) => Datum::Missing(format!(
                        "its faces {on:?} in the new revision carry the new file's {}",
                        other.kind.replace('.', " ")
                    )),
                    None => Datum::Same,
                },
            },
        };
        datums.insert(letter, state);
    }
    // Why a row cannot be carried (`Err`), or why it is to be checked (`Ok(Some)`), from the
    // datums it cites or sits on.
    let depends = |row: &Row| -> Result<Option<String>, String> {
        let mut check = None;
        if let Some(letter) = own_datum(&row.kind) {
            match datums.get(letter) {
                Some(Datum::Differs(why)) => {
                    return Err(format!(
                        "it is on datum {letter}, which is not the old file's: {why}; the old \
                         file's requirement is not written onto the new file's datum"
                    ));
                }
                Some(Datum::Missing(why)) => {
                    return Err(format!(
                        "it is on datum {letter}, which is not carried: {why}"
                    ));
                }
                _ => {}
            }
        }
        let cited: BTreeSet<String> = reader
            .stated
            .of_row(row)
            .into_iter()
            .flatten()
            .flat_map(|s| s.datums())
            .collect();
        for letter in cited {
            match datums.get(&letter) {
                Some(Datum::Missing(why)) => {
                    return Err(format!(
                        "it cites datum {letter}, which is not carried: {why}"
                    ));
                }
                None => {
                    return Err(format!(
                        "it cites datum {letter}, which the old file does not state"
                    ));
                }
                Some(Datum::Differs(why)) => {
                    check = Some(format!("it cites datum {letter}, which differs: {why}"));
                }
                Some(Datum::Same) => {}
            }
        }
        Ok(check)
    };

    // Stated rows no answer restates, with their row in NEW.
    let mut unanswered: Vec<(&Row, &Row)> = Vec::new();
    for row in &reader.table.rows {
        if row.state == State::Gap {
            continue;
        }
        let target = match new_row(row, &new_table, &map) {
            Ok(target) => target,
            Err(why) => {
                report.not_carried.push(not_carried_row(row, why));
                continue;
            }
        };
        if target.state != State::Gap {
            // NEW states it, or decided it none: NEW's stands.
            let same = match (row.state, target.state) {
                (State::DecidedNone, State::DecidedNone) => true,
                // A part setting by its text; a datum by its faces, matched in NEW; any other
                // by its requirements' values.
                (State::Stated, State::Stated) if row.id.starts_with("part.") => {
                    shown(row) == shown(target)
                }
                (State::Stated, State::Stated) if is_datum_letter(&row.kind) => {
                    matches!(
                        datums.get(row.kind.trim_start_matches("datum.")),
                        Some(Datum::Same)
                    )
                }
                (State::Stated, State::Stated) => {
                    same_values((&reader.stated, row), (&new_stated, target))
                }
                _ => false,
            };
            if !same {
                report.kept.push(Kept {
                    old: row.id.clone(),
                    new: target.id.clone(),
                    old_value: reader.text(row),
                    new_value: text_of(&new_stated, target),
                });
            }
            continue;
        }
        if row.state == State::DecidedNone {
            list.none.insert(target.id.clone());
            report.decided_none.push(Carried {
                old: row.id.clone(),
                new: target.id.clone(),
                value: shown(row),
                stated: None,
                check: None,
                with: None,
            });
            continue;
        }
        if is_datum_letter(&row.kind)
            && let Some(Datum::Missing(why)) = datums.get(row.kind.trim_start_matches("datum."))
        {
            report.not_carried.push(not_carried_row(row, why.clone()));
            continue;
        }
        let datum_check = match depends(row) {
            Ok(check) => check,
            Err(why) => {
                let mut n = not_carried_row(row, why);
                n.text = text(&reader.text(row));
                report.not_carried.push(n);
                continue;
            }
        };
        let Some(mut value) = reader.answer(row) else {
            unanswered.push((row, target));
            continue;
        };
        if is_datum_letter(&row.kind) {
            // Checked above: its faces are matched in NEW.
            value = json!(mapped(&datum_faces(row), &map).unwrap_or_default());
        }
        list.fill.insert(target.id.clone(), value.clone());
        let check = moved(&row.faces, &revised, &old_features).or(datum_check);
        let entry = Carried {
            old: row.id.clone(),
            new: target.id.clone(),
            value,
            stated: Some(reader.text(row)),
            check: check.clone(),
            with: None,
        };
        if check.is_some() {
            report.to_check.push(entry);
        } else {
            report.carried.push(entry);
        }
    }
    // A row whose requirements a carried row states too (a hole's thread callout, which
    // states its thread depth) carries with it; any other is not carried.
    for (row, target) in unanswered {
        let carrier = reader.table.rows.iter().find(|other| {
            other.id != row.id
                && list.fill.contains_key(&new_id(&report, &other.id))
                && !row.requirements.is_empty()
                && row
                    .requirements
                    .iter()
                    .all(|r| other.requirements.iter().any(|q| q == r))
        });
        match carrier {
            Some(other) => {
                let entry = Carried {
                    old: row.id.clone(),
                    new: target.id.clone(),
                    value: Value::Null,
                    stated: Some(reader.text(row)),
                    check: moved(&row.faces, &revised, &old_features),
                    with: Some(new_id(&report, &other.id)),
                };
                if entry.check.is_some() {
                    report.to_check.push(entry);
                } else {
                    report.carried.push(entry);
                }
            }
            None => {
                let mut n = not_carried_row(
                    row,
                    "no answer restates it exactly: Specify does not write this requirement \
                     as the file states it",
                );
                n.text = text(&reader.text(row));
                report.not_carried.push(n);
            }
        }
    }
    for r in &reader.table.file_pmi {
        report.not_carried.push(file_pmi_not_carried(r, None));
    }

    // The old file's number history, read on the new part through the face matching.
    let mut numbers = Map::new();
    if let Some(corr) = &revised.correspondence {
        crate::characteristics::carry_revision(old, new, corr, &mut numbers)?;
    }
    let numbers = Some(&numbers).filter(|n| !n.is_empty());
    // A fill the new revision cannot take, on a feature that changed or for a datum or general
    // tolerance it would need and does not have, is reported and left out; the list that is
    // left is applied all or nothing.
    let mut applied = crate::writer::apply::apply_numbered(new, &list, numbers);
    for _ in 0..list.len() {
        let Err(failures) = &applied else { break };
        let Some(pruned) = prunable(failures, &report) else {
            break;
        };
        for (id, why) in pruned {
            list.fill.remove(&id);
            leave_out(&mut report, &id, &reader, why);
        }
        // What carried with a fill that was left out goes with it.
        let orphans: Vec<String> = report
            .carried
            .iter()
            .chain(&report.to_check)
            .filter(|c| c.with.as_ref().is_some_and(|w| !list.fill.contains_key(w)))
            .map(|c| c.new.clone())
            .collect();
        for id in orphans {
            leave_out(
                &mut report,
                &id,
                &reader,
                "the requirement it carried with is not carried".into(),
            );
        }
        applied = crate::writer::apply::apply_numbered(new, &list, numbers);
    }
    match applied {
        Ok(applied) => {
            report.written = true;
            report.retired = applied.retired.clone();
            report.write = applied.to_dict();
            if let Some(object) = report.write.as_object_mut() {
                object.remove("output");
            }
            let out = crate::part::load_bytes("carried.step", applied.bytes.clone())
                .map_err(|e| e.to_string())?;
            numbered(&mut report, &reader, old, &out, &map)?;
            Ok(Outcome {
                bytes: Some(applied.bytes),
                report,
            })
        }
        Err(failures) => Ok(refused(report, failures)),
    }
}

/// The carry refused, all or nothing: nothing is written, and `failures` say why.
fn refused(mut report: Report, failures: Vec<Failure>) -> Outcome {
    report.written = false;
    report.refused = failures;
    Outcome {
        bytes: None,
        report,
    }
}

/// The fills of `failures` the carry leaves out rather than refuse for: each failing entry a
/// fill, either on a feature that moved or changed size (listed to check), or one that writes
/// nothing on the new part as it is (its feature changed so that the answer has nothing to
/// apply to, or it wants a datum or the general tolerance the new revision does not have). `None` when any failure
/// is of another kind: then the whole carry is refused.
fn prunable(failures: &[Failure], report: &Report) -> Option<Vec<(String, String)>> {
    let mut out = Vec::new();
    for f in failures {
        let id = f
            .entry
            .strip_prefix("fill[")
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(|quoted| serde_json::from_str::<String>(quoted).ok())?;
        let changed = report.to_check.iter().any(|c| c.new == id);
        // "writes no requirement on this part as it is": the new geometry has nothing it
        // applies to, or wants a datum or the general tolerance the new file does not have.
        let wanting = f
            .reason
            .contains("writes no requirement on this part as it is");
        if !(changed || wanting) {
            return None;
        }
        out.push((id, format!("the new revision cannot take it: {}", f.reason)));
    }
    (!out.is_empty()).then_some(out)
}

/// Moves the carried entry for NEW's `id` to not carried, with `why`.
fn leave_out(report: &mut Report, id: &str, reader: &Old<'_>, why: String) {
    let mut entry = None;
    for list in [&mut report.carried, &mut report.to_check] {
        if let Some(i) = list.iter().position(|c| c.new == id) {
            entry = Some(list.remove(i));
        }
    }
    let Some(entry) = entry else { return };
    let Some(row) = reader.table.row(&entry.old) else {
        return;
    };
    let mut n = not_carried_row(row, why);
    n.text = text(&reader.text(row));
    report.not_carried.push(n);
}

/// Each carried requirement's number in `out`: OLD's (`numbers_carried`), or a new one, listed in
/// `renumbered`.
fn numbered(
    report: &mut Report,
    reader: &Old<'_>,
    old: &LoadedPart,
    out: &LoadedPart,
    map: &FaceMap,
) -> Result<(), Error> {
    let carried: BTreeSet<u64> = report
        .carried
        .iter()
        .chain(&report.to_check)
        .filter_map(|c| reader.table.row(&c.old))
        .flat_map(|r| &r.requirements)
        .filter_map(|r| r.number.as_deref()?.strip_prefix('#')?.parse().ok())
        .collect();
    let now = crate::characteristics::read(out)?;
    for c in crate::characteristics::read(old)?
        .into_iter()
        .filter(|c| carried.contains(&c.number))
    {
        let (Ok(faces), Ok(reference)) = (mapped(&c.faces, map), mapped(&c.reference, map)) else {
            continue;
        };
        let sorted = |v: &[usize]| {
            let mut v = v.to_vec();
            v.sort_unstable();
            v.dedup();
            v
        };
        let Some(found) = now.iter().find(|n| {
            n.kind == c.kind
                && n.r#type == c.r#type
                && sorted(&n.faces) == faces
                && sorted(&n.reference) == reference
        }) else {
            continue;
        };
        if found.number == c.number {
            report.numbers_carried = true;
        } else {
            report.renumbered.push(Renumbered {
                old: c.number,
                new: found.number,
                kind: c.kind.clone(),
                r#type: c.r#type.clone(),
                faces: found.faces.clone(),
            });
        }
    }
    Ok(())
}

/// A requirement on no feature-and-kind of OLD, reported not carried.
fn file_pmi_not_carried(r: &crate::feature_table::Requirement, part: Option<&str>) -> NotCarried {
    NotCarried {
        id: None,
        number: r.number.clone(),
        kind: r.kind.clone(),
        r#type: r.r#type.clone(),
        faces: r.faces.clone(),
        text: r.text.clone(),
        part: part.map(str::to_owned),
        reason: "it sits on no feature-and-kind Specify understands (the dimension planner's \
                 own, or another tool's): carry copies feature-and-kinds only"
            .into(),
    }
}

/// [`carry`] of an assembly: each new part matched to an old one by its geometry keeps its BOM
/// item number and answers, and its material, where NEW states none (NEW's own stand, and a
/// difference is listed as kept); the old BOM history with its retired numbers carries unless
/// NEW numbers its own items. Only appended: every entity of NEW keeps its bytes. The in-place
/// writer edits a file of one part, so the parts' own requirements are not carried here yet;
/// each is reported, with each part's PMI on no feature-and-kind.
fn carry_assembly(old: &File, new: &File) -> Result<Outcome, Error> {
    let (before, after) = (
        old.load_all().map_err(|e| e.to_string())?,
        new.load_all().map_err(|e| e.to_string())?,
    );
    let mut report = Report::default();
    for part in &before {
        let (_, table) = analysed(part)?;
        for row in table
            .rows
            .iter()
            .filter(|r| r.state != State::Gap && r.id != "part.material")
        {
            let mut n = not_carried_row(
                row,
                "a part of an assembly: the in-place writer edits a file of one part, so carry \
                 takes an assembly's BOM and materials only",
            );
            n.part = Some(part.name.clone());
            report.not_carried.push(n);
        }
        for r in &table.file_pmi {
            report
                .not_carried
                .push(file_pmi_not_carried(r, Some(&part.name)));
        }
    }
    let carried = crate::bom::carry_assembly(&before, &after, new.bytes().as_ref())?;
    for k in &carried.kept {
        report.kept.push(Kept {
            old: format!("{}: {}", k.old_part, k.key),
            new: format!("{}: {}", k.new_part, k.key),
            old_value: k.old.clone(),
            new_value: k.new.clone(),
        });
    }
    for (part, fields) in &carried.fields {
        for (key, value) in fields {
            let id = format!("{part}: bom.{key}");
            report.carried.push(Carried {
                old: id.clone(),
                new: id,
                value: value.clone(),
                stated: None,
                check: None,
                with: None,
            });
        }
    }
    for (part, material) in &carried.materials {
        let id = format!("{part}: part.material");
        report.carried.push(Carried {
            old: id.clone(),
            new: id,
            value: Value::from(material.as_str()),
            stated: None,
            check: None,
            with: None,
        });
    }
    report.bom_carried = carried.bytes.is_some();
    report.numbers_carried = carried.numbers;
    report.written = true;
    Ok(Outcome {
        bytes: Some(
            carried
                .bytes
                .unwrap_or_else(|| new.bytes().as_ref().clone()),
        ),
        report,
    })
}

impl Report {
    /// The report as JSON, with its Markdown.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        out.insert("verdict".into(), Value::from(self.verdict()));
        out.insert("summary".into(), Value::from(self.summary()));
        if let Value::Object(fields) =
            serde_json::to_value(self).expect("a carry report serializes")
        {
            out.extend(fields);
        }
        out.insert("markdown".into(), Value::from(self.markdown()));
        Value::Object(out)
    }

    /// As `diff`'s: `ok` when everything OLD states was carried (or NEW states it alike), and
    /// `decide` when anything is not carried or is to be checked.
    pub fn verdict(&self) -> &'static str {
        if self.not_carried.is_empty() && self.to_check.is_empty() {
            "ok"
        } else {
            "decide"
        }
    }

    /// The counts, plainly: `Carried 12, not carried 3, to check 1.` (decided none and kept too,
    /// when there are any), and `Nothing was written.` when the carry was refused.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "Carried {}, not carried {}, to check {}",
            self.carried.len(),
            self.not_carried.len(),
            self.to_check.len()
        );
        if !self.decided_none.is_empty() {
            s.push_str(&format!(
                ", decided none carried {}",
                self.decided_none.len()
            ));
        }
        if !self.kept.is_empty() {
            s.push_str(&format!(
                ", kept (the new file differs) {}",
                self.kept.len()
            ));
        }
        s.push('.');
        if !self.refused.is_empty() {
            s.push_str(" Nothing was written.");
        }
        s
    }

    /// The report in Markdown.
    pub fn markdown(&self) -> String {
        use std::fmt::Write;
        let mark = if self.verdict() == "ok" {
            "✅"
        } else {
            "⚠️"
        };
        let mut s = format!("# Carry\n\n{mark} {}\n\n", self.summary());
        if !self.refused.is_empty() {
            s.push_str(
                "Nothing was written. A carry is applied all or nothing, and these could not be \
                 applied:\n\n",
            );
            for f in &self.refused {
                let _ = writeln!(s, "- {}: {}", f.entry, f.reason);
            }
            s.push('\n');
        }
        let carried = |s: &mut String, title: &str, rows: &[Carried]| {
            if rows.is_empty() {
                return;
            }
            let _ = writeln!(s, "## {title}\n");
            for c in rows {
                let stated = c
                    .stated
                    .as_ref()
                    .map(|v| format!(" (old file: {})", text(v)))
                    .unwrap_or_default();
                let check = c
                    .check
                    .as_ref()
                    .map(|why| format!(": {why}"))
                    .unwrap_or_default();
                let value = match &c.with {
                    Some(other) => format!("with {other}"),
                    None => text(&c.value),
                };
                let _ = writeln!(s, "- {}: {value}{stated}{check}", c.new);
            }
            s.push('\n');
        };
        carried(&mut s, "Carried", &self.carried);
        carried(&mut s, "Carried, to check", &self.to_check);
        carried(&mut s, "Decided none, carried", &self.decided_none);
        if !self.kept.is_empty() {
            s.push_str("## Kept: the new file states it differently\n\n");
            for k in &self.kept {
                let _ = writeln!(
                    s,
                    "- {}: new file {} stands (old file: {})",
                    k.new,
                    text(&k.new_value),
                    text(&k.old_value)
                );
            }
            s.push('\n');
        }
        if !self.not_carried.is_empty() {
            s.push_str("## Not carried\n\n");
            for n in &self.not_carried {
                let what =
                    n.id.clone()
                        .or_else(|| n.number.clone())
                        .unwrap_or_else(|| n.kind.clone());
                let part = n
                    .part
                    .as_ref()
                    .map(|p| format!(" (part {p})"))
                    .unwrap_or_default();
                let _ = writeln!(
                    s,
                    "- {what}{part}: {} {} on faces {:?}: {}",
                    n.kind, n.text, n.faces, n.reason
                );
            }
            s.push('\n');
        }
        if self.numbers_carried {
            s.push_str("Characteristic numbers carried with the old file's number history.\n\n");
        }
        if self.bom_carried {
            s.push_str("BOM item numbers and answers carried with the old BOM history.\n\n");
        }
        if !self.new_unstated.is_empty() {
            let what: Vec<&str> = self.new_unstated.iter().map(|u| u.what.as_str()).collect();
            let _ = writeln!(
                s,
                "New in this revision with nothing stated: {}.\n",
                what.join("; ")
            );
        }
        s
    }
}

/// The id in NEW of OLD's carried row `old`.
fn new_id(report: &Report, old: &str) -> String {
    report
        .carried
        .iter()
        .chain(&report.to_check)
        .find(|c| c.old == old)
        .map_or_else(String::new, |c| c.new.clone())
}

fn text(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list the writer itself refuses (a failure of the whole list, or of an entry on a
    /// feature that did not change, for a reason other than having nothing to apply to) is not
    /// trimmed: the whole carry is refused, and nothing is written.
    #[test]
    fn a_list_the_writer_refuses_writes_nothing() {
        let mut report = Report::default();
        report.carried.push(Carried {
            old: "datum.B".into(),
            new: "datum.B".into(),
            value: json!([19]),
            stated: None,
            check: None,
            with: None,
        });
        for failures in [
            vec![Failure {
                entry: "(the whole list)".into(),
                reason: "haecceity will not write it".into(),
            }],
            vec![Failure {
                entry: "fill[\"datum.B\"]".into(),
                reason: "[19] is not an answer to datum.B".into(),
            }],
        ] {
            assert_eq!(prunable(&failures, &report), None);
            let outcome = refused(report.clone(), failures.clone());
            assert!(outcome.bytes.is_none() && !outcome.report.written);
            assert_eq!(outcome.report.refused, failures);
            assert!(outcome.report.summary().ends_with("Nothing was written."));
        }
        // One on a feature that changed is left out instead.
        report.to_check.push(report.carried.pop().unwrap());
        let failures = vec![Failure {
            entry: "fill[\"datum.B\"]".into(),
            reason: "[19] is not an answer to datum.B".into(),
        }];
        assert_eq!(prunable(&failures, &report).unwrap()[0].0, "datum.B");
    }
}
