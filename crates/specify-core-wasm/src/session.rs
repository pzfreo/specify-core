use crate::input;
use haecceity::step;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use specify_core::{
    api, carried,
    features::Feature,
    part::{Binding, LOADER_VERSION, LoadedPart},
    resume, tolerancing,
};
use std::{path::PathBuf, sync::Arc};

pub struct Session {
    loaded: LoadedPart,
    analysis: Value,
}

impl Session {
    pub fn open(name: &str, bytes: &[u8]) -> Result<Self, String> {
        input::size("STEP input", bytes.len(), input::STEP_LIMIT).map_err(|e| e.to_string())?;
        input::size("Input name", name.len(), input::TEXT_LIMIT).map_err(|e| e.to_string())?;
        if bytes.starts_with(&[0x1f, 0x8b]) {
            return Err("Decompress the STEP file before opening it.".into());
        }
        let definitions = step::read_part_definitions(bytes).map_err(|error| error.to_string())?;
        if definitions.len() != 1 {
            return Err(format!(
                "Specify browser loading currently requires one distinct part; found {}. Assembly selection is not available yet.",
                definitions.len()
            ));
        }
        let definition = definitions.into_iter().next().ok_or_else(||
            "Specify browser loading currently requires one distinct part; found 0. Assembly selection is not available yet.".to_string())?;
        if definition.placements.len() != 1 {
            return Err(
                "Specify browser loading currently requires one instance of the part.".into(),
            );
        }
        // The length check above proves this is the sole placement.
        let placement = definition.placements[0].placement;
        // The kernel owns the part/frame/source mapping; refuse any ordered-face mismatch.
        let part = step::read_part(bytes, &definition).map_err(|error| error.to_string())?;
        if part.faces.len() != definition.faces.len()
            || definition.faces.iter().enumerate().any(|(i, entity)| {
                part.face_source(i)
                    .is_none_or(|source| source.entity != *entity)
            })
        {
            return Err(
                "The browser loader cannot preserve this part's ordered STEP face identity.".into(),
            );
        }
        let loaded = LoadedPart {
            path: PathBuf::from(name),
            bytes: Arc::new(bytes.to_vec()),
            binding: Binding {
                source_sha256: Sha256::digest(bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
                face_count: part.faces.len(),
                loader_version: LOADER_VERSION,
                part: 0,
            },
            unresolved_faces: part.unresolved_faces().to_vec(),
            unresolved_edges: part.unresolved_edges().to_vec(),
            part,
            definition,
            part_count: 1,
            name: name.into(),
            placements: vec![placement],
            rebuilt: false,
        };
        let analysis = api::analyse(&loaded).map_err(|error| error.to_string())?;
        Ok(Self { loaded, analysis })
    }

    pub fn snapshot(&self) -> Value {
        json!({"analysis": self.analysis, "mesh": api::mesh(&self.loaded),
            "provenance": {"source_sha256": self.loaded.binding.source_sha256, "face_entities": self.loaded.definition.faces, "part": 0}})
    }

    /// Write and verify the answered PMI, with the answers stored. `project` is the editor's
    /// project already in base64 (stored as `draftwright editor project`); `payload` is any
    /// application's `{name, value}`, stored as base64 JSON under `name` (`carried`). Either
    /// also records the original part, which [`Session::original`] gives back. Without either,
    /// the written file carries no project, payload or original, even if the opened one did.
    pub fn write_pmi(
        &self,
        answers: &Map<String, Value>,
        project: Option<&str>,
        payload: Option<&Value>,
    ) -> Result<Value, String> {
        input::answers(&self.analysis, answers).map_err(|e| e.to_string())?;
        let intent = api::preview(&self.analysis, answers).map_err(|error| error.to_string())?;
        let extra = carried_attributes(&self.loaded.bytes, project, payload)?;
        let (bytes, report) =
            specify_core::writer::write_bytes_with(&self.loaded, &intent, Some(answers), &extra)
                .map_err(|error| error.to_string())?;
        Ok(
            json!({"bytes": bytes, "source_sha256": self.loaded.binding.source_sha256, "report": report.to_dict()}),
        )
    }

    /// The face evidence to keep with answers held outside the STEP, for
    /// [`Session::rematch_answers`]: `{schema, face_count, faces}` (`resume::evidence`).
    pub fn evidence(&self) -> Result<Value, String> {
        resume::evidence(&self.loaded).map_err(|error| error.to_string())
    }

    /// Answers held outside the STEP matched to this part's features (`resume::rematch_answers`):
    /// `{answers, unresolved?, migrated?, unverified?, automatic?}`. Does not change the session.
    pub fn rematch_answers(
        &self,
        answers: &Map<String, Value>,
        evidence: Option<&Value>,
        automatic: Option<&[String]>,
    ) -> Result<Value, String> {
        let features: Vec<Feature> = serde_json::from_value(self.analysis["features"].clone())
            .map_err(|error| error.to_string())?;
        let resumed =
            resume::rematch_answers(&self.loaded, &features, answers, evidence, automatic)
                .map_err(|error| error.to_string())?;
        serde_json::to_value(resumed).map_err(|error| error.to_string())
    }

    /// The payload `name` the opened STEP carries, if any (`carried::payload`).
    pub fn payload(&self, name: &str) -> Option<Value> {
        carried::payload(&self.loaded.bytes, name)
    }

    /// The part the opened STEP was first written from, if it records it and the bytes
    /// verify (`carried::original`).
    pub fn original(&self) -> Option<Vec<u8>> {
        carried::original(&self.loaded.bytes)
    }

    /// Return ordinary editable answers, preserving supplied values and leaving questions
    /// without defaults open. This does not mutate the session or write PMI.
    pub fn accept_defaults(
        &self,
        answers: &Map<String, Value>,
    ) -> Result<Map<String, Value>, String> {
        crate::accept_defaults(&self.analysis, answers)
    }

    /// The review settled to the default path (`specify_core::tolerancing::settle`), with the
    /// label anchors [`Session::review`] adds: `{review, state}`.
    pub fn settle(&self, state: &tolerancing::State) -> Result<Value, String> {
        input::answers(&self.analysis, &state.answers).map_err(|e| e.to_string())?;
        let (review, settled) =
            tolerancing::settle(&self.analysis, state).map_err(|error| error.to_string())?;
        let review = self.with_anchors(review);
        Ok(json!({"review": review, "state": settled}))
    }

    pub fn review(&self, answers: &Map<String, Value>) -> Result<Value, String> {
        input::answers(&self.analysis, answers).map_err(|e| e.to_string())?;
        let review = api::review(&self.analysis, answers).map_err(|error| error.to_string())?;
        Ok(self.with_anchors(review))
    }

    fn with_anchors(&self, mut review: Value) -> Value {
        // JSON reads return Null on a missing key; api::review always constructs an object intent.
        let groups = review["intent"]["datums"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(
                review["intent"]["requirements"]
                    .as_array()
                    .into_iter()
                    .flatten(),
            )
            .filter_map(|item| item["faces"].as_array())
            .map(|faces| {
                faces
                    .iter()
                    .filter_map(|face| face.as_u64().map(|face| face as usize))
                    .collect()
            })
            .collect::<Vec<Vec<usize>>>();
        review["intent"]["anchors"] = json!(api::anchors(&self.loaded, &groups));
        review
    }
}

/// The editor's base64 `project`, or an application's `{name, value}` `payload`, as the
/// attributes the writer adds; at most one of them.
fn carried_attributes(
    source: &[u8],
    project: Option<&str>,
    payload: Option<&Value>,
) -> Result<Vec<haecceity::pmi::AttributeSet>, String> {
    const PROJECT: &str = "draftwright editor project";
    if let Some(text) = project {
        input::size("Project", text.len(), input::JSON_LIMIT).map_err(|e| e.to_string())?;
    }
    match (project, payload) {
        (Some(_), Some(_)) => Err("give a project or a payload, not both".into()),
        (Some(text), None) => carried::attributes_encoded(source, PROJECT, text)
            .map_err(|_| "the project to store is not base64".to_string()),
        (None, Some(payload)) => {
            let (Some(name), Some(value)) = (payload["name"].as_str(), payload.get("value")) else {
                return Err("Invalid payload: expected {name, value}".into());
            };
            input::size("Payload name", name.len(), input::TEXT_LIMIT)
                .map_err(|e| e.to_string())?;
            carried::attributes(source, name, value).map_err(|e| e.to_string())
        }
        (None, None) => Ok(vec![]),
    }
}
