//! Checks shared by the JSON bindings and the in-memory session, before cloning or rules.
use serde_json::{Map, Value};
use specify_core::{api, choices, tolerancing};
use std::fmt;

pub const JSON_LIMIT: usize = 8 * 1024 * 1024;
pub const STEP_LIMIT: usize = 64 * 1024 * 1024;
pub const TEXT_LIMIT: usize = 64 * 1024;

#[derive(Debug)]
pub enum Error {
    TooLarge { field: &'static str, limit: usize },
    Answer { id: String, why: String },
    State(String),
    Rules(api::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { field, limit } => write!(f, "{field} exceeds the {limit} byte limit"),
            Self::Answer { id, why } => write!(f, "Invalid answers: {id}: {why}"),
            Self::State(why) => write!(f, "Invalid state: {why}"),
            Self::Rules(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for Error {}

pub fn size(field: &'static str, length: usize, limit: usize) -> Result<(), Error> {
    if length > limit {
        Err(Error::TooLarge { field, limit })
    } else {
        Ok(())
    }
}
fn bad(id: &str, why: impl Into<String>) -> Error {
    Error::Answer {
        id: id.into(),
        why: why.into(),
    }
}

/// Shape checks also apply to editor calls that have no analysis with which to resolve ids.
pub fn answers_shape(answers: &Map<String, Value>) -> Result<(), Error> {
    for (id, value) in answers {
        let invalid = |why| bad(id, why);
        if let Some(name) = id.strip_prefix("part.") {
            if name == "notes" {
                if !value
                    .as_array()
                    .is_some_and(|v| v.iter().all(Value::is_string))
                {
                    return Err(invalid("an answer must be a list of text notes"));
                }
            } else if [
                "material",
                "general_tolerance",
                "locating",
                "surface_finish",
                "coating",
                "heat_treatment",
                "edges",
            ]
            .contains(&name)
            {
                if !value.is_string() {
                    return Err(invalid("an answer must be text"));
                }
            } else {
                return Err(invalid("unknown answer ID"));
            }
        } else if let Some(suffix) = id.strip_prefix("datum.") {
            let (letter, control) = suffix.split_once('.').unwrap_or((suffix, ""));
            if !matches!(letter, "A" | "B" | "C" | "D" | "E" | "F" | "G" | "H")
                || !matches!(control, "" | "control" | "flatness")
            {
                return Err(invalid("unknown answer ID"));
            }
            if control.is_empty() {
                if !value
                    .as_array()
                    .is_some_and(|v| v.iter().all(|x| x.as_u64().is_some()))
                {
                    return Err(invalid("a datum answer must be a list of face numbers"));
                }
            } else if !value.is_string() {
                return Err(invalid("an answer must be text"));
            }
        } else if let Some((kind, feature)) = id.split_once(':') {
            if ![
                "hole.function",
                "hole.frame",
                "hole.position",
                "hole.location",
                "diameter.fit",
                "diameter.runout",
            ]
            .contains(&kind)
                || feature.is_empty()
            {
                return Err(invalid("unknown answer ID"));
            }
            if !value.is_string() {
                return Err(invalid("an answer must be text"));
            }
        } else if id.starts_with("extra.") && id.len() > "extra.".len() {
            if !value.is_object() {
                return Err(invalid("an added requirement must be an object"));
            }
        } else {
            return Err(invalid("unknown answer ID"));
        }
    }
    Ok(())
}

pub fn answers(analysis: &Value, answers: &Map<String, Value>) -> Result<(), Error> {
    answers_shape(answers)?;
    // Resolve even dormant feature answers: edits may retain a question hidden by a mode.
    for (id, value) in answers {
        if let Some((_, feature)) = id.split_once(':') {
            let found = analysis
                .get("features")
                .and_then(Value::as_array)
                .is_some_and(|fs| {
                    fs.iter()
                        .any(|f| f.get("id").and_then(Value::as_str) == Some(feature))
                });
            if !found {
                return Err(bad(id, format!("unknown feature {feature}")));
            }
        }
        if id.starts_with("datum.")
            && let Some(ids) = value.as_array()
        {
            for face in ids {
                let found = analysis
                    .get("faces")
                    .and_then(Value::as_array)
                    .is_some_and(|fs| fs.iter().any(|f| f.get("id") == Some(face)));
                if !found {
                    return Err(bad(id, format!("unknown face {face}")));
                }
            }
        }
    }
    let questions = api::questions(analysis, answers).map_err(Error::Rules)?;
    for question in questions {
        if question.kind == "choice"
            && let Some(value) = answers.get(&question.id)
        {
            choices::check(
                &question.id,
                value,
                &question.options,
                question.diameter,
                question.depth,
            )
            .map_err(|e| bad(&question.id, e.0))?;
        }
    }
    Ok(())
}

pub fn state(state: &tolerancing::State) -> Result<(), Error> {
    answers_shape(&state.answers)?;
    if let Some(auto) = &state.auto {
        let mut seen = std::collections::HashSet::new();
        for id in auto {
            if !state.answers.contains_key(id) || !seen.insert(id) {
                return Err(Error::State(format!(
                    "automatic answer {id} is missing or repeated"
                )));
            }
        }
    }
    Ok(())
}
