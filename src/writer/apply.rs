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
//!
//! **Assemblies** (Paul, 2026-11-21). A list applies to one part of a file of several parts,
//! or of a part placed more than once, chosen as `--part` and `open_part` choose it
//! ([`apply_bytes`]'s `part`; a [`LoadedPart`] of that part). The part is its definition: one
//! placed twice is edited once, and every placement shows it. Every other part, placement and
//! instance, their PMI and stored answers, and any assembly-level item keep their bytes
//! (decision D3); characteristic numbers are the part's own.

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
/// Every value the list would write or store, checked as `analyse` checks answers when it
/// reads the file back: the questions of the part with its PMI set aside but for its datums
/// (every feature-and-kind asked), given that one answer. A value they refuse ("Choose one
/// of …", "'none' is not an answer") fails its entry.
/// The part's analysis with its PMI set aside but for its datums, as the feature table asks its
/// questions (every feature-and-kind asked).
fn bare(analysis: &Json) -> Json {
    let mut bare = analysis.clone();
    if let Some(object) = bare.as_object_mut() {
        object.remove("table");
        if let Some(existing) = object.get_mut("existing").and_then(Json::as_array_mut) {
            existing.retain(|item| item["kind"] == "datum");
        }
        // As the feature table asks them: the file's settings set aside too, so a setting
        // the list changes is asked (its answer read) as it is once the setting has gone.
        object.insert("part".into(), Json::Object(Map::new()));
        object.remove("dimensioning_stated");
        object.remove("process");
    }
    bare
}

fn answers_refused(list: &EditList, table: &FeatureTable, analysis: &Json) -> Vec<Failure> {
    let bare = bare(analysis);
    let choices: Map<String, Json> = analysis["resume"]["answers"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(id, _)| !crate::feature_table::is_requirement(id) && !id.starts_with('_'))
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    // As the writer stores it: a value meaning none is stored as the row's none.
    let stored = |id: &str, value: &Json| {
        if crate::feature_table::is_none_value(value) {
            none_answer(table, id)
        } else {
            value.clone()
        }
    };
    let mut entries: Vec<(String, &str, Json)> = Vec::new();
    for (id, value) in &list.fill {
        entries.push((
            format!("fill[{}]", Json::from(id.as_str())),
            id,
            stored(id, value),
        ));
    }
    for (id, change) in &list.change {
        entries.push((
            format!("change[{}]", Json::from(id.as_str())),
            id,
            stored(id, &change.to),
        ));
    }
    for id in &list.none {
        entries.push((
            format!("none[{}]", Json::from(id.as_str())),
            id,
            none_answer(table, id),
        ));
    }
    let choices_asked = choice_questions(table, analysis);
    for id in &list.accept_suggestions {
        let suggestion = table
            .row(id)
            .and_then(|r| r.suggestion.clone())
            .or_else(|| choices_asked.get(id).cloned().flatten());
        if let Some(s) = suggestion {
            entries.push((
                format!("accept_suggestions[{}]", Json::from(id.as_str())),
                id,
                stored(id, &s),
            ));
        }
    }
    entries
        .into_iter()
        .filter_map(|(entry, id, value)| {
            let mut answers = choices.clone();
            answers.insert(id.to_string(), value.clone());
            let why = crate::rules::questions(&bare, &answers).err()?;
            Some(entry_failure(
                entry,
                format!(
                    "{} is not an answer to {id}: {why}",
                    crate::choices::short_repr(&value, 6)
                ),
            ))
        })
        .collect()
}

/// The choices the part asks that are not requirements and have no feature-table row (how holes
/// are located, `part.locating`): an edit list may fill one (validated against its options) or
/// decide it none where none is an option; it is stored beside the PMI, as it leaves none. By
/// id, with the question's default (`accept_suggestions`).
fn choice_questions(table: &FeatureTable, analysis: &Json) -> BTreeMap<String, Option<Json>> {
    let stored: Map<String, Json> = analysis["resume"]["answers"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(id, _)| is_choice(table, id))
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    crate::rules::questions(&bare(analysis), &stored)
        .unwrap_or_default()
        .into_iter()
        .filter(|q| is_choice(table, &q.id))
        .map(|q| (q.id, q.default))
        .collect()
}

/// Whether `id` is an added requirement (`extra.<name>`): a fill of its spec adds it.
fn is_extra(id: &str) -> bool {
    id.strip_prefix("extra.")
        .is_some_and(|name| !name.is_empty())
}

/// Whether `id` is a choice that is not a requirement and has no feature-table row.
fn is_choice(table: &FeatureTable, id: &str) -> bool {
    !crate::feature_table::is_requirement(id)
        && !id.starts_with('_')
        && !id.starts_with("bom.")
        && table.row(id).is_none()
}

/// The file's state as an edit list sees it: the feature table; the choice questions
/// ([`choice_questions`]), each a gap that a fill, a none or its suggestion may answer, or, once
/// stored, a decision a clear takes back; and added requirements (`extra.<name>`), which a fill
/// adds, as a 0.2 answer did.
struct WithChoices<'a> {
    table: &'a FeatureTable,
    choices: BTreeMap<String, Option<Json>>,
    stored: Map<String, Json>,
    /// Questions the part asks only given the list's other answers (a datum's runout once the
    /// datum is answered), with no row of their own: gaps, with their suggestion.
    dependent: BTreeMap<String, Option<Json>>,
}

impl edits::FileState for WithChoices<'_> {
    fn kind(&self, id: &str) -> Option<edits::KindState> {
        if is_extra(id) {
            return Some(edits::KindState::Gap { suggestion: None });
        }
        self.table.kind(id).or_else(|| {
            if self.stored.contains_key(id) && is_choice(self.table, id) {
                return Some(edits::KindState::DecidedNone);
            }
            self.choices
                .get(id)
                .or_else(|| self.dependent.get(id))
                .map(|default| edits::KindState::Gap {
                    suggestion: default.clone().filter(|d| !d.is_null()),
                })
        })
    }
    fn requirement(&self, number: u64) -> Option<edits::Requirement> {
        self.table.requirement(number)
    }
    fn file_item(&self, id: &str) -> bool {
        self.table.file_item(id)
    }
}

/// Each added requirement the list fills (`extra.<name>`), planned on its own, against what the
/// file states: one the file does not state is a gap (written); one it states with the same
/// value changes nothing (`unchanged`); one it states with another value is refused, as a
/// differing fill is, unless the list also removes that requirement by number.
fn extra_states(
    loaded: &LoadedPart,
    list: &EditList,
    analysis: &Json,
) -> Result<(Vec<Failure>, Vec<String>), String> {
    let extras: Vec<(&String, &Json)> = list.fill.iter().filter(|(id, _)| is_extra(id)).collect();
    if extras.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let source = Source::of(&loaded.bytes).map_err(|e| e.to_string())?;
    let k = loaded.binding.part;
    let before = &source.read.parts[k];
    let prov = &source.read.provenance.parts[k];
    let removed: BTreeSet<u64> = list.items.remove.iter().map(|n| n.0).collect();
    let choices: Map<String, Json> = analysis["resume"]["answers"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(id, _)| !crate::feature_table::is_requirement(id) && !id.starts_with('_'))
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    let mut failures = Vec::new();
    let mut unchanged = Vec::new();
    for (id, value) in extras {
        let entry = format!("fill[{}]", Json::from(id.as_str()));
        let states = planned_states(loaded, analysis, &choices, before, prov, &source, id, value);
        if states.is_empty() {
            continue;
        }
        let differing: Vec<String> = states
            .iter()
            .filter_map(|(what, state)| match state {
                Some((false, numbers)) if !numbers.iter().any(|n| removed.contains(n)) => {
                    let named: Vec<String> = numbers.iter().map(|n| format!("#{n}")).collect();
                    Some(format!("{what} ({})", named.join(", ")))
                }
                _ => None,
            })
            .collect();
        if !differing.is_empty() {
            failures.push(entry_failure(
                entry,
                format!(
                    "the file already states {} with another value; to replace it, remove it \
                     by number in the same list (items.remove)",
                    differing.join("; ")
                ),
            ));
        } else if states
            .iter()
            .all(|(_, state)| matches!(state, Some((true, _))))
        {
            unchanged.push(entry);
        }
    }
    Ok((failures, unchanged))
}

/// The requirements `id` answered `value` plans on its own (with the file's stored choices), each
/// against what the file states ([`crate::characteristics::against`]); empty when it plans none.
#[allow(clippy::too_many_arguments)]
fn planned_states(
    loaded: &LoadedPart,
    analysis: &Json,
    choices: &Map<String, Json>,
    before: &PartPmi,
    prov: &haecceity::pmi::PartProvenance,
    source: &Source,
    id: &str,
    value: &Json,
) -> Vec<crate::characteristics::Against> {
    let mut answers = choices.clone();
    answers.insert(id.to_string(), value.clone());
    // A value the rules refuse is said by `answers_refused`.
    // Asked as the feature table asks it: the file's PMI set aside but for its datums.
    let Ok(mut intent) = crate::rules::preview(&bare(analysis), &answers) else {
        return Vec::new();
    };
    intent
        .requirements
        .retain(|r| r.get("question").and_then(Json::as_str) == Some(id));
    intent
        .datums
        .retain(|d| d.get("existing") == Some(&Json::Bool(true)));
    intent.part.clear();
    let Ok((planned, _)) = part_pmi(loaded, &intent, before, Mode::Add, &[]) else {
        return Vec::new();
    };
    crate::characteristics::against(&planned, before, prov, &source.doc)
}

/// A fill on a stated feature-and-kind whose value is written otherwise than the table shows
/// it (`tapped:M2` for the file's "internal thread"): planned on its own, it changes nothing
/// when every requirement it plans is one the file states with the same value. Such fills
/// are taken out of the list and named as unchanged.
fn same_as_stated(
    loaded: &LoadedPart,
    list: &mut EditList,
    table: &FeatureTable,
    analysis: &Json,
) -> Vec<String> {
    let stated: Vec<(String, Json)> = list
        .fill
        .iter()
        .filter(|(id, value)| match edits::FileState::kind(table, id) {
            Some(edits::KindState::Stated { value: shown }) => !edits::same_value(&shown, value),
            _ => false,
        })
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect();
    if stated.is_empty() {
        return Vec::new();
    }
    let Ok(source) = Source::of(&loaded.bytes) else {
        return Vec::new();
    };
    let k = loaded.binding.part;
    let (before, prov) = (&source.read.parts[k], &source.read.provenance.parts[k]);
    let choices: Map<String, Json> = analysis["resume"]["answers"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(id, _)| !crate::feature_table::is_requirement(id) && !id.starts_with('_'))
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    let mut out = Vec::new();
    for (id, value) in stated {
        let states = planned_states(
            loaded, analysis, &choices, before, prov, &source, &id, &value,
        );
        if !states.is_empty() && states.iter().all(|(_, s)| matches!(s, Some((true, _)))) {
            list.fill.remove(&id);
            out.push(format!("fill[{}]", Json::from(id.as_str())));
        }
    }
    out
}

/// The questions the part asks given the list's answers (and the file's stored choices) that
/// have no feature-table row and are not choices: those asked only once another is answered.
fn dependent_questions(
    list: &EditList,
    table: &FeatureTable,
    analysis: &Json,
) -> BTreeMap<String, Option<Json>> {
    let mut answers: Map<String, Json> = analysis["resume"]["answers"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(id, _)| !crate::feature_table::is_requirement(id) && !id.starts_with('_'))
        .map(|(id, v)| (id.clone(), v.clone()))
        .collect();
    answers.extend(list.fill.iter().map(|(id, v)| (id.clone(), v.clone())));
    answers.extend(list.change.iter().map(|(id, c)| (id.clone(), c.to.clone())));
    for id in &list.none {
        answers.insert(id.clone(), none_answer(table, id));
    }
    crate::rules::questions(&bare(analysis), &answers)
        .unwrap_or_default()
        .into_iter()
        .filter(|q| table.row(&q.id).is_none() && !is_extra(&q.id) && !is_choice(table, &q.id))
        .map(|q| (q.id, q.default))
        .collect()
}

fn validated(
    loaded: &LoadedPart,
    list: &EditList,
    table: &FeatureTable,
    analysis: &Json,
) -> (EditList, Outcome) {
    let mut list = edits::normalise(list, table);
    let same = same_as_stated(loaded, &mut list, table, analysis);
    let state = WithChoices {
        table,
        dependent: dependent_questions(&list, table, analysis),
        choices: choice_questions(table, analysis),
        stored: analysis["resume"]["answers"]
            .as_object()
            .cloned()
            .unwrap_or_default(),
    };
    let mut outcome = edits::validate(&list, &state);
    outcome.entries += same.len();
    outcome.unchanged.extend(same);
    // A 0.2 answers file carries its face evidence: one saved on another revision of the part
    // is refused, as 0.2's write refused it ("stale answers").
    if let Some(answers) = &list.answers_file
        && answers.contains_key(crate::answer_file::META)
        && let Err(why) = crate::answer_file::check(loaded, answers)
    {
        outcome.failures.push(entry_failure(
            format!("{}.evidence", crate::answer_file::META),
            why,
        ));
    }
    if outcome.failures.is_empty() {
        outcome.failures = answers_refused(&list, table, analysis);
    }
    if outcome.failures.is_empty() {
        match extra_states(loaded, &list, analysis) {
            Ok((failures, unchanged)) => {
                outcome.failures.extend(failures);
                outcome.unchanged.extend(unchanged);
            }
            Err(why) => outcome
                .failures
                .push(entry_failure("(the whole list)".into(), why)),
        }
    }
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
    let (analysis, table) = match table_of(loaded) {
        Ok(found) => found,
        Err(why) => return Outcome::malformed(whole(why)),
    };
    let (pmi_list, bom) = bom_edits(&list, &analysis);
    let mut outcome = validated(loaded, &pmi_list, &table, &analysis).1;
    if let Err(failures) = bom {
        outcome.failures.extend(failures);
    }
    outcome.entries = list.len();
    outcome.applicable = outcome.failures.is_empty();
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
pub(crate) fn locate(
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

/// The list with each change given field by field (`"to": {"value": 3.4}`) as the answer that
/// plans the same requirement (a tolerance's value; a fitted size's class), and the new
/// nominals, by row, that [`keep_untouched`] then gives the planned size. Field by field, only
/// `value` changes, on a row stated by one requirement that is not text only.
fn field_changes(
    list: &EditList,
    table: &FeatureTable,
    analysis: &Json,
) -> Result<(EditList, BTreeMap<String, f64>), Vec<Failure>> {
    let mut out = list.clone();
    let mut nominals = BTreeMap::new();
    let mut failures = Vec::new();
    for (id, change) in &mut out.change {
        let Json::Object(fields) = &change.to else {
            continue;
        };
        let entry = format!("change[{}]", Json::String(id.clone()));
        let fail = |why: String| entry_failure(entry.clone(), why);
        let value = match (fields.len(), fields.get("value").and_then(Json::as_f64)) {
            (1, Some(v)) if v > 0.0 => v,
            _ => {
                failures.push(fail(format!(
                    "a change field by field gives {{\"value\": <number>}} (the nominal of a \
                     size, or a tolerance's value), not {}",
                    change.to
                )));
                continue;
            }
        };
        let Some(row) = table.row(id).filter(|r| r.state == State::Stated) else {
            continue; // validation says why
        };
        let [req] = row.requirements.as_slice() else {
            failures.push(fail(format!(
                "{id} is stated by {} requirements; change it as a whole",
                row.requirements.len()
            )));
            continue;
        };
        if req.text_only {
            failures.push(fail(format!(
                "{id} states {}, which has parts only its text holds: it cannot be changed field \
                 by field. Change it in your CAD tool, or remove it (items.remove) and fill the \
                 gap",
                req.text
            )));
            continue;
        }
        let fit = analysis["existing"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|item| {
                let number = item["number"].as_u64().map(|n| format!("#{n}"));
                item["kind"] == "dimension"
                    && if req.number.is_some() {
                        number == req.number
                    } else {
                        item["type"] == req.r#type.as_str() && item["text"] == req.text.as_str()
                    }
            })
            .and_then(|item| item["fit"].as_str());
        let answer = if edits::answered_by_tolerance(&row.kind) && req.kind == "geometric_tolerance"
        {
            Json::from(value.to_string())
        } else if req.kind == "dimension" && req.r#type.starts_with("Size_") && fit.is_some() {
            let class = fit.unwrap_or_default();
            nominals.insert(id.clone(), value);
            match row.kind.as_str() {
                "hole.function" => Json::from(format!("fit:{class}")),
                _ => Json::from(class),
            }
        } else {
            failures.push(fail(format!(
                "{id} ({}) cannot be changed field by field: that is for a tolerance's value or \
                 a fitted size's nominal; give its new value as a whole",
                req.text
            )));
            continue;
        };
        change.to = answer;
    }
    if failures.is_empty() {
        Ok((out, nominals))
    } else {
        Err(failures)
    }
}

/// A change keeps what it does not touch: each planned tolerance replacing one of a changed
/// row's takes the original's zone and modifiers (⌖ ⌀0.3Ⓜ changed to 0.2 is ⌖ ⌀0.2Ⓜ); a planned
/// size replacing a fitted one keeps its class and takes the nominal given field by field.
fn keep_untouched(
    pmi: &mut PartPmi,
    before: &PartPmi,
    replaced: &BTreeMap<String, Vec<(String, usize)>>,
    nominals: &BTreeMap<String, f64>,
) -> Result<(), super::WriteError> {
    use haecceity::pmi::DimTolerance;
    let faces = |p: &PartPmi, f: haecceity::pmi::FeatureId| crate::existing::faces_of(p, &[f]);
    for (id, items) in replaced {
        for (collection, i) in items {
            match collection.as_str() {
                "tolerances" => {
                    let old = &before.tolerances[*i];
                    let ToleranceTarget::Feature(f) = old.target else {
                        continue;
                    };
                    let on = faces(before, f);
                    // A zone that refers to other features is text only, refused above.
                    let zone = old.zone.as_ref().filter(|z| {
                        z.projected.is_none()
                            && z.non_uniform.is_none()
                            && z.affected_plane.is_none()
                    });
                    let planned = pmi.tolerances.iter().position(|t| {
                        t.kind == old.kind
                            && matches!(t.target, ToleranceTarget::Feature(g) if faces(pmi, g) == on)
                    });
                    if let Some(j) = planned {
                        let new = &mut pmi.tolerances[j];
                        new.modifiers = old.modifiers.clone();
                        if let (Some(zone), Some(new_zone)) = (zone, new.zone.as_mut()) {
                            new_zone.form = zone.form.clone();
                        }
                    }
                }
                "dimensions" => {
                    let old = &before.dimensions[*i];
                    let DimensionKind::Size { feature, .. } = old.kind else {
                        continue;
                    };
                    let on = faces(before, feature);
                    let Some(j) = pmi.dimensions.iter().position(|d| {
                        matches!(d.kind, DimensionKind::Size { feature: g, .. } if faces(pmi, g) == on)
                    }) else {
                        continue;
                    };
                    let class = match &old.tolerance {
                        DimTolerance::Fit { class, .. } => Some(*class),
                        _ => None,
                    };
                    if let Some(value) = nominals.get(id) {
                        let new = &mut pmi.dimensions[j];
                        new.nominal = Some(super::mm(*value)?);
                        if let Some(class) = class {
                            // The class alone, as the file had it: no deviations in its place.
                            new.tolerance = DimTolerance::Fit {
                                class,
                                limits: None,
                            };
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// The entities of item `i` of collection `c` of a part, as its provenance records them.
fn item_ids<'a>(prov: &'a haecceity::pmi::PartProvenance, c: &str, i: usize) -> &'a [u64] {
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
}

/// The file's assembly-level entities: every record that refers, directly or through others,
/// to a product definition that is not one of the parts `defs` (an assembly's): its structure,
/// its placements, and any PMI or property on the assembly itself. A part edit never changes
/// them. Empty for a file of one part used once.
fn assembly_level(
    doc: &haecceity::p21::Document,
    defs: &[haecceity::step::PartDefinition],
) -> BTreeSet<u64> {
    use haecceity::p21::RawEntity;
    let parts: BTreeSet<u64> = defs.iter().map(|d| d.product_definition).collect();
    let mut todo: Vec<u64> = doc
        .ids()
        .filter(|id| !parts.contains(id))
        .filter(|&id| {
            matches!(doc.get(id), Some(RawEntity::Simple { name, .. })
                if name == "PRODUCT_DEFINITION"
                    || name == "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS")
        })
        .collect();
    let mut seen: BTreeSet<u64> = todo.iter().copied().collect();
    while let Some(id) = todo.pop() {
        for &by in doc.referrers(id) {
            if seen.insert(by) {
                todo.push(by);
            }
        }
    }
    seen
}

fn kept_bytes(doc: &haecceity::p21::Document, id: u64) -> Option<&[u8]> {
    doc.span(id).map(|s| &doc.bytes()[s])
}

/// The intent of the answers `given` on the part `analysis` reads (its PMI set aside but for
/// its datums, as the feature table reads it), with the stored `choices`: only what `given`
/// names is planned (the general tolerance's dimensions only for a hole left to it).
pub(crate) fn planned_intent(
    analysis: &Json,
    table: &FeatureTable,
    given: &BTreeMap<String, Json>,
    choices: &Map<String, Json>,
    whole_general: bool,
) -> Result<crate::rules::Intent, String> {
    let mut analysis_clean = analysis.clone();
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
    let mut intent = crate::rules::preview(&analysis_clean, &answers).map_err(|e| e.to_string())?;
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
    // the other features, which the list does not name. A hole or a diameter the list
    // leaves to the general tolerance (`general`) keeps its own size under it.
    // (Matched by faces: the planner's rows name no feature.)
    let general_holes: Vec<(&String, BTreeSet<usize>)> = given
        .iter()
        .filter(|(id, v)| {
            v.as_str() == Some("general")
                && (id.starts_with("hole.function:") || id.starts_with("diameter.fit:"))
        })
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
    // D1: the dimensions no question asks about (plane levels) are written once no question is
    // open: a list that answers every open gap (accepting every suggestion) writes the whole
    // general set, as `write --accept-defaults` does; otherwise only what it names. Not for
    // `carry` (`whole_general` false), which writes only what the older revision states.
    let nothing_open = whole_general
        && table
            .rows
            .iter()
            .all(|r| !r.open || given.contains_key(&r.id));
    if !nothing_open {
        intent.general.retain(|r| hole_of(r).is_some());
    }
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
    Ok(intent)
}

/// Applies `list` to `loaded`'s part, all or nothing: the whole list, or nothing and every
/// failing entry with its reason.
pub fn apply(loaded: &LoadedPart, list: &EditList) -> Result<Applied, Vec<Failure>> {
    apply_with(loaded, list, &[])
}

/// [`apply`], also writing `extra`: attribute sets an application carries in the file (the
/// editor's project and the record of the original, `carried::attributes`). Each replaces the
/// file's attribute sets of its name, so a payload is never carried twice; every other item is
/// left as it is. With extras the file is always written.
pub fn apply_with(
    loaded: &LoadedPart,
    list: &EditList,
    extra: &[haecceity::pmi::AttributeSet],
) -> Result<Applied, Vec<Failure>> {
    applied_with(loaded, list, None, extra, false)
}

/// [`apply`], with `numbers` an older revision's characteristic-number history carried onto
/// this part (`characteristics::carry_revision`, for `carry`): a requirement the history
/// names keeps its number, and its retired numbers stay retired. A file with a number history
/// of its own keeps that instead.
pub(crate) fn apply_numbered(
    loaded: &LoadedPart,
    list: &EditList,
    numbers: Option<&Map<String, Json>>,
) -> Result<Applied, Vec<Failure>> {
    applied_with(loaded, list, numbers, &[], true)
}

/// The `bom.*` entries of `list` (a part of an assembly's BOM fields: `fill` with a value, or
/// `accept_suggestions` taking the BOM's own suggestion) as the intent that appends them, and the
/// list without them. Every part of the file is numbered when the BOM is written, as `write`'s
/// BOM acceptance does. Refused for a file with no assembly structure (`bom_no_assembly`), for a
/// value the BOM does not take, and for a suggestion the BOM does not have.
fn bom_edits(
    list: &EditList,
    analysis: &Json,
) -> (EditList, Result<Option<crate::rules::Intent>, Vec<Failure>>) {
    let is_bom = |id: &str| id.starts_with("bom.");
    let mut rest = list.clone();
    rest.fill.retain(|id, _| !is_bom(id));
    rest.accept_suggestions.retain(|id| !is_bom(id));
    let fills: Vec<(&String, &Json)> = list.fill.iter().filter(|(id, _)| is_bom(id)).collect();
    let accepted: Vec<&String> = list
        .accept_suggestions
        .iter()
        .filter(|id| is_bom(id))
        .collect();
    if fills.is_empty() && accepted.is_empty() {
        return (rest, Ok(None));
    }
    let entry = |section: &str, id: &str| format!("{section}[{}]", Json::from(id));
    let mut failures = Vec::new();
    if !analysis.get("bom").is_some_and(Json::is_object) {
        for (id, _) in &fills {
            failures.push(entry_failure(
                entry("fill", id),
                format!(
                    "{}: the file has no assembly structure, so there is no BOM to write",
                    crate::bom::NO_ASSEMBLY
                ),
            ));
        }
        for id in &accepted {
            failures.push(entry_failure(
                entry("accept_suggestions", id),
                format!(
                    "{}: the file has no assembly structure, so there is no BOM to write",
                    crate::bom::NO_ASSEMBLY
                ),
            ));
        }
        return (rest, Err(failures));
    }
    let mut fields = Map::new();
    let mut answers = Map::new();
    for (id, value) in fills {
        let key = &id["bom.".len()..];
        match crate::bom::validate(key, value) {
            Ok(()) => {
                fields.insert(key.to_string(), value.clone());
                answers.insert(id.clone(), value.clone());
            }
            Err(why) => failures.push(entry_failure(entry("fill", id), why)),
        }
    }
    if !accepted.is_empty() {
        let mut proposed = crate::rules::Intent::new(analysis["binding"].clone());
        match crate::bom::accept_suggestions(analysis, &answers, &mut proposed) {
            Ok(()) => {
                let suggested = proposed.part.get("bom").and_then(Json::as_object);
                for id in accepted {
                    let key = &id["bom.".len()..];
                    match suggested.and_then(|s| s.get(key)) {
                        Some(value) => {
                            fields.insert(key.to_string(), value.clone());
                        }
                        None => failures.push(entry_failure(
                            entry("accept_suggestions", id),
                            format!("the BOM has no suggestion for {id}; give a value with fill"),
                        )),
                    }
                }
            }
            Err(why) => failures.push(entry_failure("(the whole list)".into(), why)),
        }
    }
    if !failures.is_empty() {
        return (rest, Err(failures));
    }
    let mut intent = crate::rules::Intent::new(analysis["binding"].clone());
    intent.part.insert("bom".into(), Json::Object(fields));
    (rest, Ok(Some(intent)))
}

/// A change of the part's general tolerance to a value (not none) goes the one route that
/// replaces a stated general tolerance (#228: `writer::write_bytes_changing_general_tolerance`,
/// `Replace::GeneralTolerance`): the file's table, classes and general-tolerance note give way
/// to the new one, every other entity byte for byte. The rest of the list is applied first, as
/// any list is; the whole list is validated first, so it is still all or nothing.
fn applied_with(
    loaded: &LoadedPart,
    list: &EditList,
    numbers: Option<&Map<String, Json>>,
    extra: &[haecceity::pmi::AttributeSet],
    carrying: bool,
) -> Result<Applied, Vec<Failure>> {
    const GENERAL: &str = "part.general_tolerance";
    let Some(change) = list
        .change
        .get(GENERAL)
        .filter(|c| !crate::feature_table::is_none_value(&c.to))
    else {
        return applied_parts(loaded, list, numbers, extra, carrying);
    };
    let (analysis, table) = table_of(loaded).map_err(whole)?;
    let (pmi_list, bom) = bom_edits(list, &analysis);
    bom?;
    let (_, outcome) = validated(loaded, &pmi_list, &table, &analysis);
    if !outcome.failures.is_empty() {
        return Err(outcome.failures);
    }
    let mut rest = list.clone();
    rest.change.remove(GENERAL);
    let first = applied_parts(loaded, &rest, numbers, extra, carrying)?;
    let entry = || format!("change[{}]", Json::from(GENERAL));
    let part = loaded.binding.part;
    let now = part::File::of_bytes(&loaded.path, first.bytes.clone())
        .and_then(|f| f.load((f.parts().len() > 1).then_some(part)))
        .map_err(|e| whole(e.to_string()))?;
    let mut intent = crate::rules::Intent::new(
        serde_json::to_value(&now.binding).map_err(|e| whole(e.to_string()))?,
    );
    intent
        .part
        .insert("general_tolerance".into(), change.to.clone());
    // What the file stores beside its PMI (decided none, choices) is stored again as it is.
    let stored = crate::resume::answers_for(&now)
        .map_err(|e| whole(e.to_string()))?
        .filter(|a| !a.is_empty());
    let (bytes, report) =
        super::write_bytes_changing_general_tolerance(&now, &intent, stored.as_ref())
            .map_err(|e| vec![entry_failure(entry(), e.to_string())])?;
    let mut applied = first;
    applied.bytes = bytes;
    applied.report.written.extend(report.written);
    applied.report.not_written.extend(report.not_written);
    applied.report.warnings.extend(report.warnings);
    applied.unchanged = false;
    Ok(applied)
}

fn applied_parts(
    loaded: &LoadedPart,
    list: &EditList,
    numbers: Option<&Map<String, Json>>,
    extra: &[haecceity::pmi::AttributeSet],
    carrying: bool,
) -> Result<Applied, Vec<Failure>> {
    if !list
        .fill
        .keys()
        .chain(&list.accept_suggestions)
        .any(|id| id.starts_with("bom."))
    {
        return applied_pmi(loaded, list, numbers, extra, carrying);
    }
    let analysis = crate::api::analyse(loaded).map_err(|e| whole(e.to_string()))?;
    let (list, bom) = bom_edits(list, &analysis);
    let intent = bom?.expect("the list has BOM entries");
    let mut applied = applied_pmi(loaded, &list, numbers, extra, carrying)?;
    // The BOM is appended after the PMI edit: every entity already there keeps its bytes.
    applied.bytes = crate::bom::append(applied.bytes, &[(loaded, &intent, None)])
        .map_err(|why| whole(format!("the BOM cannot be written: {why}")))?;
    if let Some(fields) = intent.part.get("bom").and_then(Json::as_object) {
        for (key, value) in fields {
            applied.report.written.push(format!("BOM {key} {value}"));
        }
    }
    applied.unchanged = false;
    Ok(applied)
}

fn applied_pmi(
    loaded: &LoadedPart,
    list: &EditList,
    numbers: Option<&Map<String, Json>>,
    extra: &[haecceity::pmi::AttributeSet],
    carrying: bool,
) -> Result<Applied, Vec<Failure>> {
    let (analysis, table) = table_of(loaded).map_err(whole)?;
    let (list, nominals) = field_changes(list, &table, &analysis)?;
    let (list, outcome) = validated(loaded, &list, &table, &analysis);
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
    let choices_asked = choice_questions(&table, &analysis);
    for id in &list.accept_suggestions {
        let suggestion = table
            .row(id)
            .and_then(|r| r.suggestion.clone())
            .or_else(|| choices_asked.get(id).cloned().flatten());
        if let Some(s) = suggestion {
            given.insert(id.clone(), s);
        }
    }
    // A choice with no table row (how holes are located) writes nothing of its own: it is
    // stored, and the planner reads it for the rest of the list. One equal to what the file
    // stores changes nothing.
    let stored_choices = analysis["resume"]["answers"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let mut chosen: BTreeMap<String, Json> = BTreeMap::new();
    for id in list.none.iter().filter(|id| is_choice(&table, id)) {
        chosen.insert(id.clone(), none_answer(&table, id));
    }
    given.retain(|id, value| {
        if !is_choice(&table, id) {
            return true;
        }
        chosen.insert(id.clone(), value.clone());
        false
    });
    chosen.retain(|id, value| stored_choices.get(id) != Some(&*value));
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
    for id in list.none.iter().filter(|id| !is_choice(&table, id)) {
        if !skip("none", id) {
            none.insert(id.clone(), none_answer(&table, id));
        }
    }
    if extra.is_empty()
        && list.clear.is_empty()
        && given.is_empty()
        && none.is_empty()
        && chosen.is_empty()
        && list.change.is_empty()
        && list.items.remove.is_empty()
        && list.items.remove_ids.is_empty()
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
    // Rows changed to none that a requirement shared with another feature still states.
    let mut still_stated: Vec<String> = Vec::new();
    let mut remove: BTreeSet<(String, usize)> = BTreeSet::new();
    // Each changed row's items taken out, so its new items keep what the change does not touch.
    let mut changed_items: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();
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
    // Removals by id: an item of the file's other PMI by its entity, or a stated row whole
    // (its requirements that no other feature's row states too).
    for id in &list.items.remove_ids {
        let entry = edits::removal_entry(id);
        if let Some(entity) = id
            .strip_prefix(edits::FILE_PMI)
            .and_then(|e| e.strip_prefix('#'))
            .and_then(|e| e.parse::<u64>().ok())
        {
            match located.iter().find(|l| l.entity == entity) {
                Some(l) => {
                    remove.insert((l.at.0.to_string(), l.at.1));
                }
                None => failures.push(entry_failure(
                    entry,
                    format!("{id} is not an item this writer can remove"),
                )),
            }
            continue;
        }
        let Some(row) = table.row(id) else { continue };
        if let Some(key) = id.strip_prefix("part.") {
            match setting_items(before, key) {
                Ok(items) if !items.is_empty() => remove.extend(items),
                Ok(_) => failures.push(entry_failure(
                    entry,
                    format!("{id} is not stated in a form this writer can remove"),
                )),
                Err(why) => failures.push(entry_failure(entry, why)),
            }
            continue;
        }
        let shared = |req: &crate::feature_table::Requirement| {
            table.rows.iter().any(|r| {
                r.feature != row.feature && r.requirements.iter().any(|q| same_requirement(q, req))
            })
        };
        let own: Vec<_> = row.requirements.iter().filter(|r| !shared(r)).collect();
        if own.is_empty() {
            failures.push(entry_failure(
                entry,
                format!(
                    "{id} is stated only by requirements other features' rows state too; \
                     remove them by number"
                ),
            ));
            continue;
        }
        for req in own {
            match locate(req, &located, &remove) {
                Some(at) => {
                    remove.insert(at);
                }
                None => failures.push(entry_failure(
                    entry.clone(),
                    format!(
                        "its requirement {} is not one this writer can remove; remove it by \
                         number, or in your CAD tool",
                        req.text
                    ),
                )),
            }
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
        if !row.requirements.is_empty() && row.requirements.iter().all(&shared) {
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
        if to_none && !shared_texts.is_empty() {
            still_stated.push(format!(
                "{id}: changed to none, but {} it shares with another feature still states it; \
                 if that is removed later, {id} reads decided none",
                shared_texts.join("; ")
            ));
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
                // A basic dimension: `50 (basic)` in the file's notation, `… basic` read
                // without one.
                && !(to_none && (req.text.ends_with("(basic)") || req.text.ends_with(" basic")));
            if frame_stands || shared(req) {
                continue;
            }
            // A change replans the requirement: what only its text holds would be lost.
            if req.text_only {
                failures.push(entry_failure(
                    entry.clone(),
                    format!(
                        "{id} states {}, which has parts only its text holds; a change cannot \
                         carry them over, so it is refused rather than dropping them. Change it \
                         in your CAD tool, or remove it (items.remove) and fill the gap",
                        req.text
                    ),
                ));
                continue;
            }
            match locate(req, &located, &remove) {
                Some(at) => {
                    changed_items
                        .entry(id.clone())
                        .or_default()
                        .push(at.clone());
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
    // The attribute sets the extras replace.
    let replaced: BTreeSet<&str> = extra.iter().map(|set| set.name.as_str()).collect();
    give_way.extend(
        before
            .attributes
            .iter()
            .enumerate()
            .filter(|(_, set)| replaced.contains(set.name.as_str()))
            .map(|(i, _)| ("attributes".to_string(), i)),
    );
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
            .chain(list.items.remove_ids.iter().cloned())
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
                && !list.clear.contains(*id)
        })
        .map(|(id, v)| (id.clone(), v.clone()))
        .chain(chosen.iter().map(|(id, v)| (id.clone(), v.clone())))
        .collect();
    let mut lines = super::Lines::default();
    let mut pmi = if given.is_empty() {
        PartPmi::default()
    } else {
        let analysis_clean =
            crate::api::analyse(&loaded_clean).map_err(|e| whole(e.to_string()))?;
        let mut intent =
            planned_intent(&analysis_clean, &table, &given, &choices, !carrying).map_err(whole)?;
        let failing: Vec<Failure> = given
            .keys()
            .filter(|id| {
                let written = intent
                    .requirements
                    .iter()
                    .chain(&intent.general)
                    .any(|r| r.get("question").and_then(Json::as_str) == Some(id.as_str()))
                    || intent
                        .datums
                        .iter()
                        .any(|d| d.get("question") == Some(&Json::from(id.as_str())))
                    || id
                        .strip_prefix("part.")
                        .is_some_and(|key| intent.part.contains_key(key))
                    // A hole's full-thread depth is written in its thread, which the hole's
                    // function plans.
                    || id.strip_prefix("hole.full_thread:").is_some_and(|feature| {
                        let function = format!("hole.function:{feature}");
                        intent.requirements.iter().any(|r| {
                            r.get("question").and_then(Json::as_str) == Some(function.as_str())
                                && r.get("kind").and_then(Json::as_str) == Some("thread")
                        })
                    });
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
        keep_untouched(&mut pmi, before, &changed_items, &nominals)
            .map_err(|e| whole(e.to_string()))?;
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
        if !given.contains_key(id) && !list.clear.contains(id) {
            saved.insert(id.clone(), none_answer(&table, id));
        }
    }
    // A none stored for a row that only requirements shared with other features state (a
    // change to none that left their dimension) is kept: once that goes, the row reads
    // decided none.
    let raw = crate::resume::answers_for(loaded)
        .map_err(|e| whole(e.to_string()))?
        .unwrap_or_default();
    for (id, value) in &raw {
        let shared_only = table.row(id).is_some_and(|row| {
            row.state == State::Stated
                && !row.requirements.is_empty()
                && row.requirements.iter().all(|req| {
                    table.rows.iter().any(|r| {
                        r.feature != row.feature
                            && r.requirements.iter().any(|q| same_requirement(q, req))
                    })
                })
        });
        if shared_only
            && crate::feature_table::leaves_nothing(value)
            && !given.contains_key(id)
            && !list.clear.contains(id)
        {
            saved.insert(id.clone(), value.clone());
        }
    }
    for (id, value) in &none {
        saved.insert(id.clone(), value.clone());
    }
    if !saved.is_empty() {
        let payload = crate::resume::payload(loaded, &saved).map_err(|e| whole(e.to_string()))?;
        pmi.attributes.push(crate::resume::attribute_set(&payload));
    }
    pmi.attributes.extend_from_slice(extra);
    crate::characteristics::allocate(
        loaded,
        &mut pmi,
        before,
        prov,
        &original.doc,
        numbers,
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
    lines.warnings.extend(still_stated);
    for n in &list.items.remove {
        lines.written.push(format!("removed {n}"));
    }
    for id in &list.items.remove_ids {
        lines.written.push(format!("removed {id}"));
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

    // ADR P11: every item not named keeps its bytes, whoever wrote it: this part's kept items,
    // every item of the file's other parts, and every assembly-level record (decision D3: in
    // an assembly only the part edited is written; a part placed several times is one
    // definition, edited once).
    let after = haecceity::p21::Document::parse(back.clone()).map_err(|e| whole(e.to_string()))?;
    let others = original
        .read
        .provenance
        .parts
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != k)
        .flat_map(|(j, p)| {
            requirements(&original.read.parts[j])
                .into_iter()
                .chain(
                    (0..original.read.parts[j].attributes.len())
                        .map(|i| ("attributes".to_string(), i)),
                )
                .flat_map(move |(c, i)| item_ids(p, &c, i).to_vec())
        });
    let touched: Vec<u64> = keep
        .iter()
        .flat_map(|(c, i)| item_ids(prov, c, *i).iter().copied())
        .chain(others)
        .chain(assembly_level(&original.doc, &original.defs))
        .filter(|&id| kept_bytes(&original.doc, id) != kept_bytes(&after, id))
        .collect::<BTreeSet<u64>>()
        .into_iter()
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

/// A file's requirements as `carry` compares a plan with them: the part's PMI, each
/// requirement located (with its number), and what each states, value included.
pub(crate) struct Stated {
    before: PartPmi,
    located: Vec<crate::characteristics::Located>,
    said: Vec<crate::characteristics::Statement>,
}

impl Stated {
    pub(crate) fn of(loaded: &LoadedPart) -> Result<Stated, String> {
        let source = Source::of(&loaded.bytes).map_err(|e| e.to_string())?;
        let k = loaded.binding.part;
        let before = source.read.parts[k].clone();
        let located =
            crate::characteristics::located(&before, &source.read.provenance.parts[k], &source.doc);
        let said = crate::characteristics::statements(&before);
        Ok(Stated {
            before,
            located,
            said,
        })
    }
}

impl Stated {
    /// What the row `row` of this file's table states: each of its requirements, value
    /// included, or `None` when one cannot be found among the file's requirements (a setting).
    pub(crate) fn of_row(
        &self,
        row: &crate::feature_table::Row,
    ) -> Option<Vec<&crate::characteristics::Statement>> {
        let mut taken = BTreeSet::new();
        let mut out = Vec::new();
        for req in &row.requirements {
            let at = locate(req, &self.located, &taken)?;
            let i = self
                .located
                .iter()
                .position(|l| (l.at.0.to_string(), l.at.1) == at)?;
            taken.insert(at);
            out.push(&self.said[i]);
        }
        Some(out)
    }
}

/// Whether answering the row `row` of `loaded`'s table with `value` states exactly what the
/// file states for it: the requirements the answer plans on this part, values included, are
/// the row's own. For a kind answered by a tolerance's magnitude, the tolerance frames alone
/// are compared (its basic dimensions are the planner's). `analysis` and `table` are the
/// part's, `choices` its stored answers that are not requirements.
pub(crate) fn restates(
    loaded: &LoadedPart,
    stated: &Stated,
    (analysis, table, choices): (&Json, &FeatureTable, &Map<String, Json>),
    row: &crate::feature_table::Row,
    value: &Json,
) -> bool {
    use crate::characteristics::Statement;
    let given = BTreeMap::from([(row.id.clone(), value.clone())]);
    let Ok(intent) = planned_intent(analysis, table, &given, choices, false) else {
        return false;
    };
    let Ok((planned, _)) = super::part_pmi(loaded, &intent, &stated.before, Mode::Add, &[]) else {
        return false;
    };
    let frames_only = edits::answered_by_tolerance(&row.kind);
    let counted =
        |s: &Statement| s.kind != "datum" && (!frames_only || s.kind == "geometric_tolerance");
    let planned = crate::characteristics::statements(&planned);
    let planned: Vec<&Statement> = planned.iter().filter(|s| counted(s)).collect();
    let Some(own) = stated.of_row(row) else {
        return false;
    };
    let own: Vec<&Statement> = own.into_iter().filter(|s| counted(s)).collect();
    // A hole left to the general tolerance: its size has no tolerance of its own. (Planned
    // on the file that states it, the general tolerance's size is not planned again.)
    if value == "general"
        && row.kind == "hole.function"
        && !own.is_empty()
        && own.iter().all(|s| s.general_size())
    {
        return true;
    }
    if planned.is_empty() || planned.len() != own.len() {
        return false;
    }
    let mut used = vec![false; planned.len()];
    own.iter().all(|o| {
        let found = (0..planned.len()).find(|&j| !used[j] && planned[j] == *o);
        if let Some(j) = found {
            used[j] = true;
        }
        found.is_some()
    })
}

/// `bytes` with a material added to each part `materials` names (part index, material), for
/// `carry` onto an assembly's parts that state none: every entity of the file keeps its bytes,
/// or nothing is written.
pub(crate) fn add_materials(
    bytes: &[u8],
    materials: &[(usize, String)],
) -> Result<Vec<u8>, String> {
    if materials.is_empty() {
        return Ok(bytes.to_vec());
    }
    let source = Source::of(bytes).map_err(|e| e.to_string())?;
    let items: Vec<(haecceity::pmi::PartId, PartPmi)> = materials
        .iter()
        .map(|(k, material)| {
            (
                haecceity::pmi::PartId(*k),
                PartPmi {
                    material: Some(haecceity::pmi::Material {
                        id: material.clone(),
                        name: None,
                        density: None,
                    }),
                    ..PartPmi::default()
                },
            )
        })
        .collect();
    let modes: Vec<Mode> = materials
        .iter()
        // Anything the part has (stored answers too) is kept: added to, never replaced.
        .map(|(k, _)| {
            if source.read.parts[*k] != PartPmi::default() {
                Mode::Add
            } else {
                Mode::Replace
            }
        })
        .collect();
    let (back, _) = write_by_modes(&source, &items, &modes).map_err(|e| e.to_string())?;
    verify(&source, &items, &modes, &back).map_err(|e| e.to_string())?;
    let after = haecceity::p21::Document::parse(back.clone()).map_err(|e| e.to_string())?;
    if let Some(id) = source
        .doc
        .ids()
        .find(|&id| kept_bytes(&source.doc, id) != kept_bytes(&after, id))
    {
        return Err(format!(
            "writing the material would change #{id}; nothing was written"
        ));
    }
    Ok(back)
}
