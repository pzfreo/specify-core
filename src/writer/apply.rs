//! Applying an edit list to a file in place (Specify 0.3.0, piece 2; ADR P11).
//!
//! A save applies an explicit list and touches only what it names: every other entity keeps
//! its bytes and its number, whoever wrote it, and a change in Specify's rules never alters a
//! file. The operations:
//!
//! - **fill** (and `accept_suggestions`): a gap's value becomes PMI, planned in add mode with
//!   the file's items fixed (its datums reused, its dimensions the planner's authorities) and
//!   never stating again what the file states;
//! - **change**: a stated feature-and-kind's requirements are taken out and its new value is
//!   planned as a fill, in one edit;
//! - **remove**: one requirement, by its characteristic number, is taken out; its
//!   feature-and-kind is a gap again unless the list also decides it none;
//! - **none**: the feature-and-kind is recorded decided none.
//!
//! **Numbers.** A change of limits plans the same measurement, which takes its number back. A
//! change that makes it a different measurement gets a new number, and the old one is retired;
//! a removed requirement's number is retired. Retired numbers stay below the high-water mark and
//! are never reused (`characteristics::Allocation::Edit`).
//!
//! **All or nothing.** The list is validated against the file's feature table first
//! ([`crate::edits::validate`]); any failing entry refuses the whole list and nothing is written.
//! What is stored beside the PMI is only the decided-none list (and choices that are not
//! requirements, such as how holes are located) and the number history; stored answers that
//! repeat the file's PMI are dropped.

use std::collections::{BTreeMap, BTreeSet};

use haecceity::pmi::{DimensionKind, Mode, PartPmi, ToleranceTarget};
use serde_json::{Map, Value as Json};

use super::{Source, WriteReport, drop_stated, part_pmi, requirements, verify, write_by_modes};
use crate::edits::{self, EditList, Failure, Outcome};
use crate::feature_table::{FeatureTable, State};
use crate::part::{self, LoadedPart};

/// A list applied: the file's new bytes, and what was done.
#[derive(Clone, Debug)]
pub struct Applied {
    /// The file written (the original bytes when the list changes nothing).
    pub bytes: Vec<u8>,
    pub report: WriteReport,
    /// The list's validation against the file.
    pub outcome: Outcome,
    /// The characteristic numbers this save retired.
    pub retired: Vec<u64>,
    /// Nothing changed: the bytes are the file's own.
    pub unchanged: bool,
}

fn whole(why: impl Into<String>) -> Vec<Failure> {
    vec![Failure {
        entry: "(the whole list)".into(),
        reason: why.into(),
    }]
}

fn entry_failure(entry: String, reason: String) -> Failure {
    Failure { entry, reason }
}

/// The part's analysis and its feature table, read from the file.
fn table_of(loaded: &LoadedPart) -> Result<(Json, FeatureTable), String> {
    let analysis = crate::api::analyse(loaded).map_err(|e| e.to_string())?;
    let table = serde_json::from_value(analysis["table"].clone())
        .map_err(|e| format!("the feature table does not read: {e}"))?;
    Ok((analysis, table))
}

/// What this writer cannot apply yet, whatever the file states.
fn limits(list: &EditList) -> Vec<Failure> {
    list.items
        .change
        .keys()
        .map(|n| {
            entry_failure(
                format!("items.change[{}]", Json::String(n.to_string())),
                format!(
                    "changing a single requirement by its number is not supported yet: change \
                     its feature-and-kind (change), or remove {n} (items.remove)"
                ),
            )
        })
        .collect()
}

/// The list validated against `table`, with this writer's limits.
fn validated(list: &EditList, table: &FeatureTable) -> (EditList, Outcome) {
    let list = edits::normalise(list, table);
    let mut outcome = edits::validate(&list, table);
    outcome.failures.extend(limits(&list));
    outcome.applicable = outcome.failures.is_empty();
    (list, outcome)
}

/// `text` (an edit list, or a 0.2 answers file) checked against `loaded`'s file, all or
/// nothing. Nothing is written.
///
/// The list is applied in memory (the same run as [`apply`]), so a list this check passes is
/// one `apply` writes, and one it refuses is refused with the same reasons.
pub fn check(loaded: &LoadedPart, text: &str) -> Outcome {
    let list = match edits::read(text) {
        Ok(list) => list,
        Err(failures) => return Outcome::malformed(failures),
    };
    let table = match table_of(loaded) {
        Ok((_, table)) => table,
        Err(why) => return Outcome::malformed(whole(why)),
    };
    let mut outcome = validated(&list, &table).1;
    if outcome.failures.is_empty()
        && let Err(failures) = apply(loaded, &list)
    {
        outcome.failures = failures;
        outcome.applicable = false;
    }
    outcome
}

/// `text` (an edit list, or a 0.2 answers file) applied to the part `part` (None: a file of
/// one part) of the STEP `bytes`: the API the page and the bindings call.
pub fn apply_bytes(
    name: &str,
    bytes: Vec<u8>,
    part: Option<usize>,
    text: &str,
) -> Result<Applied, Vec<Failure>> {
    let list = edits::read(text)?;
    let loaded = part::File::of_bytes(std::path::Path::new(name), bytes)
        .and_then(|f| f.load(part))
        .map_err(|e| whole(e.to_string()))?;
    apply(&loaded, &list)
}

/// [`apply`], the result written to `output` (staged beside it, which is replaced only once
/// all is well; a `.gz` destination is gzipped). Nothing is written when the list is refused.
#[cfg(not(target_arch = "wasm32"))]
pub fn apply_to(
    loaded: &LoadedPart,
    list: &EditList,
    output: &std::path::Path,
) -> Result<Applied, Vec<Failure>> {
    let mut applied = apply(loaded, list)?;
    super::put(output, applied.bytes.clone()).map_err(|e| whole(e.to_string()))?;
    applied.report.output = output.to_path_buf();
    Ok(applied)
}

impl Applied {
    /// The report as `write` prints it: the writer's report, with the numbers retired.
    pub fn to_dict(&self) -> Json {
        let mut out = self.report.to_dict();
        out["retired"] = Json::from(self.retired.clone());
        out["unchanged"] = Json::Bool(self.unchanged);
        out
    }
}

/// The haecceity collections of each requirement of the part, with the features each refers
/// to (a removed requirement's own feature gives way with it unless something kept uses it).
fn features_of(p: &PartPmi, (collection, i): (&str, usize)) -> Vec<usize> {
    let feature = |f: haecceity::pmi::FeatureId| f.0;
    match collection {
        "dimensions" => match &p.dimensions[i].kind {
            DimensionKind::Size { feature: f, .. } => vec![feature(*f)],
            DimensionKind::Location { from, to, .. } => vec![feature(*from), feature(*to)],
        },
        "tolerances" => match &p.tolerances[i].target {
            ToleranceTarget::Feature(f) => vec![feature(*f)],
            ToleranceTarget::Relation {
                relating, related, ..
            } => vec![feature(*relating), feature(*related)],
            ToleranceTarget::Dimension(_) | ToleranceTarget::WholePart => vec![],
        },
        "datums" => p.datums[i].feature().map(feature).into_iter().collect(),
        "threads" => vec![feature(p.threads[i].feature)],
        "knurls" => vec![feature(p.knurls[i].feature)],
        "surface_textures" => p.surface_textures[i].on.map(feature).into_iter().collect(),
        _ => vec![],
    }
}

/// The items stating a part setting (`part.material` → `material`), by collection and index.
fn setting_items(p: &PartPmi, key: &str) -> Result<Vec<(String, usize)>, String> {
    use haecceity::pmi::AttributeValue;
    let at = |c: &str, i: usize| (c.to_string(), i);
    let mut out = Vec::new();
    match key {
        "material" => out.extend(p.material.as_ref().map(|_| at("material", 0))),
        "general_tolerance" => {
            out.extend((0..p.general.len()).map(|i| at("general", i)));
            out.extend(p.attributes.iter().enumerate().filter_map(|(i, set)| {
                crate::existing::general_tolerance_text(set).map(|_| at("attributes", i))
            }));
        }
        "surface_finish" => out.extend(
            p.surface_textures
                .iter()
                .enumerate()
                .filter(|(_, t)| t.on.is_none())
                .map(|(i, _)| at("surface_textures", i)),
        ),
        _ => {
            let Some((_, kind)) = crate::existing::PART_NOTES.iter().find(|(k, _)| *k == key)
            else {
                return Err(format!(
                    "part.{key} is not a setting this writer can change"
                ));
            };
            for (i, set) in p.attributes.iter().enumerate() {
                if set.on.is_some() || set.name != super::SEMANTIC_TEXT {
                    continue;
                }
                let mine = set.items.iter().filter(|(name, _)| name == kind).count();
                if mine == 0 {
                    continue;
                }
                if mine != set.items.len()
                    || !set.items.iter().all(|(_, v)| {
                        matches!(v, AttributeValue::Text(_) | AttributeValue::Integer(_))
                    })
                {
                    return Err(format!(
                        "part.{key} is stated in one set with other notes; remove them together"
                    ));
                }
                out.push(at("attributes", i));
            }
            out.extend(
                p.notes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.on.is_none() && n.kind == *kind)
                    .map(|(i, _)| at("notes", i)),
            );
        }
    }
    Ok(out)
}

/// The requirement of a table row as an item of the file: by its number when it has one, else
/// by kind, type and faces.
fn locate(
    req: &crate::feature_table::Requirement,
    located: &[crate::characteristics::Located],
    taken: &BTreeSet<(String, usize)>,
) -> Option<(String, usize)> {
    if let Some(n) = req.number.as_deref().and_then(|n| n.strip_prefix('#')) {
        let n: u64 = n.parse().ok()?;
        return located
            .iter()
            .find(|l| l.number == Some(n))
            .map(|l| (l.at.0.to_string(), l.at.1));
    }
    let faces: BTreeSet<usize> = req.faces.iter().copied().collect();
    let (kind, r#type) = match (req.kind.as_str(), req.r#type.as_str()) {
        ("note", "internal thread") => ("thread", "Internal"),
        ("note", "external thread") => ("thread", "External"),
        ("note", "knurl") => ("knurl", "knurl"),
        (kind, t) => (kind, t),
    };
    located
        .iter()
        .find(|l| {
            let mut on: BTreeSet<usize> = l.faces.iter().copied().collect();
            on.extend(&l.reference);
            l.kind == kind
                && l.r#type == r#type
                && on == faces
                && !taken.contains(&(l.at.0.to_string(), l.at.1))
        })
        .map(|l| (l.at.0.to_string(), l.at.1))
}

/// Whether the intent row `row` is a basic location the file's PMI `before` states already: a
/// basic linear distance from the same faces to faces that include all of the row's (a
/// location of a group the row's feature belongs to).
fn covered_by_file(row: &Map<String, Json>, before: &PartPmi) -> bool {
    use haecceity::pmi::{DimTolerance, LocationKind};
    let ids = |key: &str| -> BTreeSet<usize> {
        row.get(key)
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_u64)
            .map(|f| f as usize)
            .collect()
    };
    if row.get("kind").and_then(Json::as_str) != Some("location")
        || row.get("basic") != Some(&Json::Bool(true))
    {
        return false;
    }
    let (faces, reference) = (ids("faces"), ids("reference"));
    if faces.is_empty() || reference.is_empty() {
        return false;
    }
    before.dimensions.iter().any(|d| {
        let DimensionKind::Location {
            from,
            to,
            kind: LocationKind::LinearDistance,
            path: None,
            angle: None,
            ..
        } = &d.kind
        else {
            return false;
        };
        let set = |f| -> BTreeSet<usize> {
            crate::existing::faces_of(before, &[f])
                .into_iter()
                .collect()
        };
        let (a, b) = (set(*from), set(*to));
        matches!(d.tolerance, DimTolerance::Basic)
            && ((a == reference && faces.is_subset(&b)) || (b == reference && faces.is_subset(&a)))
    })
}

/// Whether two requirements of the table are the same item: by number when both have one,
/// else by kind, type and faces.
fn same_requirement(
    a: &crate::feature_table::Requirement,
    b: &crate::feature_table::Requirement,
) -> bool {
    match (&a.number, &b.number) {
        (Some(x), Some(y)) => x == y,
        _ => a.kind == b.kind && a.r#type == b.r#type && a.faces == b.faces,
    }
}

/// An answer that records "none" for `id`: its suggestion when that is a "none", an empty list
/// for a datum, else `"none"`.
fn none_answer(table: &FeatureTable, id: &str) -> Json {
    let row = table.row(id);
    if let Some(value) = row
        .filter(|r| r.state == State::DecidedNone)
        .and_then(|r| r.value.clone())
    {
        return value;
    }
    if let Some(s) = row
        .and_then(|r| r.suggestion.clone())
        .filter(crate::feature_table::is_none_value)
        .filter(|s| !s.is_null())
    {
        return s;
    }
    if id.strip_prefix("datum.").is_some_and(|l| !l.contains('.')) {
        Json::Array(Vec::new())
    } else {
        Json::from("none")
    }
}

fn kept_bytes(doc: &haecceity::p21::Document, id: u64) -> Option<&[u8]> {
    doc.span(id).map(|s| &doc.bytes()[s])
}

/// Applies `list` to `loaded`'s part, all or nothing: the whole list, or nothing and every
/// failing entry with its reason.
pub fn apply(loaded: &LoadedPart, list: &EditList) -> Result<Applied, Vec<Failure>> {
    if loaded.part_count > 1 {
        return Err(whole(
            "an edit list applies to a file of one part; a part of an assembly is not supported \
             yet",
        ));
    }
    let (analysis, table) = table_of(loaded).map_err(whole)?;
    let (list, outcome) = validated(list, &table);
    if !outcome.failures.is_empty() {
        return Err(outcome.failures);
    }
    let unchanged: BTreeSet<&str> = outcome.unchanged.iter().map(String::as_str).collect();
    let skip = |section: &str, id: &str| {
        unchanged.contains(format!("{section}[{}]", Json::String(id.to_owned())).as_str())
    };

    // What the list writes (by question id, with its value), and what it decides writes
    // nothing (with the answer stored for it).
    let mut given: BTreeMap<String, Json> = BTreeMap::new();
    let mut none: BTreeMap<String, Json> = BTreeMap::new();
    for (id, value) in &list.fill {
        if !skip("fill", id) {
            given.insert(id.clone(), value.clone());
        }
    }
    for (id, change) in &list.change {
        given.insert(id.clone(), change.to.clone());
    }
    for id in &list.accept_suggestions {
        if let Some(s) = table.row(id).and_then(|r| r.suggestion.clone()) {
            given.insert(id.clone(), s);
        }
    }
    // A value that writes nothing ("none"; "general", a hole left to the general tolerance)
    // is a decision, stored as given.
    given.retain(|id, value| {
        let nothing = crate::feature_table::leaves_nothing(value);
        if nothing {
            let stored = if crate::feature_table::is_none_value(value) {
                none_answer(&table, id)
            } else {
                value.clone()
            };
            none.insert(id.clone(), stored);
        }
        !nothing
    });
    for id in &list.none {
        if !skip("none", id) {
            none.insert(id.clone(), none_answer(&table, id));
        }
    }
    if given.is_empty() && none.is_empty() && list.change.is_empty() && list.items.remove.is_empty()
    {
        return Ok(Applied {
            bytes: loaded.bytes.to_vec(),
            report: report(loaded, super::Lines::default()),
            outcome,
            retired: Vec::new(),
            unchanged: true,
        });
    }

    let original = Source::of(&loaded.bytes).map_err(|e| whole(e.to_string()))?;
    let k = loaded.binding.part;
    let before = &original.read.parts[k];
    let prov = &original.read.provenance.parts[k];
    let located = crate::characteristics::located(before, prov, &original.doc);

    // The items to take out: the removed requirements, and the changed rows' requirements.
    let mut failures = Vec::new();
    // Each changed row: its faces, how many location dimensions of its own it replaces, and
    // the requirements it shares with other features' rows.
    let mut own_locations: BTreeMap<String, (BTreeSet<usize>, usize, Vec<String>)> =
        BTreeMap::new();
    let mut remove: BTreeSet<(String, usize)> = BTreeSet::new();
    for n in &list.items.remove {
        match located.iter().find(|l| l.number == Some(n.0)) {
            Some(l) => {
                remove.insert((l.at.0.to_string(), l.at.1));
            }
            None => failures.push(entry_failure(
                format!("items.remove[{}]", Json::String(n.to_string())),
                format!("{n} is not a requirement this writer can remove"),
            )),
        }
    }
    for id in list.change.keys() {
        let entry = format!("change[{}]", Json::String(id.clone()));
        let Some(row) = table.row(id) else { continue };
        if let Some(key) = id.strip_prefix("part.") {
            match setting_items(before, key) {
                Ok(items) if !items.is_empty() => remove.extend(items),
                Ok(_) => failures.push(entry_failure(
                    entry,
                    format!("{id} is not stated in a form this writer can replace"),
                )),
                Err(why) => failures.push(entry_failure(entry, why)),
            }
            continue;
        }
        // A tolerance's value is the frame alone: its basic dimensions stand. A requirement
        // another feature's row lists too (a shared location) stands for that feature.
        let frame_only = edits::answered_by_tolerance(&row.kind)
            && row
                .requirements
                .iter()
                .any(|r| r.kind == "geometric_tolerance");
        let same = same_requirement;
        let shared = |req: &crate::feature_table::Requirement| {
            table
                .rows
                .iter()
                .any(|r| r.feature != row.feature && r.requirements.iter().any(|q| same(q, req)))
        };
        // Changed to none, the frame goes with the basic dimensions only it used (decision 1).
        let to_none = list
            .change
            .get(id)
            .is_some_and(|c| crate::feature_table::is_none_value(&c.to));
        let shared_texts: Vec<&str> = row
            .requirements
            .iter()
            .filter(|r| shared(r))
            .map(|r| r.text.as_str())
            .collect();
        if !row.requirements.is_empty() && row.requirements.iter().all(|r| shared(r)) {
            failures.push(entry_failure(
                entry.clone(),
                format!(
                    "{id} is stated only by requirements other features' rows state too ({}); \
                     changing it would add a second requirement beside them. Remove them by \
                     number, or change them in your CAD tool",
                    shared_texts.join("; ")
                ),
            ));
            continue;
        }
        own_locations.insert(
            id.clone(),
            (
                row.faces.iter().copied().collect::<BTreeSet<usize>>(),
                row.requirements
                    .iter()
                    .filter(|r| {
                        r.r#type.starts_with("Location_") && !shared(r) && !(frame_only && !to_none)
                    })
                    .count(),
                shared_texts
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>(),
            ),
        );
        for req in &row.requirements {
            let frame_stands = frame_only
                && req.kind != "geometric_tolerance"
                && !(to_none && req.text.ends_with(" basic"));
            if frame_stands || shared(req) {
                continue;
            }
            match locate(req, &located, &remove) {
                Some(at) => {
                    remove.insert(at);
                }
                None => failures.push(entry_failure(
                    entry.clone(),
                    format!(
                        "its requirement {} is not one this writer can replace yet; remove it \
                         by number, or change it in your CAD tool",
                        req.text
                    ),
                )),
            }
        }
    }
    if !failures.is_empty() {
        return Err(failures);
    }
    let removed_numbers: BTreeSet<u64> = located
        .iter()
        .filter(|l| remove.contains(&(l.at.0.to_string(), l.at.1)))
        .filter_map(|l| l.number)
        .collect();

    // Take them out, with Specify's bookkeeping (replaced below) and each removed item's own
    // features unless something kept uses them; every other item is protected.
    let all = requirements(before);
    let bookkeeping: BTreeSet<(String, usize)> = (0..before.attributes.len())
        .map(|i| ("attributes".to_string(), i))
        .filter(|at| !all.contains(at))
        .collect();
    let mut give_way = bookkeeping.clone();
    give_way.extend(remove.iter().cloned());
    let mut features = BTreeSet::new();
    for (c, i) in &remove {
        features.extend(features_of(before, (c, *i)));
    }
    give_way.extend(features.iter().map(|&f| ("features".to_string(), f)));
    // What belongs to a removed item goes with it: the notes and attribute sets on it (a
    // dimension's plan, a thread's manufacturing data on its feature, unless kept).
    let owned = |on: &Option<haecceity::pmi::NoteOwner>| {
        use haecceity::pmi::NoteOwner;
        match on {
            Some(NoteOwner::Dimension(d)) => remove.contains(&("dimensions".to_string(), d.0)),
            Some(NoteOwner::Tolerance(t)) => remove.contains(&("tolerances".to_string(), t.0)),
            Some(NoteOwner::Feature(f)) => features.contains(&f.0),
            _ => false,
        }
    };
    for (i, set) in before.attributes.iter().enumerate() {
        if owned(&set.on) {
            give_way.insert(("attributes".to_string(), i));
        }
    }
    for (i, note) in before.notes.iter().enumerate() {
        if owned(&note.on) {
            give_way.insert(("notes".to_string(), i));
        }
    }
    let keep: BTreeSet<(String, usize)> = all
        .iter()
        .filter(|at| !give_way.contains(*at))
        .cloned()
        .collect();
    let cleaned = crate::provenance::without_owned_except(
        &original.doc,
        &original.read,
        &[k],
        &BTreeMap::from([(k, keep.clone())]),
        &BTreeMap::from([(k, give_way)]),
    )
    .map_err(|why| whole(format!("the requirements named cannot be taken out: {why}")))?;
    let source = Source::of(&cleaned).map_err(|e| whole(e.to_string()))?;
    // Every requirement named was taken out, and nothing else.
    let counts = |p: &PartPmi| -> BTreeMap<&'static str, usize> {
        BTreeMap::from([
            ("datums", p.datums.len()),
            ("dimensions", p.dimensions.len()),
            ("tolerances", p.tolerances.len()),
            ("general", p.general.len()),
            ("threads", p.threads.len()),
            ("knurls", p.knurls.len()),
            ("surface_textures", p.surface_textures.len()),
            ("material", usize::from(p.material.is_some())),
        ])
    };
    let mut expected = counts(before);
    for (c, _) in &remove {
        if let Some(n) = expected.get_mut(c.as_str()) {
            *n -= 1;
        }
    }
    if counts(&source.read.parts[k]) != expected {
        let names: Vec<String> = list
            .items
            .remove
            .iter()
            .map(|n| n.to_string())
            .chain(list.change.keys().cloned())
            .collect();
        return Err(whole(format!(
            "{} cannot be taken out on its own: a requirement the file keeps depends on it (a \
             tolerance on a dimension, a datum a frame cites); remove that one too",
            names.join(", ")
        )));
    }

    // Plan what is written, on the file without what was taken out: only the questions the
    // list names, with the file's items fixed.
    let loaded_clean = part::File::of_bytes(&loaded.path, cleaned.clone())
        .and_then(|f| f.load(Some(k)))
        .map_err(|e| whole(e.to_string()))?;
    let stored: Map<String, Json> = analysis["resume"]["answers"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let choices: Map<String, Json> = stored
        .iter()
        .filter(|(id, _)| {
            !crate::feature_table::is_requirement(id)
                && !id.starts_with('_')
                && !id.starts_with("bom.")
        })
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    let mut lines = super::Lines::default();
    let mut pmi = if given.is_empty() {
        PartPmi::default()
    } else {
        let mut analysis_clean =
            crate::api::analyse(&loaded_clean).map_err(|e| whole(e.to_string()))?;
        // The questions are those of the part with its PMI set aside but for its datums, as
        // the feature table reads them (a stated hole is not asked otherwise); what the plan
        // states again is not written twice.
        if let Some(existing) = analysis_clean
            .get_mut("existing")
            .and_then(Json::as_array_mut)
        {
            existing.retain(|item| item["kind"] == "datum");
        }
        // The list was validated against the table: what it names is asked here, whatever
        // the table says (a changed row is still stated by what stands of it).
        if let Some(object) = analysis_clean.as_object_mut() {
            object.remove("table");
        }
        let mut answers = choices.clone();
        answers.extend(given.iter().map(|(id, v)| (id.clone(), v.clone())));
        let mut intent =
            crate::rules::preview(&analysis_clean, &answers).map_err(|e| whole(e.to_string()))?;
        let addressed = |item: &Map<String, Json>| {
            item.get("question")
                .and_then(Json::as_str)
                .is_some_and(|q| given.contains_key(q))
        };
        intent.requirements.retain(addressed);
        intent
            .datums
            .retain(|d| d.get("existing") == Some(&Json::Bool(true)) || addressed(d));
        intent
            .part
            .retain(|key, _| given.contains_key(&format!("part.{key}")));
        // The general tolerance is its class alone: the planner's dimensions under it are for
        // the other features, which the list does not name. A hole the list leaves to the
        // general tolerance (`general`) keeps its own size under it.
        // (Matched by faces: the planner's rows name no feature.)
        let general_holes: Vec<(&String, BTreeSet<usize>)> = given
            .iter()
            .filter(|(id, v)| v.as_str() == Some("general") && id.starts_with("hole.function:"))
            .filter_map(|(id, _)| Some((id, table.row(id)?.faces.iter().copied().collect())))
            .collect();
        let hole_of = |r: &Map<String, Json>| -> Option<String> {
            if r.get("kind").and_then(Json::as_str) != Some("size") {
                return None;
            }
            let faces: BTreeSet<usize> = r
                .get("faces")?
                .as_array()?
                .iter()
                .filter_map(Json::as_u64)
                .map(|f| f as usize)
                .collect();
            general_holes
                .iter()
                .find(|(_, on)| !faces.is_empty() && faces.is_subset(on))
                .map(|(id, _)| (*id).clone())
        };
        intent.general.retain(|r| hole_of(r).is_some());
        for r in &mut intent.general {
            if let Some(id) = hole_of(r) {
                r.insert("question".into(), Json::from(id));
            }
        }
        for item in intent
            .requirements
            .iter_mut()
            .chain(intent.datums.iter_mut())
        {
            item.shift_remove("default");
        }
        let failing: Vec<Failure> = given
            .keys()
            .filter(|id| {
                let written = intent
                    .requirements
                    .iter()
                    .chain(&intent.general)
                    .any(|r| r["question"] == **id)
                    || intent
                        .datums
                        .iter()
                        .any(|d| d.get("question") == Some(&Json::from(id.as_str())))
                    || id
                        .strip_prefix("part.")
                        .is_some_and(|key| intent.part.contains_key(key));
                !written
            })
            .map(|id| {
                let section = if list.change.contains_key(id) {
                    "change"
                } else if list.fill.contains_key(id) {
                    "fill"
                } else {
                    "accept_suggestions"
                };
                // Usually a frame that cites datums the file does not state yet.
                let datums: Vec<&str> = table
                    .rows
                    .iter()
                    .filter(|r| {
                        r.state != State::Stated
                            && r.kind
                                .strip_prefix("datum.")
                                .is_some_and(|l| !l.contains('.'))
                            && !given.contains_key(&r.id)
                    })
                    .map(|r| r.id.as_str())
                    .collect();
                let general = given[id] == "general"
                    && table
                        .row("part.general_tolerance")
                        .is_some_and(|r| r.state != State::Stated)
                    && !given.contains_key("part.general_tolerance");
                let why = if general {
                    ": a hole left to the general tolerance needs one; fill \
                     part.general_tolerance in the same list"
                        .to_string()
                } else if datums.is_empty() {
                    String::new()
                } else {
                    format!(
                        ": the datums it would cite are not in the file; fill {} in the same \
                         list",
                        datums.join(", ")
                    )
                };
                entry_failure(
                    format!("{section}[{}]", Json::String(id.clone())),
                    format!(
                        "{id} with {} writes no requirement on this part as it is{why}",
                        given[id]
                    ),
                )
            })
            .collect();
        if !failing.is_empty() {
            return Err(failing);
        }
        let clean_before = &source.read.parts[k];
        // A basic location the file states already, as part of a larger group (one
        // dimension to a pattern and this hole), is not stated again for one of its holes.
        let mut held = Vec::new();
        intent.requirements.retain(|r| {
            let covered = covered_by_file(r, clean_before);
            if covered {
                held.push(format!(
                    "location of {} from faces {}: the file already states it; not written again",
                    r.get("feature").and_then(Json::as_str).unwrap_or("faces"),
                    r.get("reference").map(Json::to_string).unwrap_or_default()
                ));
            }
            !covered
        });
        let mode = if source.has_pmi(k) {
            Mode::Add
        } else {
            Mode::Replace
        };
        let (mut pmi, planned) = part_pmi(&loaded_clean, &intent, clean_before, mode, &[])
            .map_err(|e| whole(e.to_string()))?;
        lines = planned;
        // A change replaces its row's own items: it never adds a location dimension beside
        // one the row shares with another feature (another tool's group dimension, say).
        if mode == Mode::Add {
            let mut extra = Vec::new();
            for (id, (faces, own, shared)) in &own_locations {
                let planned = pmi
                    .dimensions
                    .iter()
                    .filter(|d| {
                        let DimensionKind::Location { from, to, .. } = &d.kind else {
                            return false;
                        };
                        [from, to].iter().any(|f| {
                            crate::existing::faces_of(&pmi, &[**f])
                                .iter()
                                .any(|x| faces.contains(x))
                        })
                    })
                    .count();
                if planned > *own && !shared.is_empty() {
                    extra.push(entry_failure(
                        format!("change[{}]", Json::String(id.clone())),
                        format!(
                            "the new value would add a location beside one {id} shares with \
                             another feature ({}); remove that one by number first, or change \
                             it in your CAD tool",
                            shared.join("; ")
                        ),
                    ));
                }
            }
            if !extra.is_empty() {
                return Err(extra);
            }
        }
        // Only what the list names is planned, and it was validated against the table; an
        // item the plan makes that measures exactly what the file states (a planner's own
        // dimension) is not written twice.
        if mode == Mode::Add {
            for what in drop_stated(&mut pmi, clean_before, &BTreeSet::new()) {
                lines.not_written.push(format!(
                    "{what}: the file already states it; not written again"
                ));
            }
        }
        lines.not_written.extend(held);
        pmi
    };
    let mode = if source.has_pmi(k) {
        Mode::Add
    } else {
        Mode::Replace
    };

    // The stored state: decided none, and the choices that are not requirements.
    let mut saved = choices;
    for id in &table.decided_none {
        if !given.contains_key(id) {
            saved.insert(id.clone(), none_answer(&table, id));
        }
    }
    for (id, value) in &none {
        saved.insert(id.clone(), value.clone());
    }
    if !saved.is_empty() {
        let payload = crate::resume::payload(loaded, &saved).map_err(|e| whole(e.to_string()))?;
        pmi.attributes.push(crate::resume::attribute_set(&payload));
    }
    crate::characteristics::allocate(
        loaded,
        &mut pmi,
        before,
        prov,
        &original.doc,
        None,
        if mode == Mode::Replace {
            crate::characteristics::Allocation::Replace
        } else {
            crate::characteristics::Allocation::Edit(&removed_numbers)
        },
    )
    .map_err(whole)?;
    // With nothing to number, the file's record is written back as it was.
    let record = |set: &haecceity::pmi::AttributeSet| set.name == crate::characteristics::PROPERTY;
    if !pmi.attributes.iter().any(record) {
        pmi.attributes
            .extend(before.attributes.iter().filter(|s| record(s)).cloned());
    }
    let retired =
        crate::characteristics::retired(before, prov, &original.doc, &pmi).map_err(whole)?;
    if !retired.is_empty() {
        lines.warnings.push(format!(
            "retired characteristic numbers: {}",
            retired
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for n in &list.items.remove {
        lines.written.push(format!("removed {n}"));
    }
    for (id, value) in &none {
        lines.written.push(format!(
            "{id}: decided {}",
            crate::choices::short_repr(value, 6)
        ));
    }

    let items = vec![(haecceity::pmi::PartId(k), pmi)];
    let (back, _) = write_by_modes(&source, &items, &[mode]).map_err(|e| whole(e.to_string()))?;
    verify(&source, &items, &[mode], &back).map_err(|e| whole(e.to_string()))?;

    // ADR P11: every item not named keeps its bytes, whoever wrote it.
    let after = haecceity::p21::Document::parse(back.clone()).map_err(|e| whole(e.to_string()))?;
    let ids = |c: &str, i: usize| -> &[u64] {
        match c {
            "features" => &prov.features[i],
            "datums" => &prov.datums[i],
            "datum_targets" => &prov.datum_targets[i],
            "dimensions" => &prov.dimensions[i],
            "tolerances" => &prov.tolerances[i],
            "general" => &prov.general[i],
            "threads" => &prov.threads[i],
            "knurls" => &prov.knurls[i],
            "notes" => &prov.notes[i],
            "surface_textures" => &prov.surface_textures[i],
            "attributes" => &prov.attributes[i],
            "material" => &prov.material,
            _ => &[],
        }
    };
    let touched: Vec<u64> = keep
        .iter()
        .flat_map(|(c, i)| ids(c, *i).iter().copied())
        .filter(|&id| kept_bytes(&original.doc, id) != kept_bytes(&after, id))
        .collect();
    if !touched.is_empty() {
        return Err(whole(format!(
            "the edit would change items the list does not name (#{}); nothing was written",
            touched
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(", #")
        )));
    }
    Ok(Applied {
        bytes: back,
        report: report(loaded, lines),
        outcome,
        retired,
        unchanged: false,
    })
}

fn report(loaded: &LoadedPart, lines: super::Lines) -> WriteReport {
    WriteReport {
        output: loaded.path.clone(),
        written: lines.written,
        not_written: lines.not_written,
        warnings: lines.warnings,
        geometry_carrier_exemptions: Vec::new(),
        presentation_warnings: Vec::new(),
        preserved_normalizations: Vec::new(),
        retained_shared: Vec::new(),
        saved_view_findings: Vec::new(),
    }
}
