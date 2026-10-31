//! Boundary to quiddity's shared requirement planner (#75). No dimension selection lives here.
//!
//! Analyses carry the typed recognition input so rules can plan without reopening geometry.
//! Older snapshots are upgraded at the writer boundary using the loaded part. Legacy rows
//! survive only for explicitly deferred owners (including round Cartesian envelopes).
use std::collections::{BTreeMap, BTreeSet};

use quiddity::dimensioning::{
    self as shared, Datum, Input, Kind, Policy, Required, Support, Tolerance,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::rules::Intent;

pub(crate) fn input(part: &haecceity::Part) -> Result<Input, String> {
    Input::from_recognition(part, vec![], Policy::default())
}

fn ids(r: &Map<String, Value>, key: &str) -> Vec<usize> {
    r.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_u64().and_then(|n| usize::try_from(n).ok()))
        .collect()
}
fn overlaps(a: &[usize], b: &[usize]) -> bool {
    a.iter().any(|i| b.contains(i))
}
fn on_holes(input: &Input, faces: &[usize]) -> bool {
    input
        .holes
        .iter()
        .any(|h| overlaps(faces, &h.support.faces))
}
fn deferred(input: &Input, r: &Map<String, Value>) -> bool {
    let faces = ids(r, "faces");
    if on_holes(input, &faces) {
        return false;
    }
    if r.get("kind").and_then(Value::as_str) == Some("size") {
        return true;
    }
    if !input.cartesian_envelope {
        return true;
    }
    input.deferred.iter().any(|d| overlaps(&faces, &d.faces))
}
fn contract(r: &Map<String, Value>) -> Tolerance {
    if r.get("basic") == Some(&json!(true)) {
        Tolerance::Basic
    } else if r.get("general") == Some(&json!(true)) {
        Tolerance::General
    } else {
        Tolerance::Explicit(
            json!([
                r.get("fit"),
                r.get("upper"),
                r.get("lower"),
                r.get("stored_tolerance")
            ])
            .to_string(),
        )
    }
}
fn support(input: &Input, faces: &[usize]) -> Option<Support> {
    input
        .holes
        .iter()
        .map(|h| &h.support)
        .chain(input.planes.iter().map(|p| &p.support))
        .find(|s| !faces.is_empty() && faces.iter().all(|i| s.faces.contains(i)))
        .cloned()
}

/// Complete lowering information travels with a row, including support coordinates and the
/// planner's count, pattern role, reference proof and covers. Arena indices never cross JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Planned {
    pub required: Required,
    pub supports: Vec<Support>,
    pub groups: BTreeMap<String, Vec<String>>,
    pub frame: quiddity::frames::PartFrame,
}

/// Replace slice-one rows by exactly the shared planner's output. Answered location *contracts*
/// apply to the pattern's own origin and pitch; old per-member basics are generated witnesses,
/// not independently authored dimensions. Explicit face-picked dimensions remain authorities.
pub(crate) fn apply(mut input: Input, intent: &mut Intent) -> Result<(), String> {
    input.datums = intent
        .datums
        .iter()
        .filter_map(|r| {
            Some(Datum {
                label: r.get("letter")?.as_str()?.into(),
                faces: ids(r, "faces"),
            })
        })
        .collect();
    let general = intent.part.get("general_tolerance").and_then(Value::as_str);
    input.policy.general_tolerance = general.is_some();
    input.specified.clear();
    let mut authorities = BTreeMap::new();

    // Ask the planner for hole grammar independently of plane consolidation so a Basic
    // hole origin cannot first disappear into an equal General plane interval.
    let mut holes = input.clone();
    holes.steps = Default::default();
    holes.cartesian_envelope = false;
    holes.deferred.clear();
    let (hole_requirements, _) = shared::plan(&holes);
    let generated_location = |r: &Map<String, Value>| {
        r.get("kind").and_then(Value::as_str) == Some("location")
            && r.get("question")
                .and_then(Value::as_str)
                .is_some_and(|q| q.starts_with("hole.position:") || q.starts_with("hole.location:"))
    };
    // Feed each answered contract to the planner before coincident-span consolidation.
    // A General neighbour must not swallow the pattern origin needed by a position FCF.
    let mut contracts: BTreeMap<String, (Map<String, Value>, Vec<String>)> = BTreeMap::new();
    for h in &input.holes {
        if let Some(answer) = intent
            .requirements
            .iter()
            .find(|r| generated_location(r) && overlaps(&ids(r, "faces"), &h.support.faces))
        {
            let question = answer.get("question").and_then(Value::as_str).unwrap_or("");
            let planes: BTreeSet<Vec<usize>> = intent
                .requirements
                .iter()
                .filter(|r| {
                    generated_location(r)
                        && r.get("question").and_then(Value::as_str) == Some(question)
                })
                .map(|r| ids(r, "reference"))
                .collect();
            let key = json!([contract(answer), planes]).to_string();
            contracts
                .entry(key)
                .or_insert_with(|| (answer.clone(), vec![]))
                .1
                .push(h.support.feature.clone());
        }
    }
    for (answer, members) in contracts.into_values() {
        let mut contracted = holes.clone();
        contracted
            .holes
            .retain(|h| members.contains(&h.support.feature));
        contracted
            .patterns
            .retain(|p| p.members.iter().all(|m| members.contains(m)));
        let question = answer.get("question").and_then(Value::as_str).unwrap_or("");
        let planes: BTreeSet<_> = intent
            .requirements
            .iter()
            .filter(|r| {
                generated_location(r) && r.get("question").and_then(Value::as_str) == Some(question)
            })
            .map(|r| ids(r, "reference"))
            .collect();
        contracted.datums.retain(|d| planes.contains(&d.faces));
        for mut required in shared::plan(&contracted).0 {
            let Kind::Location { from, .. } = &required.kind else {
                continue;
            };
            if !matches!(required.pattern, Some(shared::PatternRole::Pitch { .. }))
                && !planes
                    .iter()
                    .any(|p| !p.is_empty() && p.iter().all(|i| from.faces.contains(i)))
            {
                continue;
            }
            let witnesses: Vec<_> = intent
                .requirements
                .iter()
                .filter(|r| {
                    generated_location(r)
                        && input.holes.iter().any(|h| {
                            required.covers.contains(&h.support.feature)
                                && overlaps(&ids(r, "faces"), &h.support.faces)
                        })
                })
                .collect();
            let witness = witnesses
                .iter()
                .copied()
                .find(|r| match &required.kind {
                    Kind::Location { to, .. } => ids(r, "faces") == to.faces,
                    _ => false,
                })
                .or_else(|| witnesses.first().copied())
                .unwrap_or(&answer);
            required.id = format!("answered:{}", required.id);
            required.tolerance = contract(witness);
            authorities.insert(required.id.clone(), witness.clone());
            input.specified.push(required);
        }
    }
    for r in &intent.requirements {
        let kind = r.get("kind").and_then(Value::as_str).unwrap_or("");
        if generated_location(r) || !matches!(kind, "size" | "location") || deferred(&input, r) {
            continue;
        }
        let faces = ids(r, "faces");
        let mut required = if kind == "size" {
            let selected: BTreeSet<_> = input
                .holes
                .iter()
                .filter(|h| overlaps(&faces, &h.support.faces))
                .map(|h| h.support.feature.clone())
                .collect();
            let whole = hole_requirements
                .iter()
                .find(|s| {
                    matches!(&s.kind, Kind::Size {role,..} if role=="bore.diameter")
                        && s.covers.iter().cloned().collect::<BTreeSet<_>>() == selected
                })
                .cloned();
            whole.or_else(|| {
                let feature = if selected.len() == 1 {
                    selected.first()?.clone()
                } else {
                    format!(
                        "multiple[{}]",
                        selected.iter().cloned().collect::<Vec<_>>().join(",")
                    )
                };
                Some(Required {
                    id: format!("dim:{feature}/bore.diameter"),
                    kind: Kind::Size {
                        feature,
                        role: "bore.diameter".into(),
                        kind: shared::SizeKind::Diameter,
                    },
                    nominal: r.get("nominal")?.as_f64()?,
                    count: selected.len(),
                    pattern: None,
                    tolerance: contract(r),
                    reference: None,
                    covers: selected.into_iter().collect(),
                })
            })
        } else {
            let to = support(&input, &faces);
            let from = support(&input, &ids(r, "reference"));
            match (from, to) {
                (Some(from), Some(to)) => {
                    let axis = input
                        .planes
                        .iter()
                        .find(|p| p.support == from)
                        .map(|p| p.axis)
                        .unwrap_or_else(|| {
                            (0..3)
                                .max_by(|&a, &b| {
                                    (to.at[a] - from.at[a])
                                        .abs()
                                        .total_cmp(&(to.at[b] - from.at[b]).abs())
                                })
                                .unwrap_or(0)
                        });
                    Some(Required {
                        id: format!("dim:{}/to/{}/{}", from.feature, to.feature, axis),
                        nominal: r.get("distance").and_then(Value::as_f64).unwrap_or(0.),
                        covers: vec![to.feature.clone()],
                        kind: Kind::Location { from, to, axis },
                        count: 1,
                        pattern: None,
                        tolerance: contract(r),
                        reference: None,
                    })
                }
                _ => None,
            }
        };
        if let Some(s) = &mut required {
            // Preserve the exact stated nominal and tolerance. Full-group answers use the
            // planner's group identity, so specified-wins retires its generated equivalent.
            s.id = format!(
                "{}:{}",
                if r.get("stored") == Some(&json!(true)) {
                    "stored"
                } else {
                    "specified"
                },
                s.id
            );
            s.nominal = r
                .get(if kind == "size" {
                    "nominal"
                } else {
                    "distance"
                })
                .and_then(Value::as_f64)
                .unwrap_or(s.nominal);
            s.tolerance = contract(r);
            authorities.insert(s.id.clone(), r.clone());
            input.specified.push(s.clone());
        }
    }
    let (required, _) = shared::plan(&input);
    let mut supports: Vec<_> = input
        .holes
        .iter()
        .map(|h| h.support.clone())
        .chain(input.planes.iter().map(|p| p.support.clone()))
        .collect();
    let mut groups = BTreeMap::new();
    for r in &required {
        if let Kind::Location { from, to, .. } = &r.kind {
            supports.extend([from.clone(), to.clone()]);
        }
        if let Kind::Size { feature, .. } = &r.kind
            && (feature.starts_with("pattern[") || feature.starts_with("multiple["))
        {
            groups.insert(
                feature.clone(),
                r.covers
                    .iter()
                    .filter(|id| input.holes.iter().any(|h| &h.support.feature == *id))
                    .cloned()
                    .collect(),
            );
        }
    }
    for p in &input.patterns {
        groups.insert(p.id(), p.members.clone());
    }
    let old = intent.requirements.clone();
    let is_authority = |r: &Map<String, Value>| authorities.values().any(|a| a == r);
    intent.requirements.retain(|r| {
        let kind = r.get("kind").and_then(Value::as_str).unwrap_or("");
        !matches!(kind, "size" | "location")
            || deferred(&input, r)
            || (!generated_location(r) && !is_authority(r))
    });
    intent.general.retain(|r| deferred(&input, r));
    for required in required {
        if general.is_none() && !authorities.contains_key(&required.id) {
            continue;
        }
        let mut row = authorities.get(&required.id).cloned().unwrap_or_default();
        let (kind, feature, faces, reference) = match &required.kind {
            Kind::Size { feature, .. } => {
                let faces: Vec<_> = required
                    .covers
                    .iter()
                    .flat_map(|id| input.holes.iter().filter(move |h| &h.support.feature == id))
                    .flat_map(|h| h.support.faces.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                ("size", feature.clone(), faces, vec![])
            }
            Kind::Location { from, to, .. } => (
                "location",
                to.feature.clone(),
                to.faces.clone(),
                from.faces.clone(),
            ),
        };
        // Keep Specify's recognised feature spelling for origin/source and question consumers.
        let named = old
            .iter()
            .find(|r| ids(r, "faces") == faces)
            .and_then(|r| r.get("feature"))
            .cloned()
            .unwrap_or_else(|| {
                if feature.starts_with("hole:") {
                    json!(feature.replacen("hole:", "holes:", 1))
                } else {
                    json!("")
                }
            });
        row.insert("kind".into(), json!(kind));
        row.insert("feature".into(), named);
        row.insert("faces".into(), json!(faces));
        row.insert(
            if kind == "size" {
                "nominal"
            } else {
                "distance"
            }
            .into(),
            json!(required.nominal),
        );
        if kind == "location" {
            row.insert("reference".into(), json!(reference));
        }
        row.remove("basic");
        row.remove("general");
        match &required.tolerance {
            Tolerance::Basic => {
                row.remove("upper");
                row.remove("lower");
                row.insert("basic".into(), json!(true));
            }
            Tolerance::General => {
                row.insert("general".into(), json!(true));
                if let Some(t) =
                    general.and_then(|g| crate::general::deviation(g, required.nominal))
                {
                    row.insert("upper".into(), json!(t));
                    row.insert("lower".into(), json!(-t));
                }
            }
            Tolerance::Untoleranced => {}
            Tolerance::Explicit(_) => {}
        }
        let is_general = matches!(
            required.tolerance,
            Tolerance::General | Tolerance::Untoleranced
        );
        let mut used = BTreeSet::new();
        match &required.kind {
            Kind::Size { feature, .. } => {
                used.insert(feature.clone());
            }
            Kind::Location { from, to, .. } => {
                used.insert(from.feature.clone());
                used.insert(to.feature.clone());
            }
        }
        let used_groups: BTreeMap<_, _> = groups
            .iter()
            .filter(|(id, _)| used.contains(*id))
            .map(|(id, members)| (id.clone(), members.clone()))
            .collect();
        for members in used_groups.values() {
            used.extend(members.iter().cloned());
        }
        let used_supports = supports
            .iter()
            .filter(|s| used.contains(&s.feature))
            .cloned()
            .collect();
        row.insert(
            "planner".into(),
            serde_json::to_value(Planned {
                required,
                supports: used_supports,
                groups: used_groups,
                frame: input.frame,
            })
            .map_err(|e| e.to_string())?,
        );
        if is_general {
            intent.general.push(row);
        } else {
            intent.requirements.push(row);
        }
    }
    Ok(())
}

/// Already-stated dimensions are planner authorities, not fresh writer jobs. Their exact
/// tolerance is opaque to the planner; the original PMI remains the authority in Add mode.
pub(crate) fn stated(p: &haecceity::pmi::PartPmi) -> Vec<Map<String, Value>> {
    use haecceity::pmi::{DimTolerance, DimensionKind};
    p.dimensions
        .iter()
        .filter_map(|d| {
            let mut r = Map::new();
            let value = crate::existing::value(d.nominal.as_ref()?);
            match &d.kind {
                DimensionKind::Size {
                    feature,
                    kind: haecceity::pmi::SizeKind::Diameter,
                    ..
                } => {
                    r.insert("kind".into(), json!("size"));
                    r.insert(
                        "faces".into(),
                        json!(crate::existing::faces_of(p, &[*feature])),
                    );
                    r.insert("nominal".into(), json!(value));
                }
                DimensionKind::Location {
                    from,
                    to,
                    kind: haecceity::pmi::LocationKind::LinearDistance,
                    ..
                } => {
                    r.insert("kind".into(), json!("location"));
                    r.insert("faces".into(), json!(crate::existing::faces_of(p, &[*to])));
                    r.insert(
                        "reference".into(),
                        json!(crate::existing::faces_of(p, &[*from])),
                    );
                    r.insert("distance".into(), json!(value));
                }
                _ => return None,
            }
            r.insert("stored".into(), json!(true));
            r.insert(
                "stored_tolerance".into(),
                json!(format!("{:?}", d.tolerance)),
            );
            if matches!(d.tolerance, DimTolerance::Basic) {
                r.insert("basic".into(), json!(true));
            }
            Some(r)
        })
        .collect()
}
