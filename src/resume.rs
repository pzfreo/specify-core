//! Answers stored in a written STEP and resumed from it (port of `resume.py`).
//!
//! Writing a part, specify-core also stores the answers it was given in the file, so that
//! opening the file again resumes them: the analysis is of the geometry alone and carries the
//! answers back, and writing it again replaces its PMI rather than adding to it. The payload is
//! Python's: `{schema, answers, part, face_count, faces}` as compact JSON, base64-encoded (a
//! Part 21 string then needs no escapes), `faces` being a [`fingerprint`] of the part's faces
//! that is checked before any answer is trusted. Schema 2 is written (zeros unsigned in the
//! fingerprint, question Q14); schema 1 is still read.
//!
//! **Where it is stored.** Python appends its own text: `PROPERTY_DEFINITION('pmi-assist
//! answers','schema 1',#pd)` with a representation holding one
//! `DESCRIPTIVE_REPRESENTATION_ITEM('pmi-assist answers','<base64>')`. This port writes the
//! same item through haecceity, as a CAx-IF user defined attribute on the part
//! ([`attribute_set`]: `AttributeSet` 'pmi-assist answers' with that one string item), within
//! the same `pmi::write` edit as the PMI: a standard construct that haecceity's reader consumes
//! as the part's, so a replace removes it with the PMI it describes. Both forms carry the same
//! `DESCRIPTIVE_REPRESENTATION_ITEM('pmi-assist answers','<base64>')`, which is all Python's
//! reader looks for, so each engine resumes the other's files; [`stored`] reads both.
//!
//! In an assembly each part's answers are stored on that part and say which part they are for;
//! a part with none stored has not been written by specify-core.

use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use haecceity::p21::{Attribute, Document, RawEntity};
use haecceity::pmi::{AttributeSet, AttributeValue};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::features::{FeatureError, describe_faces};
use crate::part::LoadedPart;
use crate::pyfmt::round_half_even;
use crate::pyjson;

/// The payload's schema as written: 2, whose fingerprint writes zeros unsigned
/// ([`fingerprint`]). A payload of schema 1 (Python's, and this port's before schema 2) is
/// still read and resumed when its own fingerprint matches; one of any other schema is ignored,
/// as Python ignores it.
pub const SCHEMA: i64 = 2;

/// The schemas read: 1 (fingerprint with zeros' signs kept) and 2 (zeros unsigned).
pub const SCHEMAS: [i64; 2] = [1, SCHEMA];

/// The name of the stored item (and of the attribute set holding it): Python's, kept from
/// before specify-core was renamed so that older files still resume.
pub const NAME: &str = "pmi-assist answers";

/// The faces in order, by kind and centroid rounded to 3 decimals: what answers refer to, as
/// schema 2 hashes them ([`fingerprint_of`]). The fingerprint written.
pub fn fingerprint(loaded: &LoadedPart) -> Result<String, FeatureError> {
    Ok(fingerprint_of(&rounded_faces(loaded)?, SCHEMA))
}

/// Schema 1's fingerprint of the faces (Python's `resume.fingerprint` before schema 2): the
/// one a schema 1 payload is checked against.
pub fn fingerprint_v1(loaded: &LoadedPart) -> Result<String, FeatureError> {
    Ok(fingerprint_of(&rounded_faces(loaded)?, 1))
}

/// The faces as `[kind, [x, y, z]]`, each centroid component rounded to 3 decimals (Python's
/// `round(c, 3)`, half to even), in face order.
fn rounded_faces(loaded: &LoadedPart) -> Result<Value, FeatureError> {
    Ok(Value::Array(
        describe_faces(loaded)?
            .iter()
            .map(|f| {
                let centroid: Vec<Value> = f
                    .centroid
                    .iter()
                    .map(|&c| json!(round_half_even(c, 3)))
                    .collect();
                json!([f.kind, centroid])
            })
            .collect(),
    ))
}

/// The fingerprint, of `schema`, of `faces` given as `[kind, [x, y, z]]` with each component
/// already rounded to 3 decimals: the lowercase hex sha256 of the UTF-8 text Python's
/// `json.dumps(faces)` writes (`pyjson::dumps`: `", "` and `": "` separators, floats by
/// `float.__repr__`). Schema 1 hashes the components as they are, so a zero keeps its sign
/// (`-0.0`), which is integration noise; schema 2 first writes every zero unsigned (`0.0`), so
/// either engine's fingerprint of the same faces is the same (docs/design.md, "The
/// fingerprint").
pub fn fingerprint_of(faces: &Value, schema: i64) -> String {
    let unsigned = |v: &Value| match v.as_f64() {
        // A float pattern compares by `==`, so -0.0 matches too.
        Some(0.0) => json!(0.0),
        _ => v.clone(),
    };
    let faces: Value = if schema >= 2 {
        faces
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| {
                // f is a serde_json::Value: missing or ill-typed reads return Null rather than panicking.
                let centroid: Vec<Value> = f[1]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(unsigned)
                    .collect();
                json!([f[0], centroid])
            })
            .collect()
    } else {
        faces.clone()
    };
    Sha256::digest(pyjson::dumps(&faces).as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The payload Python's `embed` stores for `answers` given for `loaded`, in its key order.
pub fn payload(loaded: &LoadedPart, answers: &Map<String, Value>) -> Result<Value, FeatureError> {
    Ok(json!({
        "schema": SCHEMA,
        "answers": answers,
        "part": loaded.binding.part,
        "face_count": loaded.binding.face_count,
        "faces": fingerprint(loaded)?,
    }))
}

/// `payload` as stored: compact JSON (`separators=(",", ":")`), base64.
pub fn encode(payload: &Value) -> String {
    STANDARD.encode(pyjson::dumps_compact(payload))
}

/// The user defined attribute that stores `payload` on its part, written by the writer in the
/// same edit as the part's PMI.
pub fn attribute_set(payload: &Value) -> AttributeSet {
    AttributeSet {
        description: None,
        general_property: None,
        name: NAME.into(),
        on: None,
        items: vec![(NAME.into(), AttributeValue::Text(encode(payload)))],
    }
}

/// Whether `set` is stored answers (this port's form), not PMI.
pub fn is_answers(set: &AttributeSet) -> bool {
    set.name == NAME
        && set.on.is_none()
        && matches!(set.items.as_slice(), [(name, AttributeValue::Text(_))] if name == NAME)
}

/// Every part's stored answers in the STEP text `bytes`, by part (Python's `stored`): each
/// `DESCRIPTIVE_REPRESENTATION_ITEM('pmi-assist answers', ...)`, in file order, whose string
/// is base64 of a JSON object of one of [`SCHEMAS`] with an `answers` object. The first for a
/// part is kept; answers saved before parts were told apart are part 0's. Anything else (not
/// base64, not JSON, another schema, a part that is not an integer) is passed over, as Python
/// passes it over. A file that does not parse as Part 21 stores nothing.
pub fn stored(bytes: &[u8]) -> BTreeMap<usize, Map<String, Value>> {
    match Document::parse(bytes.to_vec()) {
        Ok(doc) => stored_in(&doc),
        Err(_) => BTreeMap::new(),
    }
}

/// [`stored`] of the file already parsed as `doc`.
pub fn stored_in(doc: &Document) -> BTreeMap<usize, Map<String, Value>> {
    let mut out = BTreeMap::new();
    for id in items_in_file_order(doc) {
        let Some(RawEntity::Simple { attributes, .. }) = doc.get(id) else {
            continue;
        };
        let Some(Attribute::String(text)) = attributes.get(1) else {
            continue;
        };
        let Some(payload) = decode(text) else {
            continue;
        };
        let part = match payload.get("part") {
            None => Some(0),
            // Python's isinstance(part, int): an integer (a bool is one too, as True is 1).
            Some(Value::Number(n)) => n.as_u64().map(|k| k as usize),
            Some(Value::Bool(b)) => Some(usize::from(*b)),
            Some(_) => None,
        };
        if let Some(part) = part {
            out.entry(part).or_insert(payload);
        }
    }
    out
}

/// The `DESCRIPTIVE_REPRESENTATION_ITEM`s named [`NAME`], in the order they appear in the file.
fn items_in_file_order(doc: &Document) -> Vec<u64> {
    let mut found: Vec<(usize, u64)> = doc
        .ids()
        .filter(|&id| match doc.get(id) {
            Some(RawEntity::Simple {
                name, attributes, ..
            }) => {
                name == "DESCRIPTIVE_REPRESENTATION_ITEM"
                    && matches!(attributes.first(), Some(Attribute::String(s)) if s == NAME)
            }
            _ => false,
        })
        .map(|id| (doc.span(id).map_or(usize::MAX, |s| s.start), id))
        .collect();
    found.sort_unstable();
    found.into_iter().map(|(_, id)| id).collect()
}

/// A stored string's payload, if it is one: base64 (the characters Python's pattern allows,
/// strictly decoded) of a JSON object of one of [`SCHEMAS`] whose `answers` is an object.
fn decode(text: &str) -> Option<Map<String, Value>> {
    if !text
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
    {
        return None;
    }
    let raw = STANDARD.decode(text).ok()?;
    let Value::Object(payload) = serde_json::from_slice::<Value>(&raw).ok()? else {
        return None;
    };
    (schema_of(&payload).is_some() && payload.get("answers").is_some_and(Value::is_object))
        .then_some(payload)
}

/// Which of [`SCHEMAS`] a payload is, compared as Python's `==` compares (so `1.0`, and `true`
/// as 1, are schema 1); `None` for any other.
fn schema_of(payload: &Map<String, Value>) -> Option<i64> {
    let n = match payload.get("schema") {
        Some(Value::Number(n)) => n.as_f64()?,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => return None,
    };
    SCHEMAS.into_iter().find(|&s| s as f64 == n)
}

/// The stored answers for this part, if the file holds them for it and its faces are the ones
/// they were given for (same face count, same fingerprint of the payload's schema: [`fingerprint`]
/// for 2, [`fingerprint_v1`] for 1); `None` otherwise. An error when
/// the part's faces cannot be described, so the fingerprint cannot be checked.
pub fn answers_for(loaded: &LoadedPart) -> Result<Option<Map<String, Value>>, FeatureError> {
    answers_in(loaded, stored(&loaded.bytes))
}

/// [`answers_for`] from the file already parsed: `doc` is `loaded.bytes` as a Part 21 document,
/// or `None` when they do not parse (nothing is stored then).
pub fn answers_for_parsed(
    loaded: &LoadedPart,
    doc: Option<&Document>,
) -> Result<Option<Map<String, Value>>, FeatureError> {
    answers_in(loaded, doc.map(stored_in).unwrap_or_default())
}

/// The answers for `loaded` among `stored`, the file's stored answers by part.
fn answers_in(
    loaded: &LoadedPart,
    mut stored: BTreeMap<usize, Map<String, Value>>,
) -> Result<Option<Map<String, Value>>, FeatureError> {
    let Some(payload) = stored.remove(&loaded.binding.part) else {
        return Ok(None);
    };
    if payload.get("face_count") != Some(&json!(loaded.binding.face_count)) {
        return Ok(None);
    }
    // A payload of either schema, checked against that schema's fingerprint.
    let schema = schema_of(&payload).unwrap_or(SCHEMA);
    let faces = fingerprint_of(&rounded_faces(loaded)?, schema);
    if payload.get("faces") != Some(&Value::String(faces)) {
        return Ok(None);
    }
    Ok(payload.get("answers").and_then(Value::as_object).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payload_is_compact_json_in_base64() {
        let p = json!({"schema": 1, "answers": {"part.material": "steel"}, "part": 0});
        let text = encode(&p);
        assert_eq!(
            String::from_utf8(STANDARD.decode(&text).unwrap()).unwrap(),
            r#"{"schema":1,"answers":{"part.material":"steel"},"part":0}"#
        );
        assert!(decode(&text).is_some());
    }

    #[test]
    fn schema_two_writes_zeros_unsigned_and_schema_one_keeps_their_signs() {
        let signed = json!([
            ["plane", [-50.0, -0.0, 0.0]],
            ["cylinder", [1e16, 0.5, -1.25]]
        ]);
        let unsigned = json!([
            ["plane", [-50.0, 0.0, 0.0]],
            ["cylinder", [1e16, 0.5, -1.25]]
        ]);
        let sha = |t: &str| -> String {
            Sha256::digest(t.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect()
        };
        assert_eq!(
            fingerprint_of(&signed, 2),
            sha(r#"[["plane", [-50.0, 0.0, 0.0]], ["cylinder", [1e+16, 0.5, -1.25]]]"#)
        );
        assert_eq!(fingerprint_of(&signed, 2), fingerprint_of(&unsigned, 2));
        assert_eq!(
            fingerprint_of(&signed, 1),
            sha(r#"[["plane", [-50.0, -0.0, 0.0]], ["cylinder", [1e+16, 0.5, -1.25]]]"#)
        );
        assert_ne!(fingerprint_of(&signed, 1), fingerprint_of(&unsigned, 1));
    }

    #[test]
    fn what_is_not_a_payload_is_passed_over() {
        assert!(decode("not base64!").is_none());
        assert!(decode(&STANDARD.encode(r#"{"schema":3,"answers":{}}"#)).is_none());
        assert!(decode(&STANDARD.encode(r#"{"schema":2,"answers":{}}"#)).is_some());
        assert!(decode(&STANDARD.encode(r#"{"schema":2.0,"answers":{}}"#)).is_some());
        assert!(decode(&STANDARD.encode(r#"{"schema":"2","answers":{}}"#)).is_none());
        assert!(decode(&STANDARD.encode(r#"{"schema":1,"answers":[]}"#)).is_none());
        assert!(decode(&STANDARD.encode(r#"[1]"#)).is_none());
        assert!(decode(&STANDARD.encode(r#"{"schema":1.0,"answers":{}}"#)).is_some());
    }
}
