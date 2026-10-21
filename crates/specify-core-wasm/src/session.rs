use crate::input;
use haecceity::step;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use specify_core::{
    api,
    part::{Binding, LOADER_VERSION, LoadedPart},
    tolerancing,
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

    pub fn write_pmi(
        &self,
        answers: &Map<String, Value>,
        project: Option<&str>,
    ) -> Result<Value, String> {
        input::answers(&self.analysis, answers).map_err(|e| e.to_string())?;
        let intent = api::preview(&self.analysis, answers).map_err(|error| error.to_string())?;
        let extra = project_attribute(project)?;
        let (bytes, report) =
            specify_core::writer::write_bytes_with(&self.loaded, &intent, Some(answers), &extra)
                .map_err(|error| error.to_string())?;
        Ok(
            json!({"bytes": bytes, "source_sha256": self.loaded.binding.source_sha256, "report": report.to_dict()}),
        )
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

/// Match the editor's alphabet check and part-owned attribute exactly.
fn project_attribute(project: Option<&str>) -> Result<Vec<haecceity::pmi::AttributeSet>, String> {
    use haecceity::pmi::{AttributeSet, AttributeValue};
    const NAME: &str = "draftwright editor project";
    if let Some(text) = project {
        input::size("Project", text.len(), input::JSON_LIMIT).map_err(|e| e.to_string())?;
    }
    match project {
        Some(text)
            if text.is_empty()
                || !text
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)) =>
        {
            Err("the project to store is not base64".into())
        }
        Some(text) => Ok(vec![AttributeSet {
            name: NAME.into(),
            on: None,
            items: vec![(NAME.into(), AttributeValue::Text(text.into()))],
            description: None,
            general_property: None,
        }]),
        None => Ok(vec![]),
    }
}
