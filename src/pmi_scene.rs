//! Haecceity's semantic/presentation documents with Specify's interview metadata.
//! `Scene` caches the parsed source; answering never reloads STEP or geometry.
use std::collections::BTreeMap;

use haecceity::{
    json::{self as wire, Semantic, presentation},
    p21::Document,
    pmi::{self, NoteOwner, PartId, PartPmi},
    step,
};
use serde_json::{Map, Value, json};

use crate::{api, characteristics, part::LoadedPart, pmi_origin, resume, writer};

type PlannedScene = (Semantic, Map<String, Value>, BTreeMap<String, usize>);

type Bindings = BTreeMap<(String, usize), (String, Value, usize)>;

pub struct Scene {
    doc: Document,
    read: pmi::PmiRead,
    index: usize,
    stored: Option<Map<String, Value>>,
}

impl Scene {
    pub fn new(loaded: &LoadedPart) -> Result<Self, String> {
        let doc = Document::parse(loaded.bytes.to_vec()).map_err(|e| e.to_string())?;
        let defs = step::read_part_definitions(&loaded.bytes).map_err(|e| e.to_string())?;
        let read = pmi::read(&doc, &defs).map_err(|e| e.to_string())?;
        Ok(Self {
            doc,
            read,
            index: loaded.binding.part,
            stored: resume::answers_for(loaded).map_err(|e| e.to_string())?,
        })
    }

    /// `None` shows only the file; `Some` shows the current answers. Stored origin/source are authoritative when showing the file.
    pub fn scene(
        &self,
        loaded: &LoadedPart,
        analysis: Option<&Value>,
        answers: Option<&Map<String, Value>>,
    ) -> Result<Value, String> {
        let before = &self.read.parts[self.index];
        let foreign = self.stored.is_none() && has_pmi(before);
        if let Some(answers) = answers {
            let owned;
            let analysis = match analysis {
                Some(a) => a,
                None => {
                    owned = api::analyse(loaded).map_err(|e| e.to_string())?;
                    &owned
                }
            };
            let (semantic, metadata, counts) = self.planned(loaded, analysis, answers, foreign)?;
            let mut graphics =
                presentation::generated(&semantic, &loaded.part).map_err(|e| e.to_string())?;
            for (id, count) in counts {
                if count > 1
                    && let Some(items) = graphics.items.get_mut(&id)
                {
                    for item in items {
                        item.name = format!("{count}× {}", item.name);
                        if let Some(first) = item.text.first_mut() {
                            *first = format!("{count}× {first}");
                        }
                    }
                }
            }
            let mut result = encode(&semantic, graphics, metadata)?;
            if foreign {
                merge(&mut result, self.file(loaded, None)?)?;
            }
            return Ok(result);
        }
        // Reconstruct only the interview identities/numbers of Specify-written files.
        // PMI values and #77 metadata below always come from the stored model.
        let planned = if let Some(saved) = &self.stored {
            let owned;
            let analysis = match analysis {
                Some(a) => a,
                None => {
                    owned = api::analyse(loaded).map_err(|e| e.to_string())?;
                    &owned
                }
            };
            Some(self.planned(loaded, analysis, saved, false)?.1)
        } else {
            None
        };
        self.file(loaded, planned.as_ref())
    }

    /// Use the editor's review state to distinguish automatic defaults from answers.
    /// The state's `auto` list is authoritative; equal values alone never imply a default.
    pub fn scene_with_state(
        &self,
        loaded: &LoadedPart,
        analysis: Option<&Value>,
        state: &crate::tolerancing::State,
    ) -> Result<Value, String> {
        let mut result = self.scene(loaded, analysis, Some(&state.answers))?;
        if let Some(automatic) = &state.auto {
            for meta in result["specify"]
                .as_object_mut()
                .ok_or("invalid scene metadata")?
                .values_mut()
            {
                if meta["status"] == "answered"
                    && meta["question"]
                        .as_str()
                        .is_some_and(|q| automatic.iter().any(|id| id == q))
                {
                    meta["status"] = json!("default");
                }
            }
        }
        Ok(result)
    }

    fn planned(
        &self,
        loaded: &LoadedPart,
        analysis: &Value,
        answers: &Map<String, Value>,
        foreign: bool,
    ) -> Result<PlannedScene, String> {
        let intent = api::preview(analysis, answers).map_err(|e| e.to_string())?;
        let mut bindings = Bindings::new();
        let mut observe = |collection: &str, index: usize, req: &Map<String, Value>| {
            let id = requirement_id(req);
            let question = req.get("question").and_then(Value::as_str);
            let status = if req.get("general") == Some(&Value::Bool(true)) {
                "general"
            } else if question.is_some_and(|q| answers.contains_key(q)) {
                "answered"
            } else {
                "default"
            };
            let mut meta = json!({"status":status});
            if let Some(q) = question {
                meta["question"] = json!(q);
            }
            let count = analysis["features"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|feature| feature["id"] == req.get("feature").cloned().unwrap_or(Value::Null))
                .and_then(|feature| feature["members"].as_array())
                .map_or(1, |members| members.len().max(1));
            bindings.insert((collection.into(), index), (id, meta, count));
        };
        let before = &self.read.parts[self.index];
        let mut p = writer::scene_model(
            loaded,
            &intent,
            before,
            if foreign {
                pmi::write::Mode::Add
            } else {
                pmi::write::Mode::Replace
            },
            &mut observe,
        )
        .map_err(|e| e.to_string())?;
        characteristics::allocate(
            loaded,
            &mut p,
            before,
            &self.read.provenance.parts[self.index],
            &self.doc,
            Some(answers),
            !foreign,
        )?;
        let numbers = characteristics::planned_numbers(&p)?;
        let mut semantic = wire::identify(p, None).map_err(|e| e.to_string())?;
        let mut metadata = Map::new();
        let mut counts = BTreeMap::new();
        let mut number_index = 0;
        // Same collection order as characteristics::items.
        for collection in [
            "datums",
            "dimensions",
            "tolerances",
            "surface_textures",
            "threads",
            "knurls",
        ] {
            let len = semantic.ids[collection].len();
            for i in 0..len {
                let number = numbers.get(number_index).copied().flatten();
                number_index += 1;
                if let Some((id, mut meta, count)) = bindings.remove(&(collection.into(), i)) {
                    if let Some(n) = number {
                        meta["number"] = json!(n);
                    }
                    stored_metadata(&semantic.pmi, collection, i, &mut meta);
                    semantic
                        .ids
                        .get_mut(collection)
                        .ok_or("missing id collection")?[i] = id.clone();
                    if metadata.insert(id.clone(), meta).is_some() {
                        return Err(format!("ambiguous Specify requirement id {id}"));
                    }
                    counts.insert(id, count);
                }
            }
        }
        part_metadata(&mut semantic, &mut metadata, answers)?;
        fill_metadata(&semantic, &mut metadata, "general");
        Ok((semantic, metadata, counts))
    }

    fn file(
        &self,
        _loaded: &LoadedPart,
        planned: Option<&Map<String, Value>>,
    ) -> Result<Value, String> {
        let p = &self.read.parts[self.index];
        // identify (without named anchor bindings) deliberately exports mesh indices.
        let mut semantic =
            wire::identify(p.clone(), Some(&self.read.identifiers.parts[self.index]))
                .map_err(|e| e.to_string())?;
        let mut metadata = Map::new();
        for (collection, ids) in &mut semantic.ids {
            for (i, id) in ids.iter_mut().enumerate() {
                let mut meta = json!({"status":"existing"});
                if let Some(n) = id.parse::<u64>().ok().filter(|n| *n > 0) {
                    meta["number"] = json!(n);
                    if let Some((stable, saved)) =
                        planned.and_then(|m| m.iter().find(|(_, v)| v["number"] == n))
                    {
                        *id = stable.clone();
                        meta = saved.clone();
                        if let Some(object) = meta.as_object_mut() {
                            object.remove("origin");
                            object.remove("source");
                        }
                    }
                }
                stored_metadata(p, collection, i, &mut meta);
                metadata.insert(id.clone(), meta);
            }
        }
        if let Some(answers) = &self.stored {
            part_metadata(&mut semantic, &mut metadata, answers)?;
            // Drop obsolete content ids after renaming the part settings.
            let ids: std::collections::BTreeSet<_> = semantic.ids.values().flatten().collect();
            metadata.retain(|id, _| ids.contains(id));
        }
        let graphics = presentation::read(&self.doc, &self.read, PartId(self.index), &semantic)
            .map_err(|e| e.to_string())?;
        encode(&semantic, graphics, metadata)
    }
}

fn part_metadata(
    semantic: &mut Semantic,
    metadata: &mut Map<String, Value>,
    answers: &Map<String, Value>,
) -> Result<(), String> {
    for collection in ["general", "surface_textures", "attributes"] {
        for i in 0..semantic.ids[collection].len() {
            let key = match collection {
                "general" => Some("general_tolerance"),
                "surface_textures" if semantic.pmi.surface_textures[i].on.is_none() => {
                    Some("finish")
                }
                "attributes"
                    if semantic.pmi.attributes[i].name == writer::SEMANTIC_TEXT
                        && semantic.pmi.attributes[i].on.is_none() =>
                {
                    semantic.pmi.attributes[i]
                        .items
                        .first()
                        .map(|(k, _)| k.as_str())
                }
                _ => None,
            };
            if let Some(key) = key {
                let question = match key {
                    "general note" => "part.notes".to_owned(),
                    "surface treatment" => "part.surface_treatment".into(),
                    "heat treatment" => "part.heat_treatment".into(),
                    "edge condition" => "part.edge_condition".into(),
                    _ => format!("part.{key}"),
                };
                let id = if key == "general note" {
                    let index = semantic.pmi.attributes[i]
                        .items
                        .iter()
                        .find_map(|(k, v)| {
                            if k == writer::NOTE_INDEX {
                                if let pmi::AttributeValue::Integer(n) = v {
                                    Some(n.to_string())
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        })
                        .unwrap_or_else(|| i.to_string());
                    format!("{question}:{index}")
                } else {
                    question.clone()
                };
                semantic
                    .ids
                    .get_mut(collection)
                    .ok_or("missing id collection")?[i] = id.clone();
                metadata.insert(id, json!({"status": if answers.contains_key(&question) {"answered"} else {"default"}, "question":question}));
            }
        }
    }
    Ok(())
}

fn has_pmi(p: &PartPmi) -> bool {
    let mut p = p.clone();
    p.attributes.retain(|a| !resume::is_answers(a));
    p != PartPmi::default()
}

fn requirement_id(req: &Map<String, Value>) -> String {
    if let Some(letter) = req.get("letter").and_then(Value::as_str) {
        return format!("datum:{letter}");
    }
    let kind = req.get("kind").and_then(Value::as_str).unwrap_or("item");
    let question = req.get("question").and_then(Value::as_str).unwrap_or("");
    let target = if question.starts_with("extra.") {
        question.to_string()
    } else if let Some(feature) = req.get("feature").and_then(Value::as_str) {
        feature.to_owned()
    } else {
        format!("faces:{}", req.get("faces").unwrap_or(&Value::Null))
    };
    let mut id = format!("{kind}:{target}");
    if kind == "location" {
        id.push_str(&format!(
            ":{}",
            req.get("reference").unwrap_or(&Value::Null)
        ));
    }
    id
}

fn stored_metadata(p: &PartPmi, collection: &str, i: usize, meta: &mut Value) {
    if collection != "dimensions" {
        return;
    }
    let stored = pmi_origin::read(p, NoteOwner::Dimension(pmi::DimensionId(i)));
    if let Some(origin) = stored.origin {
        meta["origin"] = json!(origin);
        match origin {
            pmi_origin::Origin::GeneralTolerance => meta["status"] = json!("general"),
            pmi_origin::Origin::Imported => meta["status"] = json!("existing"),
            pmi_origin::Origin::Requirement if meta["status"] == "existing" => {
                meta["status"] = json!("answered")
            }
            _ => {}
        }
    }
    if let Some(source) = stored.source {
        meta["source"] = json!(source);
    }
}

fn fill_metadata(semantic: &Semantic, metadata: &mut Map<String, Value>, status: &str) {
    for ids in semantic.ids.values() {
        for id in ids {
            metadata
                .entry(id.clone())
                .or_insert_with(|| json!({"status":status}));
        }
    }
}

fn encode(
    semantic: &Semantic,
    graphics: presentation::Presentation,
    metadata: Map<String, Value>,
) -> Result<Value, String> {
    Ok(
        json!({"pmi": wire::to_value(semantic).map_err(|e|e.to_string())?, "presentation": graphics, "specify":metadata}),
    )
}

fn merge(target: &mut Value, other: Value) -> Result<(), String> {
    for (collection, rows) in other["pmi"]["pmi"]
        .as_object()
        .ok_or("invalid PMI document")?
    {
        if let Some(rows) = rows.as_array() {
            let output = target["pmi"]["pmi"][collection]
                .as_array_mut()
                .ok_or("missing PMI collection")?;
            for row in rows {
                if !output.iter().any(|r| r["id"] == row["id"]) {
                    output.push(row.clone());
                }
            }
        } else if target["pmi"]["pmi"][collection].is_null() {
            target["pmi"]["pmi"][collection] = rows.clone();
        }
    }
    for table in ["length", "angle", "density", "other"] {
        let output = target["pmi"]["units"][table]
            .as_array_mut()
            .ok_or("missing unit table")?;
        for unit in other["pmi"]["units"][table]
            .as_array()
            .ok_or("invalid unit table")?
        {
            if !output.iter().any(|u| u["id"] == unit["id"]) {
                output.push(unit.clone());
            }
        }
    }
    for key in ["specify", "presentation"] {
        if key == "specify" {
            target[key]
                .as_object_mut()
                .ok_or("invalid metadata")?
                .extend(other[key].as_object().ok_or("invalid metadata")?.clone());
        } else {
            target[key]["items"]
                .as_object_mut()
                .ok_or("invalid graphics")?
                .extend(
                    other[key]["items"]
                        .as_object()
                        .ok_or("invalid graphics")?
                        .clone(),
                );
            for field in ["semantic_ids", "unlinked"] {
                target[key][field]
                    .as_array_mut()
                    .ok_or("invalid graphics")?
                    .extend(
                        other[key][field]
                            .as_array()
                            .ok_or("invalid graphics")?
                            .clone(),
                    );
            }
            let ids = target[key]["semantic_ids"]
                .as_array_mut()
                .ok_or("invalid ids")?;
            ids.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
            ids.dedup();
        }
    }
    // Both inputs are trusted haecceity exports. Keep import/schema validation out
    // of production builds; package tests validate the complete merged documents.
    for field in ["planes", "views"] {
        if let Some(rows) = other["presentation"][field].as_array() {
            let presentation = target["presentation"]
                .as_object_mut()
                .ok_or("invalid graphics")?;
            let output = presentation
                .entry(field)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or("invalid presentation collection")?;
            for row in rows {
                if let Some(old) = output.iter().find(|old| old["id"] == row["id"]) {
                    if old != row {
                        return Err(format!("conflicting presentation {field} id {}", row["id"]));
                    }
                } else {
                    output.push(row.clone());
                }
            }
        }
    }
    Ok(())
}

/// Raw STEP view-only form. Select an assembly part by its `parts()` index.
pub fn pmi_scene(bytes: &[u8], part: Option<usize>) -> Result<Value, String> {
    let file = crate::part::File::of_bytes(std::path::Path::new("scene.step"), bytes.to_vec())
        .map_err(|e| e.to_string())?;
    let loaded = file.load(part).map_err(|e| e.to_string())?;
    Scene::new(&loaded)?.scene(&loaded, None, None)
}
