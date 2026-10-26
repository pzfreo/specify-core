//! Revisions: a part's answers carried from one revision of its STEP file to the next, and what
//! changed.
//!
//! [`crate::resume::rematch_answers`] resumes answers on the same geometry: with face evidence
//! that does not match, every answer on a feature or a datum is left unresolved. A revision is
//! different geometry by design, so here quiddity matches the two parts
//! (`quiddity::correspondence::correspond`): it aligns them and classifies each recognised
//! feature and face as carried, adapted, ambiguous, orphaned or new.
//!
//! An answer names its feature by family and defining faces (`hole.function:holes:12.13`) or,
//! for a datum, lists faces (`datum.A: [4]`). Each face is mapped through the correspondence;
//! the answer is then carried to the new part's feature of that family holding the mapped faces.
//! An answer is never guessed: when a face is gone (*orphaned*), matches several faces
//! (*ambiguous*), or no single feature holds the mapped faces, it is left unresolved with its
//! value and the reason, as resuming does. An answer carried to a feature that quiddity reports
//! changed (a resized hole keeping its fit) is carried and listed for a person to check.
//!
//! The report also lists the questions the new part opens that the old one did not (a new hole
//! needs its function), the problems review finds that it did not before, and the features that
//! changed; [`picture`] draws the new part coloured by what changed.

use std::collections::{BTreeMap, BTreeSet};

use quiddity::correspondence::{self as qc, Class, Correspondence};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::api::{self, Result};
use crate::features::Feature;
use crate::part::LoadedPart;

/// What one old face is in the new part.
#[derive(Clone, Debug, PartialEq)]
enum Bind {
    To(usize),
    Orphaned,
    Ambiguous(Vec<usize>),
}

#[derive(Clone, Debug, Serialize)]
pub struct Revised {
    /// `ok`: every answer carried and no new question or problem; `decide`: an answer is
    /// unresolved, a match is ambiguous or the new part asks something new; `problems`: review
    /// finds problems it did not on the old part.
    pub verdict: String,
    pub summary: String,
    /// The answers on the new part: carried, renamed where the feature's ID changed.
    pub answers: Map<String, Value>,
    /// Each answer whose ID changed, `[old, new]`.
    pub migrated: Vec<(String, String)>,
    pub unresolved: Vec<Unresolved>,
    /// Answers carried to a feature quiddity reports changed: still applied, to be checked.
    pub check: Vec<Check>,
    pub features: Vec<FeatureChange>,
    /// Questions open on the new part that were not open on the old one.
    pub questions: Vec<OpenQuestion>,
    pub problems: Problems,
    /// Each new face's class (`changed` or `new`; faces carried unchanged are left out), and the
    /// old faces that are gone or moved, for [`picture`].
    #[serde(skip)]
    pub faces: BTreeMap<usize, &'static str>,
    #[serde(skip)]
    pub gone: Vec<usize>,
    #[serde(skip)]
    pub correspondence: Option<Correspondence>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Unresolved {
    pub id: String,
    pub answer: Value,
    /// `orphaned`, `ambiguous`, `unmatched` (the faces carried, but no single feature of the
    /// family holds them) or `stale` (the old part had no such feature either).
    pub class: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Check {
    pub id: String,
    pub answer: Value,
    pub feature: String,
    pub changes: BTreeMap<String, qc::Change>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FeatureChange {
    /// `adapted`, `ambiguous`, `orphaned` or `new`.
    pub class: String,
    pub family: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub changes: BTreeMap<String, qc::Change>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OpenQuestion {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Problems {
    pub added: Vec<Value>,
    pub resolved: Vec<Value>,
}

/// Families that are facts about faces, not features a person answers for: they follow every
/// change to the faces around them.
const EVIDENCE: [&str; 2] = ["risers", "step_levels"];

/// The quiddity record of each feature: the one whose defining faces it shares most.
fn records(features: &[Feature], prints: &qc::Fingerprints) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for record in &prints.features {
        let best = features
            .iter()
            .filter(|f| !f.faces.is_empty())
            .map(|f| {
                (
                    f,
                    f.faces.iter().filter(|i| record.faces.contains(i)).count(),
                )
            })
            .filter(|(f, n)| *n > 0 && *n == f.faces.len().min(record.faces.len()))
            // The most faces shared, then the smallest feature: a hole, not its pattern.
            .max_by_key(|(f, n)| (*n, std::cmp::Reverse(f.faces.len())));
        if let Some((f, _)) = best {
            out.entry(record.id.clone()).or_insert_with(|| f.id.clone());
        }
    }
    out
}

/// The question and feature of an answer on one feature (`hole.function:holes:12.13`), as
/// resuming reads them.
fn feature_answer(id: &str) -> Option<(&str, &str, Vec<usize>)> {
    let (question, feature) = id.split_once(':')?;
    if !matches!(
        question,
        "hole.function"
            | "hole.full_thread"
            | "hole.frame"
            | "hole.position"
            | "hole.location"
            | "diameter.fit"
            | "diameter.runout"
    ) {
        return None;
    }
    let (family, support) = feature.split_once(':')?;
    let faces = support
        .split('.')
        .map(|s| s.parse().ok())
        .collect::<Option<Vec<usize>>>()?;
    Some((question, family, faces))
}

struct Matching<'a> {
    faces: BTreeMap<usize, Bind>,
    old_features: &'a [Feature],
    new_features: &'a [Feature],
}

impl Matching<'_> {
    /// The old faces in the new part, or the first that does not carry.
    fn faces(&self, old: &[usize]) -> std::result::Result<Vec<usize>, (usize, Bind)> {
        old.iter()
            .map(|i| match self.faces.get(i) {
                Some(Bind::To(n)) => Ok(*n),
                Some(other) => Err((*i, other.clone())),
                None => Err((*i, Bind::Orphaned)),
            })
            .collect()
    }

    /// The new feature of *family* holding *faces*, or every candidate.
    fn feature(&self, family: &str, faces: &[usize]) -> std::result::Result<&Feature, Vec<String>> {
        let found: Vec<&Feature> = self
            .new_features
            .iter()
            .filter(|f| {
                f.id.split_once(':').map(|(fam, _)| fam) == Some(family)
                    && faces.iter().all(|i| f.faces.contains(i))
            })
            .collect();
        match found.as_slice() {
            [one] => Ok(one),
            many => Err(many.iter().map(|f| f.id.clone()).collect()),
        }
    }

    /// *id*'s answer on the new part: its new ID, or why it does not carry.
    fn answer(
        &self,
        id: &str,
        answer: &Value,
    ) -> std::result::Result<(String, Value), Box<Unresolved>> {
        let unresolved = |class: &str, reason: String, candidates: Vec<String>| {
            Box::new(Unresolved {
                id: id.into(),
                answer: answer.clone(),
                class: class.into(),
                reason,
                candidates,
            })
        };
        let gone = |(face, bind): (usize, Bind)| match bind {
            Bind::Ambiguous(c) => unresolved(
                "ambiguous",
                format!("face {face} matches more than one face of the new part"),
                c.iter().map(|n| format!("face {n}")).collect(),
            ),
            _ => unresolved(
                "orphaned",
                format!("face {face} has no match in the new part"),
                Vec::new(),
            ),
        };
        if let Some((question, family, faces)) = feature_answer(id) {
            let feature = &id[question.len() + 1..];
            if !self.old_features.iter().any(|f| f.id == feature) {
                return Err(unresolved(
                    "stale",
                    format!("the old part has no {feature} either"),
                    Vec::new(),
                ));
            }
            let now = self.faces(&faces).map_err(gone)?;
            return match self.feature(family, &now) {
                Ok(f) => Ok((format!("{question}:{}", f.id), answer.clone())),
                Err(candidates) if candidates.is_empty() => Err(unresolved(
                    "unmatched",
                    format!("no {family} feature of the new part holds its faces"),
                    candidates,
                )),
                Err(candidates) => Err(unresolved(
                    "ambiguous",
                    format!("several {family} features of the new part hold its faces"),
                    candidates,
                )),
            };
        }
        if id.starts_with("datum.")
            && let Some(list) = answer.as_array()
            && list.iter().all(Value::is_u64)
        {
            let old: Vec<usize> = list
                .iter()
                .filter_map(Value::as_u64)
                .map(|i| i as usize)
                .collect();
            let now = self.faces(&old).map_err(gone)?;
            return Ok((
                id.into(),
                Value::Array(now.into_iter().map(Value::from).collect()),
            ));
        }
        Ok((id.into(), answer.clone()))
    }
}

/// Carry *answers* (and *automatic*, the IDs the default path answered) from *old* to *new*.
pub fn revise(old: &LoadedPart, new: &LoadedPart, answers: &Map<String, Value>) -> Result<Revised> {
    // One recognition of each part, for both its analysis and its fingerprints.
    let old_q = qc::recognise(&old.part);
    let new_q = qc::recognise(&new.part);
    let (old_analysis, new_analysis) = (
        api::analyse_recognised(old, &old_q)?,
        api::analyse_recognised(new, &new_q)?,
    );
    let features = |a: &Value| -> Result<Vec<Feature>> {
        serde_json::from_value(a["features"].clone())
            .map_err(|e| api::Error::Analysis(format!("the analysis's features: {e}")))
    };
    let (old_features, new_features) = (features(&old_analysis)?, features(&new_analysis)?);
    let corr =
        qc::correspond(&old_q.fingerprints, &new_q.fingerprints).map_err(api::Error::Analysis)?;
    let faces = corr
        .faces
        .iter()
        .map(|e| {
            let bind = match e.class {
                Class::Carried | Class::Adapted => e.new.map_or(Bind::Orphaned, Bind::To),
                Class::Ambiguous => Bind::Ambiguous(e.candidates.clone()),
                _ => Bind::Orphaned,
            };
            (e.old, bind)
        })
        .collect();
    let matching = Matching {
        faces,
        old_features: &old_features,
        new_features: &new_features,
    };

    // Features, by specify-core's IDs.
    let (old_records, new_records) = (
        records(&old_features, &old_q.fingerprints),
        records(&new_features, &new_q.fingerprints),
    );
    let family_of = |fs: &[Feature], id: &str| {
        fs.iter()
            .find(|f| f.id == id)
            .map(|f| f.family.clone())
            .unwrap_or_default()
    };
    let mut feature_changes = Vec::new();
    let mut changed: BTreeMap<String, BTreeMap<String, qc::Change>> = BTreeMap::new();
    let mut reached = BTreeSet::new();
    for e in &corr.features {
        let Some(old_id) = old_records.get(&e.old) else {
            continue;
        };
        if EVIDENCE.contains(&family_of(&old_features, old_id).as_str()) {
            continue;
        }
        let new_id = e.new.as_ref().and_then(|n| new_records.get(n)).cloned();
        if let Some(n) = &new_id {
            reached.insert(n.clone());
        }
        let class = match e.class {
            Class::Carried => continue,
            Class::Adapted if e.changes.keys().all(|k| k == "adjacency") => continue,
            Class::Adapted => "adapted",
            Class::Ambiguous => "ambiguous",
            _ => "orphaned",
        };
        if let Some(n) = &new_id {
            changed.insert(n.clone(), e.changes.clone());
        }
        let candidates: Vec<String> = e
            .candidates
            .iter()
            .filter_map(|c| new_records.get(c))
            .cloned()
            .collect();
        reached.extend(candidates.iter().cloned());
        feature_changes.push(FeatureChange {
            class: class.into(),
            family: family_of(&old_features, old_id),
            old: Some(old_id.clone()),
            new: new_id,
            candidates,
            changes: e.changes.clone(),
        });
    }
    for f in &new_features {
        if !f.faces.is_empty()
            && !EVIDENCE.contains(&f.family.as_str())
            && new_records.values().any(|v| v == &f.id)
            && !reached.contains(&f.id)
        {
            feature_changes.push(FeatureChange {
                class: "new".into(),
                family: f.family.clone(),
                old: None,
                new: Some(f.id.clone()),
                candidates: Vec::new(),
                changes: BTreeMap::new(),
            });
        }
    }

    // The answers.
    let mut carried = Map::new();
    let (mut migrated, mut unresolved, mut check) = (Vec::new(), Vec::new(), Vec::new());
    let results: Vec<_> = answers
        .iter()
        .map(|(id, answer)| (id, answer, matching.answer(id, answer)))
        .collect();
    let mut reached: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, _, r) in &results {
        if let Ok((now, _)) = r {
            *reached.entry(now.as_str()).or_default() += 1;
        }
    }
    for (id, answer, result) in &results {
        match result {
            // Two answers would carry to one: neither is chosen, as resuming does.
            Ok((now, _)) if reached[now.as_str()] > 1 => unresolved.push(Unresolved {
                id: (*id).clone(),
                answer: (*answer).clone(),
                class: "ambiguous".into(),
                reason: format!("several answers carry to {now}; review required"),
                candidates: vec![now.clone()],
            }),
            Ok((now, value)) => {
                let (id, now, value) = (*id, now.clone(), value.clone());
                if now != *id {
                    migrated.push((id.clone(), now.clone()));
                }
                if let Some(feature) = now.split_once(':').map(|(_, f)| f.to_string())
                    && let Some(changes) = changed.get(&feature)
                {
                    check.push(Check {
                        id: now.clone(),
                        answer: value.clone(),
                        feature,
                        changes: changes.clone(),
                    });
                }
                carried.insert(now, value);
            }
            Err(u) => unresolved.push((**u).clone()),
        }
    }
    // A datum's settings (`datum.A.flatness`) follow it: they stay open with an open datum.
    let open_datums: Vec<(String, String)> = unresolved
        .iter()
        .filter(|u| u.id.starts_with("datum.") && u.id.matches('.').count() == 1)
        .map(|u| (format!("{}.", u.id), u.class.clone()))
        .collect();
    let settings: Vec<(String, String)> = carried
        .keys()
        .filter_map(|id| {
            open_datums
                .iter()
                .find(|(datum, _)| id.starts_with(datum.as_str()))
                .map(|(_, class)| (id.clone(), class.clone()))
        })
        .collect();
    for (id, class) in settings {
        let answer = carried
            .remove(&id)
            .expect("listed from the carried answers");
        let datum = &id[..id.rfind('.').expect("a datum setting")];
        unresolved.push(Unresolved {
            reason: format!("its datum {datum} is unresolved"),
            id,
            answer,
            class,
            candidates: Vec::new(),
        });
    }

    // What the new part asks and finds that the old one did not.
    // An old ID in the new part; one that does not carry names nothing new, so it can't match a
    // new question or problem even where its old number is reused.
    let rename = |id: &str| {
        matching
            .answer(id, &Value::Null)
            .map(|(n, _)| n)
            .unwrap_or_else(|_| format!("\u{0}gone {id}"))
    };
    let open = |analysis: &Value, answers: &Map<String, Value>| -> Result<Vec<(String, String)>> {
        let qs = api::questions(analysis, answers)?;
        Ok(crate::rules::open_questions(&qs, answers)
            .iter()
            .map(|q| {
                (
                    q.id.clone(),
                    q.title
                        .as_ref()
                        .map_or_else(|| q.prompt.clone(), |t| format!("{t}: {}", q.prompt)),
                )
            })
            .collect())
    };
    let before: BTreeSet<String> = open(&old_analysis, answers)?
        .into_iter()
        .map(|(id, _)| rename(&id))
        .collect();
    let questions: Vec<OpenQuestion> = open(&new_analysis, &carried)?
        .into_iter()
        .filter(|(id, _)| !before.contains(id))
        .map(|(id, title)| OpenQuestion { id, title })
        .collect();
    let warnings = |analysis: &Value, answers: &Map<String, Value>| -> Result<Vec<Value>> {
        let review = api::review(analysis, answers)?;
        Ok(review["problems"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p["severity"] != "info")
            .collect())
    };
    // A problem is its code, its subject and its message; an old problem's subject (an answer
    // ID, which its message may quote) is read in the new part first.
    let key = |p: &Value, old: bool| {
        let text = |k: &str| p[k].as_str().unwrap_or_default().to_string();
        let (about, message) = (text("about"), text("message"));
        if old && !about.is_empty() {
            let now = rename(&about);
            let message = message.replace(&about, &now);
            (text("code"), now, message)
        } else {
            (text("code"), about, message)
        }
    };
    let (was, now) = (
        warnings(&old_analysis, answers)?,
        warnings(&new_analysis, &carried)?,
    );
    let (was_keys, now_keys): (BTreeSet<_>, BTreeSet<_>) = (
        was.iter().map(|p| key(p, true)).collect(),
        now.iter().map(|p| key(p, false)).collect(),
    );
    let problems = Problems {
        added: now
            .iter()
            .filter(|p| !was_keys.contains(&key(p, false)))
            .cloned()
            .collect(),
        resolved: was
            .iter()
            .filter(|p| !now_keys.contains(&key(p, true)))
            .cloned()
            .collect(),
    };

    // The picture's faces.
    let mut face_class: BTreeMap<usize, &'static str> = BTreeMap::new();
    for n in &corr.new_faces {
        face_class.insert(*n, "new");
    }
    let mut gone: Vec<usize> = corr
        .faces
        .iter()
        .filter(|e| e.class == Class::Orphaned)
        .map(|e| e.old)
        .collect();
    for c in &feature_changes {
        let faces_of = |fs: &[Feature], id: &Option<String>| -> Vec<usize> {
            id.as_ref()
                .and_then(|id| fs.iter().find(|f| &f.id == id))
                .map(|f| f.faces.clone())
                .unwrap_or_default()
        };
        match c.class.as_str() {
            "adapted" => {
                for i in faces_of(&new_features, &c.new) {
                    face_class.insert(i, "changed");
                }
                if c.changes.contains_key("position") {
                    gone.extend(faces_of(&old_features, &c.old));
                }
            }
            "ambiguous" => {
                for id in &c.candidates {
                    for i in faces_of(&new_features, &Some(id.clone())) {
                        face_class.insert(i, "ambiguous");
                    }
                }
            }
            "new" => {
                for i in faces_of(&new_features, &c.new) {
                    face_class.insert(i, "new");
                }
            }
            _ => gone.extend(faces_of(&old_features, &c.old)),
        }
    }
    gone.sort_unstable();
    gone.dedup();

    let ambiguous = feature_changes
        .iter()
        .filter(|f| f.class == "ambiguous")
        .count();
    let open_count = unresolved.len() + ambiguous + questions.len();
    let verdict = if !problems.added.is_empty() {
        "problems"
    } else if open_count > 0 {
        "decide"
    } else {
        "ok"
    };
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    let summary = match verdict {
        "ok" => format!(
            "Every answer carries to the new part ({}), and it asks nothing new.",
            plural(carried.len(), "answer", "answers")
        ),
        "decide" => {
            let mut parts = Vec::new();
            if !unresolved.is_empty() {
                parts.push(format!(
                    "{} not carried",
                    plural(unresolved.len(), "answer", "answers")
                ));
            }
            if ambiguous > 0 {
                parts.push(format!(
                    "{} that can't be told apart",
                    plural(ambiguous, "feature", "features")
                ));
            }
            if !questions.is_empty() {
                parts.push(format!(
                    "{} new to the part",
                    plural(questions.len(), "question", "questions")
                ));
            }
            format!("A decision is needed: {}.", parts.join(", "))
        }
        _ => format!(
            "Review finds {} on the new part it did not before.",
            plural(problems.added.len(), "problem", "problems")
        ),
    };
    Ok(Revised {
        verdict: verdict.into(),
        summary,
        answers: carried,
        migrated,
        unresolved,
        check,
        features: feature_changes,
        questions,
        problems,
        faces: face_class,
        gone,
        correspondence: Some(corr),
    })
}

impl Revised {
    /// The report as Markdown, for a pull request comment; *image* is the picture's link.
    pub fn markdown(&self, title: &str, image: Option<&str>) -> String {
        use std::fmt::Write;
        let mark = match self.verdict.as_str() {
            "ok" => "✅",
            "decide" => "⚠️",
            _ => "❌",
        };
        let mut s = format!("### {title}: Specify\n\n{mark} {}\n\n", self.summary);
        if let Some(image) = image {
            let _ = writeln!(
                s,
                "![The new part: changed in orange, new in green, can't tell in purple, gone or moved from in red]({image})\n"
            );
        }
        let fmt = |v: &Value| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let changes = |c: &BTreeMap<String, qc::Change>| {
            c.iter()
                .filter(|(k, _)| *k != "adjacency")
                .map(|(k, c)| format!("{k} {} → {}", fmt(&c.old), fmt(&c.new)))
                .collect::<Vec<_>>()
                .join("; ")
        };
        if !self.features.is_empty() {
            s += "**The part**\n\n";
            for f in &self.features {
                let who = f
                    .old
                    .as_ref()
                    .or(f.new.as_ref())
                    .cloned()
                    .unwrap_or_default();
                let _ = writeln!(
                    s,
                    "{}",
                    match f.class.as_str() {
                        "adapted" => format!("- {} `{who}`: {}", f.family, changes(&f.changes)),
                        "ambiguous" => format!(
                            "- {} `{who}`: can't tell which it is now ({})",
                            f.family,
                            f.candidates.join(", ")
                        ),
                        "orphaned" => format!("- {} `{who}` is gone", f.family),
                        _ => format!("- new {} `{who}`", f.family),
                    }
                );
            }
            s += "\n";
        }
        if !self.unresolved.is_empty() {
            s += "**Answers not carried**\n\n";
            for u in &self.unresolved {
                let _ = writeln!(
                    s,
                    "- `{}` = {} ({}): {}{}",
                    u.id,
                    fmt(&u.answer),
                    u.class,
                    u.reason,
                    if u.candidates.is_empty() {
                        String::new()
                    } else {
                        format!("; candidates {}", u.candidates.join(", "))
                    }
                );
            }
            s += "\n";
        }
        if !self.check.is_empty() {
            s += "**Answers carried to a changed feature: check them**\n\n";
            for c in &self.check {
                let _ = writeln!(
                    s,
                    "- `{}` = {}: {}",
                    c.id,
                    fmt(&c.answer),
                    changes(&c.changes)
                );
            }
            s += "\n";
        }
        if !self.questions.is_empty() {
            s += "**New questions**\n\n";
            for q in &self.questions {
                let _ = writeln!(s, "- {} (`{}`)", q.title, q.id);
            }
            s += "\n";
        }
        if !self.problems.added.is_empty() {
            s += "**New problems**\n\n";
            for p in &self.problems.added {
                let _ = writeln!(s, "- {}", p["message"].as_str().unwrap_or_default());
            }
            s += "\n";
        }
        if !self.migrated.is_empty() {
            let _ = writeln!(
                s,
                "{} carried to renumbered faces.\n",
                if self.migrated.len() == 1 {
                    "1 answer was".to_string()
                } else {
                    format!("{} answers were", self.migrated.len())
                }
            );
        }
        s
    }
}

/// The new part's picture: an isometric view, its faces coloured by what changed (changed
/// orange, new green, ambiguous purple, the rest grey) with the old part's gone or moved faces
/// over it in translucent red, as a PNG *width* pixels wide.
pub fn picture(old: &LoadedPart, new: &LoadedPart, revised: &Revised, width: u32) -> Vec<u8> {
    let colour = |class: Option<&&str>| -> [f64; 3] {
        match class.copied() {
            Some("changed") => [0.88, 0.48, 0.0],
            Some("new") => [0.04, 0.56, 0.24],
            Some("ambiguous") => [0.48, 0.25, 0.75],
            _ => [0.80, 0.81, 0.83],
        }
    };
    let mesh_new = crate::mesh::mesh(new);
    let mesh_old = crate::mesh::mesh(old);
    let triangles = |mesh: &Value,
                     keep: &dyn Fn(usize) -> Option<[f64; 3]>,
                     motion: Option<&qc::Alignment>|
     -> Vec<([[f64; 3]; 3], [f64; 3])> {
        let pos: Vec<f64> = mesh["positions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_f64)
            .collect();
        let idx: Vec<usize> = mesh["indices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .map(|i| i as usize)
            .collect();
        let place = |v: usize| -> [f64; 3] {
            let p = [pos[v * 3], pos[v * 3 + 1], pos[v * 3 + 2]];
            match motion {
                Some(a) if a.found => [0, 1, 2].map(|i| {
                    a.rotation[i][0] * p[0]
                        + a.rotation[i][1] * p[1]
                        + a.rotation[i][2] * p[2]
                        + a.translation[i]
                }),
                _ => p,
            }
        };
        let mut out = Vec::new();
        for (face, range) in mesh["faces"].as_array().into_iter().flatten().enumerate() {
            let (Some(start), Some(count)) = (range[0].as_u64(), range[1].as_u64()) else {
                continue;
            };
            let Some(c) = keep(face) else { continue };
            for t in start as usize..(start + count) as usize {
                out.push((
                    [
                        place(idx[t * 3]),
                        place(idx[t * 3 + 1]),
                        place(idx[t * 3 + 2]),
                    ],
                    c,
                ));
            }
        }
        out
    };
    let solid = triangles(&mesh_new, &|f| Some(colour(revised.faces.get(&f))), None);
    let gone: BTreeSet<usize> = revised.gone.iter().copied().collect();
    let ghost = triangles(
        &mesh_old,
        &|f| gone.contains(&f).then_some([0.84, 0.16, 0.16]),
        revised.correspondence.as_ref().map(|c| &c.alignment),
    );
    let edges: Vec<Vec<[f64; 3]>> = mesh_new["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            let p: Vec<f64> = e
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_f64)
                .collect();
            p.chunks(3)
                .filter(|c| c.len() == 3)
                .map(|c| [c[0], c[1], c[2]])
                .collect()
        })
        .collect();
    render(&solid, &ghost, &edges, width)
}

/// A z-buffered isometric rendering (looking along −(1, −1, 1), z up), 2× supersampled.
fn render(
    solid: &[([[f64; 3]; 3], [f64; 3])],
    ghost: &[([[f64; 3]; 3], [f64; 3])],
    edges: &[Vec<[f64; 3]>],
    width: u32,
) -> Vec<u8> {
    let norm = |v: [f64; 3]| {
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        v.map(|x| x / n)
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let toward = norm([1.0, -1.0, 1.0]);
    let right = norm(cross([0.0, 0.0, 1.0], toward));
    let up = cross(toward, right);
    let light = norm([0.4, -0.8, 1.0]);
    let all = solid
        .iter()
        .chain(ghost)
        .flat_map(|(t, _)| t.iter().copied());
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in all {
        let q = [dot(p, right), dot(p, up)];
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let w = width as usize * 2;
    // As wide as asked, and no taller than twice that: a long thin part standing up is drawn
    // smaller rather than in a towering buffer.
    let scale = ((w as f64 * 0.9) / (hi[0] - lo[0]).max(1e-9))
        .min((w as f64 * 2.0 * 0.9) / (hi[1] - lo[1]).max(1e-9));
    let h = (((hi[1] - lo[1]) * scale) / 0.9).ceil().max(1.0) as usize + 2;
    let (ox, oy) = (
        (w as f64 - (hi[0] - lo[0]) * scale) / 2.0 - lo[0] * scale,
        h as f64 * 0.05 + hi[1] * scale,
    );
    let screen = |p: [f64; 3]| -> [f64; 3] {
        [
            ox + dot(p, right) * scale,
            oy - dot(p, up) * scale,
            dot(p, toward),
        ]
    };
    let mut rgb = vec![[1.0f64; 3]; w * h];
    let mut depth = vec![f64::NEG_INFINITY; w * h];
    let fill = |tri: &[[f64; 3]; 3],
                c: [f64; 3],
                alpha: f64,
                test: bool,
                rgb: &mut Vec<[f64; 3]>,
                depth: &mut Vec<f64>| {
        let n = norm(cross(
            [
                tri[1][0] - tri[0][0],
                tri[1][1] - tri[0][1],
                tri[1][2] - tri[0][2],
            ],
            [
                tri[2][0] - tri[0][0],
                tri[2][1] - tri[0][1],
                tri[2][2] - tri[0][2],
            ],
        ));
        let n = if dot(n, toward) < 0.0 {
            n.map(|x| -x)
        } else {
            n
        };
        let shade = 0.45 + 0.55 * dot(n, light).max(0.0);
        let c = c.map(|x| (x * shade).min(1.0));
        let s = tri.map(screen);
        let (x0, x1) = (
            s.iter()
                .map(|p| p[0])
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(0.0) as usize,
            (s.iter()
                .map(|p| p[0])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil() as usize)
                .min(w - 1),
        );
        let (y0, y1) = (
            s.iter()
                .map(|p| p[1])
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(0.0) as usize,
            (s.iter()
                .map(|p| p[1])
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil() as usize)
                .min(h - 1),
        );
        let area =
            (s[1][0] - s[0][0]) * (s[2][1] - s[0][1]) - (s[2][0] - s[0][0]) * (s[1][1] - s[0][1]);
        if area.abs() < 1e-12 {
            return;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let b0 = ((s[1][0] - px) * (s[2][1] - py) - (s[2][0] - px) * (s[1][1] - py)) / area;
                let b1 = ((s[2][0] - px) * (s[0][1] - py) - (s[0][0] - px) * (s[2][1] - py)) / area;
                let b2 = 1.0 - b0 - b1;
                if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                    continue;
                }
                let z = b0 * s[0][2] + b1 * s[1][2] + b2 * s[2][2];
                let i = y * w + x;
                if test {
                    if z <= depth[i] {
                        continue;
                    }
                    depth[i] = z;
                }
                rgb[i] = [0, 1, 2].map(|k| rgb[i][k] * (1.0 - alpha) + c[k] * alpha);
            }
        }
    };
    for (t, c) in solid {
        fill(t, *c, 1.0, true, &mut rgb, &mut depth);
    }
    // The edges, where they are not behind a face (a little depth allowance for the face they
    // bound).
    let allowance = (hi[0] - lo[0]).max(hi[1] - lo[1]) * 0.004;
    for e in edges {
        for seg in e.windows(2) {
            let (a, b) = (screen(seg[0]), screen(seg[1]));
            let steps = ((b[0] - a[0]).hypot(b[1] - a[1]).ceil() as usize).max(1);
            for k in 0..=steps {
                let t = k as f64 / steps as f64;
                let p = [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (x, y) = (p[0] as usize + dx, p[1] as usize + dy);
                    if x < w && y < h && p[2] + allowance >= depth[y * w + x] {
                        rgb[y * w + x] = [0.2, 0.2, 0.2];
                    }
                }
            }
        }
    }
    for (t, c) in ghost {
        fill(t, *c, 0.45, false, &mut rgb, &mut depth);
    }
    // Down-sample 2× and encode.
    let (ow, oh) = (w / 2, h / 2);
    let mut bytes = Vec::with_capacity(ow * oh * 3);
    for y in 0..oh {
        for x in 0..ow {
            let quad =
                [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| rgb[(2 * y + dy) * w + 2 * x + dx]);
            for k in 0..3 {
                let mean = quad.iter().map(|p| p[k]).sum::<f64>() / 4.0;
                bytes.push((mean * 255.0).round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, ow as u32, oh as u32);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        // Invariant: writing to a Vec cannot fail, and the buffer is exactly ow × oh × 3.
        let mut writer = encoder.write_header().expect("PNG header");
        writer.write_image_data(&bytes).expect("PNG data");
    }
    out
}
