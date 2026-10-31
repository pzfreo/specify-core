//! Exact authored names, shared by saved-answer resume and revision diff.
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub(crate) enum NameMatch {
    To(usize),
    Missing(String),
    Ambiguous(String),
}

impl NameMatch {
    pub(crate) fn result(&self) -> Result<usize, String> {
        match self {
            Self::To(i) => Ok(*i),
            Self::Missing(reason) | Self::Ambiguous(reason) => Err(reason.clone()),
        }
    }
}

/// Entries only for stored names. An absent entry permits the caller's existing geometric
/// path. A failed named entry is authoritative: it must never fall back to geometry.
pub(crate) fn match_names(
    old: &[Option<String>],
    new: &[Option<String>],
) -> BTreeMap<usize, NameMatch> {
    let index = |names: &[Option<String>]| {
        let mut out: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, name) in names.iter().enumerate() {
            if let Some(name) = name {
                out.entry(name.clone()).or_default().push(i);
            }
        }
        out
    };
    let (before, after) = (index(old), index(new));
    old.iter()
        .enumerate()
        .filter_map(|(i, name)| {
            let name = name.as_ref()?;
            let binding = if before[name].len() > 1 {
                NameMatch::Ambiguous(format!(
                    "ambiguous face name {name:?}: used on several faces in the saved revision"
                ))
            } else {
                match after.get(name).map(Vec::as_slice) {
                    Some([one]) => NameMatch::To(*one),
                    None => NameMatch::Missing(format!("face name {name:?} not in this revision")),
                    _ => NameMatch::Ambiguous(format!(
                        "ambiguous face name {name:?}: used on several faces in this revision"
                    )),
                }
            };
            Some((i, binding))
        })
        .collect()
}

/// Explicit face lists in datums and added requirements, including a distance's reference.
/// Keep all other fields and answer values exactly as supplied.
pub(crate) fn map_value(
    id: &str,
    answer: &serde_json::Value,
    mut map: impl FnMut(usize) -> Result<usize, String>,
) -> Result<serde_json::Value, String> {
    use serde_json::Value;
    let mut list = |value: &Value| -> Result<Value, String> {
        let faces = value.as_array().ok_or("saved faces must be an array")?;
        faces
            .iter()
            .map(|v| {
                let i = v
                    .as_u64()
                    .and_then(|i| usize::try_from(i).ok())
                    .ok_or("saved face must be an index")?;
                map(i).map(Value::from)
            })
            .collect()
    };
    let mut out = answer.clone();
    if id.starts_with("datum.") && answer.is_array() {
        out = list(answer)?;
    } else if id.starts_with("extra.")
        && let Some(object) = out.as_object_mut()
    {
        for key in ["faces", "reference"] {
            if let Some(value) = object.get_mut(key) {
                *value = list(value)?;
            }
        }
    }
    Ok(out)
}
