//! The feature table (Specify 0.3.0, ADR P11): for each feature of the part and each kind of
//! requirement it can need, whether the file *states* it (a requirement of that kind on the
//! feature's faces, whoever wrote it), it is a *gap* (with Specify's suggestion), or it was
//! *decided: none*.
//!
//! The table is read from the file alone. A requirement is matched to a feature and kind by
//! the faces it sits on and its kind; its authorship marker plays no part. PMI on no
//! recognised feature and kind is listed as `file_pmi`. The only things stored beside the
//! PMI are the decided-none answers (and the characteristic-number history, which is read
//! elsewhere): nothing here duplicates what the file states.
//!
//! Each row's id is the question id of its feature and kind (`hole.position:holes:12`: the
//! position of hole 12). The rows are the questions the part has with its PMI set aside, so
//! a stated feature-and-kind keeps its row.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Questions that are not a requirement of a feature: how holes are located is a choice
/// that shapes other requirements, and writes no PMI of its own.
const NOT_REQUIREMENTS: [&str; 1] = ["part.locating"];

/// Questions that are never counted open (as [`crate::rules::open_questions`]).
const NEVER_OPEN: [&str; 2] = ["hole.full_thread", crate::process::QUESTION];

/// The state of one feature-and-kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// The file has a requirement of this kind on the feature's faces.
    Stated,
    /// Nothing is there yet.
    Gap,
    /// Someone decided nothing is needed (stored in the file, as it leaves no PMI).
    DecidedNone,
}

/// One requirement in the file: a PMI item, or a part setting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Requirement {
    /// Its characteristic number, `#N`, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    /// `dimension`, `geometric_tolerance`, `datum`, `note` or `setting`.
    pub kind: String,
    /// The PMI type (`Position`, `Size_Diameter`), the datum's letter, the note's kind, or
    /// the setting's key.
    pub r#type: String,
    /// The faces it sits on (both sides of a location).
    pub faces: Vec<usize>,
    /// Its value in short notation.
    pub text: String,
}

/// One feature-and-kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// The feature-and-kind's id, the question's (`hole.position:holes:12`).
    pub id: String,
    /// The kind (`hole.position`).
    pub kind: String,
    /// The feature (`holes:12`, `part`, `datum.A`).
    pub feature: String,
    /// The feature's faces.
    pub faces: Vec<usize>,
    pub state: State,
    /// Stated: the file's value, its requirements' notation joined. Decided none: the
    /// stored answer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    /// Stated: the requirements that state it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<Requirement>,
    /// Gap: Specify's suggestion (`null` when it has none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<Value>,
    /// A gap to ask about: a gap whose suggestion is "none" isn't open (leaving it and
    /// accepting "none" give the same file).
    pub open: bool,
}

/// The table, the file's PMI on no recognised feature-and-kind, and the decided-none list.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FeatureTable {
    pub rows: Vec<Row>,
    pub file_pmi: Vec<Requirement>,
    /// The feature-and-kinds decided none, as read back from the file.
    pub decided_none: Vec<String>,
}

impl FeatureTable {
    /// The row of `id`.
    pub fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.id == id)
    }
}

/// Whether an answer or suggestion means "nothing is needed": absent, empty, or a choice
/// that reads "none" (`none`, `None`, `None (as machined)`).
pub fn is_none_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => {
            let s = s.trim();
            s.is_empty() || s.to_ascii_lowercase().starts_with("none")
        }
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

/// Whether an answer leaves no PMI of its own behind: "none", or a hole left to the general
/// tolerance (`general`), which needs no requirement of its own. Such an answer is stored.
pub fn leaves_nothing(value: &Value) -> bool {
    is_none_value(value) || value == "general"
}

/// Whether the question `id` is a requirement of a feature (a row of the table) rather than
/// a choice that writes no PMI of its own, or bookkeeping.
pub fn is_requirement(id: &str) -> bool {
    !(id.starts_with('_')
        || id.starts_with("bom.")
        || id.starts_with(crate::extras::PREFIX)
        || NOT_REQUIREMENTS.contains(&id))
}

/// The answers worth storing in the file (ADR P11): those that leave no PMI behind, being
/// decided none or a choice that is not a requirement. An answer that writes PMI is not
/// stored: the file states it.
pub fn storable(answers: &Map<String, Value>) -> Map<String, Value> {
    answers
        .iter()
        .filter(|(id, value)| !is_requirement(id) || leaves_nothing(value))
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect()
}

/// The kind and feature of a question id: `hole.position:holes:12` → (`hole.position`,
/// `holes:12`); `datum.A.flatness` → (`datum.A.flatness`, `datum.A`); `part.coating` →
/// (`part.coating`, `part`).
fn kind_and_feature(id: &str) -> (String, String) {
    if let Some((kind, feature)) = id.split_once(':') {
        return (kind.to_string(), feature.to_string());
    }
    if let Some(rest) = id.strip_prefix("datum.") {
        let letter = rest.split('.').next().unwrap_or(rest);
        return (id.to_string(), format!("datum.{letter}"));
    }
    let feature = id.split('.').next().unwrap_or(id);
    (id.to_string(), feature.to_string())
}

fn faces_of(value: &Value, key: &str) -> Vec<usize> {
    value[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .map(|f| f as usize)
        .collect()
}

fn number(v: f64) -> String {
    crate::pyfmt::repr(v)
}

/// Short notation of an `existing` entry.
fn text_of(item: &Value) -> String {
    let kind = item["kind"].as_str().unwrap_or_default();
    let r#type = item["type"].as_str().unwrap_or_default();
    let value = item["value"].as_f64();
    match kind {
        "datum" => format!("datum {type}"),
        "geometric_tolerance" => {
            let mut out = format!("{type} {}", value.map(number).unwrap_or_default());
            let datums: Vec<&str> = item["datums"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            if !datums.is_empty() {
                out.push_str(&format!(" | {}", datums.join(" | ")));
            }
            out
        }
        "dimension" => {
            let mut out = format!("{type} {}", value.map(number).unwrap_or_default());
            if item["basic"] == Value::Bool(true) {
                out.push_str(" basic");
            } else if let Some([lo, hi]) = item["limits"]
                .as_array()
                .map(|l| l.iter().filter_map(Value::as_f64).collect::<Vec<_>>())
                .as_deref()
            {
                out.push_str(&format!(" [{}, {}]", number(*lo), number(*hi)));
            } else if let (Some(u), Some(l)) = (item["upper"].as_f64(), item["lower"].as_f64()) {
                out.push_str(&format!(" +{} {}", number(u), number(l)));
            }
            out
        }
        _ => match value {
            Some(v) => format!("{type} {}", number(v)),
            None => r#type.to_string(),
        },
    }
    .trim()
    .to_string()
}

fn requirement(item: &Value) -> Requirement {
    let mut faces = faces_of(item, "faces");
    faces.extend(faces_of(item, "faces2"));
    Requirement {
        number: item["number"].as_u64().map(|n| format!("#{n}")),
        kind: item["kind"].as_str().unwrap_or_default().to_string(),
        r#type: item["type"].as_str().unwrap_or_default().to_string(),
        faces,
        text: text_of(item),
    }
}

/// Whether the `existing` entry `item` is a requirement of kind `kind` on `faces`.
fn states(kind: &str, faces: &BTreeSet<usize>, item: &Value) -> bool {
    let item_kind = item["kind"].as_str().unwrap_or_default();
    let r#type = item["type"].as_str().unwrap_or_default();
    let on = |key: &str| faces_of(item, key).iter().any(|f| faces.contains(f));
    let on_either = on("faces") || on("faces2");
    let family = kind.rsplit('.').next().unwrap_or(kind);
    if let Some(rest) = kind.strip_prefix("datum.") {
        let mut parts = rest.split('.');
        let letter = parts.next().unwrap_or_default();
        return match parts.next() {
            None => item_kind == "datum" && r#type == letter,
            Some("flatness") => {
                item_kind == "geometric_tolerance" && r#type == "Flatness" && on("faces")
            }
            Some(_) => {
                item_kind == "geometric_tolerance"
                    && !matches!(r#type, "Position" | "Flatness")
                    && on("faces")
            }
        };
    }
    match family {
        "position" | "frame" => {
            (item_kind == "geometric_tolerance" && r#type == "Position" && on("faces"))
                || (item_kind == "dimension" && r#type.starts_with("Location_") && on_either)
        }
        "location" => item_kind == "dimension" && r#type.starts_with("Location_") && on_either,
        "function" | "fit" | "size" => {
            (item_kind == "dimension" && r#type.starts_with("Size_") && on("faces"))
                || (item_kind == "note" && r#type.ends_with("thread") && on("faces"))
        }
        "full_thread" => item_kind == "note" && r#type.ends_with("thread") && on("faces"),
        "runout" => item_kind == "geometric_tolerance" && r#type.contains("Runout") && on("faces"),
        _ => item_kind != "datum" && on("faces"),
    }
}

/// The setting key a part question reads (`part.notes` → `notes`).
fn setting(id: &str) -> Option<&str> {
    id.strip_prefix("part.")
}

fn setting_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
            .collect::<Vec<_>>()
            .join("; "),
        other => other.to_string(),
    }
}

/// Whether a setting's value states nothing: absent, empty text or an empty list.
fn blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(s) => s.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

/// The file's value of a part question, if it states one.
fn stated_setting<'a>(settings: &'a Map<String, Value>, id: &str) -> Option<&'a Value> {
    settings.get(setting(id)?).filter(|v| !blank(v))
}

/// The feature table of `analysis` (its `existing` the file's whole PMI, whoever wrote it),
/// with `settings` the part's settings as the file states them (the material and general
/// tolerance too, for a part of an assembly), and `answers` the answers stored in the file
/// that leave no PMI behind ([`storable`]): decided none, and choices that are not
/// requirements.
pub fn build(
    analysis: &Value,
    settings: &Map<String, Value>,
    answers: &Map<String, Value>,
) -> Result<FeatureTable, crate::rules::Error> {
    let existing: Vec<&Value> = analysis["existing"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    // The rows: every feature-and-kind the part has, with the file's PMI set aside (a stated
    // feature is not asked, but keeps its row). The file's datums stand for their questions,
    // so the controls are on the faces the file gives them.
    let mut bare = analysis.clone();
    if let Some(object) = bare.as_object_mut() {
        object.insert("existing".into(), Value::Array(Vec::new()));
        object.insert("part".into(), Value::Object(Map::new()));
        object.remove("resume");
        object.remove("dimensioning_stated");
        object.remove("process");
    }
    let mut given: Map<String, Value> = answers
        .iter()
        .filter(|(id, _)| !id.starts_with('_'))
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect();
    for item in &existing {
        if item["kind"] == "datum"
            && let Some(letter) = item["type"].as_str()
        {
            let faces = faces_of(item, "faces");
            let id = format!("datum.{letter}");
            if !faces.is_empty() && !given.contains_key(&id) {
                given.insert(id, Value::from(faces));
            }
        }
    }
    let all = crate::rules::with_all_defaults(&bare, &given).unwrap_or_else(|_| given.clone());
    let questions = crate::rules::questions(&bare, &all)?;

    let mut claimed = vec![false; existing.len()];
    let mut claimed_settings = BTreeSet::new();
    let mut rows = Vec::new();
    let mut decided_none = Vec::new();
    for q in &questions {
        if !is_requirement(&q.id) {
            continue;
        }
        let (kind, feature) = kind_and_feature(&q.id);
        let faces: BTreeSet<usize> = q
            .faces
            .iter()
            .filter_map(|&f| usize::try_from(f).ok())
            .collect();
        let mut requirements = Vec::new();
        let mut value = None;
        if q.id.starts_with("part.") {
            if let Some(v) = stated_setting(settings, &q.id) {
                value = Some(Value::String(setting_text(v)));
                if let Some(key) = setting(&q.id) {
                    claimed_settings.insert(key.to_string());
                }
            }
        } else {
            for (i, item) in existing.iter().enumerate() {
                if states(&kind, &faces, item) {
                    claimed[i] = true;
                    requirements.push(requirement(item));
                }
            }
            if !requirements.is_empty() {
                value = Some(Value::String(
                    requirements
                        .iter()
                        .map(|r| r.text.as_str())
                        .collect::<Vec<_>>()
                        .join("; "),
                ));
            }
        }
        let never_open = NEVER_OPEN.contains(&kind.as_str());
        let row = if value.is_some() {
            Row {
                id: q.id.clone(),
                kind,
                feature,
                faces: faces.into_iter().collect(),
                state: State::Stated,
                value,
                requirements,
                suggestion: None,
                open: false,
            }
        } else if let Some(stored) = answers.get(&q.id).filter(|v| leaves_nothing(v)) {
            decided_none.push(q.id.clone());
            Row {
                id: q.id.clone(),
                kind,
                feature,
                faces: faces.into_iter().collect(),
                state: State::DecidedNone,
                value: Some(stored.clone()),
                requirements,
                suggestion: None,
                open: false,
            }
        } else {
            let suggestion = q.default.clone().unwrap_or(Value::Null);
            // No suggestion at all (a material) is open; a suggestion of "none" is not.
            let open = !never_open && (q.default.is_none() || !is_none_value(&suggestion));
            Row {
                id: q.id.clone(),
                kind,
                feature,
                faces: faces.into_iter().collect(),
                state: State::Gap,
                value: None,
                requirements,
                suggestion: Some(suggestion),
                open,
            }
        };
        rows.push(row);
    }
    let mut file_pmi: Vec<Requirement> = existing
        .iter()
        .zip(&claimed)
        .filter(|(_, claimed)| !**claimed)
        .map(|(item, _)| requirement(item))
        .collect();
    for (key, value) in settings {
        if !claimed_settings.contains(key) && !blank(value) {
            file_pmi.push(Requirement {
                number: None,
                kind: "setting".into(),
                r#type: key.clone(),
                faces: Vec::new(),
                text: setting_text(value),
            });
        }
    }
    Ok(FeatureTable {
        rows,
        file_pmi,
        decided_none,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn none_values() {
        for v in [
            json!(null),
            json!("none"),
            json!("None (as machined)"),
            json!([]),
            json!(""),
        ] {
            assert!(is_none_value(&v), "{v}");
        }
        for v in [json!("0.05"), json!([2]), json!("general"), json!("Ra 3.2")] {
            assert!(!is_none_value(&v), "{v}");
        }
    }

    #[test]
    fn ids_split_into_kind_and_feature() {
        assert_eq!(
            kind_and_feature("hole.position:holes:12"),
            ("hole.position".into(), "holes:12".into())
        );
        assert_eq!(
            kind_and_feature("datum.A.flatness"),
            ("datum.A.flatness".into(), "datum.A".into())
        );
        assert_eq!(
            kind_and_feature("part.coating"),
            ("part.coating".into(), "part".into())
        );
    }

    #[test]
    fn only_answers_that_leave_no_pmi_are_stored() {
        let answers = json!({
            "part.locating": "position tolerances",
            "part.coating": "None (as machined)",
            "datum.C": [],
            "datum.A": [2],
            "hole.position:holes:12": "0.1",
            "datum.A.flatness": "none",
        });
        let stored = storable(answers.as_object().unwrap());
        let keys: Vec<&str> = stored.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "part.locating",
                "part.coating",
                "datum.C",
                "datum.A.flatness"
            ]
        );
    }
}
