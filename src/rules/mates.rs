//! A suggestion from the assembly (`mates.suggestion`): a pure function of the analysis's
//! `mates`, so it lives with the rules; finding mates in the geometry is `crate::mates`.

use serde_json::{Map, Value};

use super::{Error, Result};

/// The suggestion every one of `faces` has (`analysis["mates"]` is keyed by face index as a
/// string), if they all have one with the same value; None otherwise, or without mates.
pub fn suggestion<'m>(
    found: Option<&'m Map<String, Value>>,
    faces: &[i64],
) -> Result<Option<&'m Map<String, Value>>> {
    let Some(found) = found.filter(|m| !m.is_empty()) else {
        return Ok(None);
    };
    if faces.is_empty() {
        return Ok(None);
    }
    // Every face must have one before any is read, as Python's list comprehension does.
    let mut each = Vec::with_capacity(faces.len());
    for i in faces {
        match found.get(&i.to_string()).filter(|v| !v.is_null()) {
            Some(s) => each.push((i, s)),
            None => return Ok(None),
        }
    }
    let mut first: Option<(&Map<String, Value>, &Value)> = None;
    let mut same = true;
    for (i, s) in each {
        let Some(s) = s.as_object() else {
            return Err(Error::Invalid(format!(
                "the mate suggestion for face {i} is not an object"
            )));
        };
        let value = s.get("value").ok_or_else(|| {
            Error::Invalid(format!("the mate suggestion for face {i} has no 'value'"))
        })?;
        match first {
            None => first = Some((s, value)),
            Some((_, v)) if v != value => same = false,
            Some(_) => {}
        }
    }
    Ok(first.filter(|_| same).map(|(each, _)| each))
}
