//! Requirement changes on the same semantic documents and identities as `pmiScene`.
use super::{Bind, Feature, PmiItemChange, pmi_on};
use crate::{api, part::LoadedPart, pmi_scene::Scene};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

struct Item {
    id: String,
    number: Option<u64>,
    kind: String,
    match_kind: String,
    on_faces: Vec<usize>,
    status: String,
    faces: Vec<usize>,
    text: String,
    cells: Value,
    semantic: Value,
}

// Resolve support and unit identities, so file-local hashes/indices are not changes.
// Topology is matched separately, including one old face becoming several new faces.
fn resolve(value: &Value, rows: &BTreeMap<&str, (&str, &Value)>, depth: usize) -> Value {
    if depth > 32 {
        return json!("recursive support");
    }
    match value {
        Value::String(id) if rows.contains_key(id.as_str()) => {
            let (collection, value) = rows[id.as_str()];
            if collection == "features" {
                json!("feature support")
            } else {
                resolve(value, rows, depth + 1)
            }
        }
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| !["face", "edge", "deriving_faces"].contains(&key.as_str()))
                .map(|(key, v)| (key.clone(), resolve(v, rows, depth + 1)))
                .collect(),
        ),
        Value::Array(array) => {
            Value::Array(array.iter().map(|v| resolve(v, rows, depth + 1)).collect())
        }
        _ => value.clone(),
    }
}

fn supports(
    value: &Value,
    rows: &BTreeMap<&str, (&str, &Value)>,
    seen: &mut BTreeSet<String>,
    out: &mut Vec<usize>,
) {
    match value {
        Value::String(id) => {
            if let Some((collection, v)) = rows.get(id.as_str())
                // Datum references describe the frame, not the controlled feature.
                && !["datums", "datum_targets"].contains(collection)
                && seen.insert(id.clone())
            {
                supports(v, rows, seen, out);
            }
        }
        Value::Object(object) => {
            for (key, v) in object {
                if key == "face" {
                    out.extend(v.as_u64().map(|n| n as usize));
                } else if key == "deriving_faces" {
                    out.extend(
                        v.as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_u64)
                            .map(|n| n as usize),
                    );
                } else if key != "datums" {
                    supports(v, rows, seen, out);
                }
            }
        }
        Value::Array(array) => {
            for v in array {
                supports(v, rows, seen, out);
            }
        }
        _ => {}
    }
}

fn items(scene: &Value) -> Vec<Item> {
    let mut rows = BTreeMap::new();
    for section in ["pmi", "units"] {
        for (collection, list) in scene["pmi"][section].as_object().into_iter().flatten() {
            for row in list.as_array().into_iter().flatten() {
                if let Some(id) = row["id"].as_str() {
                    rows.insert(id, (collection.as_str(), &row["value"]));
                }
            }
        }
    }
    let mut result = Vec::new();
    for (id, meta) in scene["specify"].as_object().into_iter().flatten() {
        let Some(&(collection, value)) = rows.get(id.as_str()) else {
            continue;
        };
        if collection == "attributes"
            && [
                crate::pmi_origin::PROPERTY,
                crate::characteristics::PROPERTY,
            ]
            .contains(&value["name"].as_str().unwrap_or_default())
        {
            continue;
        }
        let graphics = &scene["presentation"]["items"][id];
        let text = graphics
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| g["name"].as_str())
            .collect::<Vec<_>>()
            .join("; ");
        let kind = match collection {
            "dimensions" => "dimension",
            "tolerances" => "geometric_tolerance",
            "datums" => "datum",
            "notes" | "attributes" => "note",
            "surface_textures" => "surface_texture",
            "threads" => "thread",
            "knurls" => "knurl",
            "general" => "general_tolerance",
            "datum_targets" => "datum_target",
            other => other,
        };
        let text = if text.is_empty() {
            // Some semantic requirements (e.g. manufacturing attributes) have no callout.
            value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or(kind)
                .to_owned()
        } else {
            text
        };
        let cells = json!(
            graphics
                .as_array()
                .into_iter()
                .flatten()
                .map(|g| g["text"].clone())
                .collect::<Vec<_>>()
        );
        let mut faces = Vec::new();
        supports(value, &rows, &mut BTreeSet::new(), &mut faces);
        faces.sort_unstable();
        faces.dedup();
        result.push(Item {
            id: id.clone(),
            number: meta["number"].as_u64(),
            kind: kind.into(),
            match_kind: format!(
                "{kind}:{}",
                if collection == "dimensions" {
                    value
                        .pointer("/kind/size/kind")
                        .or_else(|| value.pointer("/kind/location/kind"))
                        .unwrap_or(&Value::Null)
                } else if collection == "tolerances" {
                    &value["kind"]
                } else {
                    &Value::Null
                }
            ),
            on_faces: {
                let mut primary = Vec::new();
                let target = value.pointer("/kind/location/to").unwrap_or(value);
                supports(target, &rows, &mut BTreeSet::new(), &mut primary);
                primary.sort_unstable();
                primary.dedup();
                primary
            },
            status: meta["status"].as_str().unwrap_or("existing").into(),
            faces,
            text,
            cells,
            semantic: resolve(value, &rows, 0),
        });
    }
    result
}

fn mapped(item: &Item, faces: &BTreeMap<usize, Bind>) -> Option<Vec<usize>> {
    let mut result = Vec::new();
    for face in &item.faces {
        match faces.get(face) {
            Some(Bind::To(face)) => result.push(*face),
            Some(Bind::Ambiguous(candidates)) if !candidates.is_empty() => {
                result.extend(candidates)
            }
            _ => return None,
        }
    }
    result.sort_unstable();
    result.dedup();
    Some(result)
}

fn compare(
    old: &[Item],
    new: &[Item],
    faces: &BTreeMap<usize, Bind>,
    old_features: &[Feature],
    new_features: &[Feature],
) -> Vec<PmiItemChange> {
    let mut pairs = BTreeMap::new();
    let mut used = BTreeSet::new();
    // Complete each stronger pass before any weaker match can consume its candidate.
    for pass in 0..4 {
        for (i, was) in old
            .iter()
            .enumerate()
            .filter(|(i, _)| !pairs.contains_key(i))
            .collect::<Vec<_>>()
        {
            let mapped = mapped(was, faces);
            let candidates: Vec<_> = new
                .iter()
                .enumerate()
                .filter(|(j, now)| {
                    !used.contains(j)
                        && match pass {
                            0 => was.number.is_some() && was.number == now.number,
                            1 => was.id == now.id,
                            2 => {
                                mapped.as_ref().is_some_and(|f| *f == now.faces)
                                    && !(was.number.is_some()
                                        && now.number.is_some()
                                        && was.number != now.number)
                            }
                            _ => {
                                was.match_kind == now.match_kind
                                    && !(was.number.is_some()
                                        && now.number.is_some()
                                        && was.number != now.number)
                                    && old
                                        .iter()
                                        .enumerate()
                                        .filter(|(k, other)| {
                                            !pairs.contains_key(k)
                                                && other.match_kind == was.match_kind
                                        })
                                        .count()
                                        == 1
                            }
                        }
                })
                .collect();
            let candidates = if candidates.len() > 1 || pass == 2 {
                candidates
                    .into_iter()
                    .filter(|(_, now)| now.kind == was.kind)
                    .collect::<Vec<_>>()
            } else {
                candidates
            };
            if let [(j, _)] = candidates.as_slice() {
                pairs.insert(i, *j);
                used.insert(*j);
            }
        }
    }
    let change = |class: &str, was: Option<&Item>, now: Option<&Item>, reason: Option<String>| {
        let Some(item) = now.or(was) else {
            unreachable!("a change has an item")
        };
        PmiItemChange {
            class: class.into(),
            number: item.number,
            id: item.id.clone(),
            kind: item.kind.clone(),
            status: item.status.clone(),
            on: pmi_on(
                &item.on_faces,
                if now.is_some() {
                    new_features
                } else {
                    old_features
                },
            ),
            old: was.map(|p| p.text.clone()),
            new: now.map(|p| p.text.clone()),
            reason,
        }
    };
    let mut result = Vec::new();
    for (i, was) in old.iter().enumerate() {
        if let Some(&j) = pairs.get(&i) {
            let now = &new[j];
            if was.text != now.text || was.cells != now.cells || was.semantic != now.semantic {
                let reason = (was.text == now.text).then(|| "semantic fields changed".into());
                result.push(change("changed", Some(was), Some(now), reason));
            }
            if was.number != now.number {
                result.push(change(
                    "renumbered",
                    Some(was),
                    Some(now),
                    Some(format!(
                        "characteristic number {} → {}",
                        was.number.map_or("none".into(), |n| format!("#{n}")),
                        now.number.map_or("none".into(), |n| format!("#{n}"))
                    )),
                ));
            }
        } else {
            result.push(change(
                "removed",
                Some(was),
                None,
                was.number.map(|_| "retired".into()),
            ));
        }
    }
    for (_, now) in new.iter().enumerate().filter(|(j, _)| !used.contains(j)) {
        result.push(change("added", None, Some(now), None));
    }
    result.sort_by(|a, b| {
        (a.number.is_none(), a.number, &a.id, &a.class).cmp(&(
            b.number.is_none(),
            b.number,
            &b.id,
            &b.class,
        ))
    });
    result
}

pub(super) fn diff(
    old: &LoadedPart,
    new: &LoadedPart,
    analyses: (&Value, &Value),
    given: Option<&Map<String, Value>>,
    carried: &Map<String, Value>,
    faces: &BTreeMap<usize, Bind>,
    features: (&[Feature], &[Feature]),
) -> api::Result<Vec<PmiItemChange>> {
    let scene = |part: &LoadedPart, analysis: &Value, answers| {
        Scene::new(part)
            .and_then(|scene| scene.comparison(part, analysis, answers))
            .map_err(api::Error::Analysis)
    };
    let (old_analysis, new_analysis) = analyses;
    let before = scene(old, old_analysis, given)?;
    // A written new revision owns its requirements. Otherwise show what writing
    // carried answers would produce, alongside the new CAD file's own PMI.
    let answers = (!new_analysis["resume"].is_object() && !carried.is_empty()).then_some(carried);
    let after = scene(new, new_analysis, answers)?;
    Ok(compare(
        &items(&before),
        &items(&after),
        faces,
        features.0,
        features.1,
    ))
}
