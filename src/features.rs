//! Faces described and features recognised, keyed by face index (port of `features.py`).
//!
//! A face is identified by its index in the part's numbering ([`crate::part`]), and a feature
//! by its family and the sorted indices of its faces (`holes:3`, `hole_patterns.grid:6.7.8`),
//! as in specify-core. Saved STEP answers are re-matched on resume (see `resume`).
//!
//! What differs from Python, by design (docs/design.md, "Features differ by design"):
//!
//! - Features come from quiddity-rust (`quiddity::correspondence::recognise`, framed
//!   records converted/labelled at this boundary), not Python quiddity's evidence view. A feature's faces are its
//!   *defining* faces (`Features::defining`), not its constituent faces (upstream need U2): a
//!   blind hole is its bore, without its floor, so ids for the same feature can differ between
//!   the engines. The families are quiddity-rust's: Python's cross-family reconciliation
//!   (`step_levels`, `bosses` dropped on turned parts) is not there.
//! - A pattern's member holes are found among the hole records by equality, the first not yet
//!   taken (as quiddity-rust's own `fingerprint::derived_faces`), because `HolePattern` holds
//!   copies of its members, not their indices (U2).
//! - Recognition refuses a part a caller rebuilt (`rebuilt`) with unresolved faces or edges
//!   ([`LoadedPart::degraded`]): its `Part` has lost haecceity's record of what did not
//!   resolve, so its solids would be taken as valid.
//! - Features are listed family by family in quiddity-rust's order (the order of `Features`'
//!   fields), patterns after the holes they are made of, hole sets last. Python lists its
//!   evidence view's order; both are deterministic and nothing depends on the order.

// Face indices below enumerate the loaded part; Session::open sets binding.face_count from that same list.
use std::collections::BTreeSet;
use std::fmt;

use haecceity::geom::{Surface, V3};
use quiddity::features::hole_patterns::HolePattern;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::part::LoadedPart;
use crate::pyfmt::round_half_even;

/// Quiddity's hole-pattern variants, by the short kind used in feature ids and `record.kind`
/// (Python's `HOLE_PATTERN_KINDS`, keyed by Python quiddity's record type names, which are
/// quiddity-rust's variant names).
pub const HOLE_PATTERN_KINDS: [(&str, &str); 4] = [
    ("BoltCircle", "bolt_circle"),
    ("LinearArray", "linear"),
    ("RectGrid", "grid"),
    ("RectangularHoleSet", "rectangle"),
];

/// One face of the part, with what a datum or picking question needs (Python's `Face`).
/// Serialises as `Face.to_dict`: absent fields are left out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub id: usize,
    /// `plane`, `cylinder`, `cone`, `torus` or `other`.
    pub kind: String,
    pub area: f64,
    /// The area centroid, rounded to 6 decimals.
    pub centroid: [f64; 3],
    /// A plane's outward normal (the surface normal, flipped for a reversed face), or a surface
    /// of revolution's axis direction; rounded to 9 decimals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<[f64; 3]>,
    /// A point on a surface of revolution's axis (the surface's own frame origin), rounded to 6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f64; 3]>,
    /// A cylinder's radius, a torus's minor radius; none for a cone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f64>,
}

/// A recognised feature (Python's `Feature`). Serialises as `Feature.to_dict`: `members` and
/// `notes` only when not empty.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    /// `family:i.j.k`, or `hole_patterns.<kind>:i.j.k` for a pattern or a set of holes.
    pub id: String,
    pub family: String,
    /// The recogniser's record, as JSON (non-finite numbers as `null`).
    pub record: Map<String, Value>,
    /// The feature's faces, ascending.
    pub faces: Vec<usize>,
    /// For patterns and sets: the ids of the member features.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    /// Why a pattern is incomplete, when members could not all be linked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Why faces could not be described or features recognised.
#[derive(Clone, Debug, PartialEq)]
pub enum FeatureError {
    /// The part cannot be trusted as a whole solid ([`LoadedPart::degraded`]'s reason).
    Degraded(String),
    /// haecceity could not integrate the face's area and centroid (`Part::face_moments` gave
    /// none); no value is invented for it.
    NoMoments { part: usize, face: usize },
    /// A family's records and defining faces do not line up, or a record is not a JSON object:
    /// quiddity's output is not what this port reads.
    Records { family: String, reason: String },
}

impl fmt::Display for FeatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeatureError::Degraded(why) => write!(f, "cannot recognise features: {why}"),
            FeatureError::NoMoments { part, face } => write!(
                f,
                "part {part}: face {face}'s area and centroid cannot be computed by haecceity \
                 (Part::face_moments); not described rather than guessed"
            ),
            FeatureError::Records { family, reason } => {
                write!(f, "quiddity's {family} records cannot be read: {reason}")
            }
        }
    }
}

impl std::error::Error for FeatureError {}

/// `family:i.j.k` (Python's `feature_id`).
pub fn feature_id(family: &str, faces: &[usize]) -> String {
    let faces: Vec<String> = faces.iter().map(usize::to_string).collect();
    format!("{family}:{}", faces.join("."))
}

/// Every face of the part, with what a datum or picking question needs (Python's
/// `describe_faces`). A face whose surface haecceity could not resolve is `other`, where
/// haecceity can still integrate it. Refused, naming the face, where haecceity cannot integrate
/// a face's area (no value is made up): this includes an unresolved face with no surface, as on
/// a degraded part (tests/features.rs, `recognition_refuses_a_degraded_part`).
pub fn describe_faces(loaded: &LoadedPart) -> Result<Vec<Face>, FeatureError> {
    let part = &loaded.part;
    let unresolved: BTreeSet<usize> = loaded.unresolved_faces.iter().copied().collect();
    (0..part.faces.len())
        .map(|index| {
            let moments = part.face_moments(index).ok_or(FeatureError::NoMoments {
                part: loaded.binding.part,
                face: index,
            })?;
            // index comes from enumerating this part's faces; the collection is not mutated.
            let face = &part.faces[index];
            let mut out = Face {
                id: index,
                kind: "other".into(),
                area: moments.area,
                centroid: moments.centroid.map(|x| round_half_even(x, 6)),
                direction: None,
                origin: None,
                radius: None,
            };
            if unresolved.contains(&index) {
                return Ok(out);
            }
            // OpenCascade's GeomAbs kinds that specify-core names; a sphere, a freeform surface
            // and anything unresolved are `other`, as there.
            let (kind, frame, radius) = match &face.surface {
                Surface::Plane { frame } => ("plane", frame, None),
                Surface::Cylinder { frame, radius } => ("cylinder", frame, Some(*radius)),
                Surface::Cone { frame, .. } => ("cone", frame, None),
                Surface::Torus { frame, minor, .. } => ("torus", frame, Some(*minor)),
                _ => return Ok(out),
            };
            out.kind = kind.into();
            if kind == "plane" {
                // The outward normal: the surface's, against it for a reversed face (Python
                // multiplies by -1.0, so a zero component becomes -0.0 there too).
                let sign = if face.reversed { -1.0 } else { 1.0 };
                out.direction = Some(rounded(frame.z.map(|x| sign * x), 9));
            } else {
                out.direction = Some(rounded(frame.z, 9));
                out.origin = Some(rounded(frame.origin, 6));
                out.radius = radius;
            }
            Ok(out)
        })
        .collect()
}

fn rounded(v: V3, digits: i32) -> V3 {
    v.map(|x| round_half_even(x, digits))
}

/// The families whose records are made of other families' records: (family, the record field
/// listing the members, the family the members are records of), as quiddity-rust's
/// fingerprints derive them.
const DERIVED: [(&str, &str, &str); 1] = [("hole_patterns", "holes", "holes")];

/// Derived families that are not features: Python's evidence view lists physical families and
/// hole patterns only, so a gusset rib, slot, pocket or oriented slot pattern (derived, like a
/// hole pattern, with no defining faces of its own, but never in Python's analysis) is left out
/// to keep the analysis JSON specify-core's. Its members remain, each a feature of its family.
const NOT_FEATURES: [&str; 4] = [
    "gusset_rib_patterns",
    "slot_patterns",
    "pocket_patterns",
    "oriented_slot_patterns",
];

/// The part's features (Python's `recognise`): every family's records from quiddity with their
/// defining faces, hole patterns with their members, and sets of identical holes no pattern
/// claims. Refused on a degraded part ([`LoadedPart::degraded`]).
pub fn recognise(loaded: &LoadedPart) -> Result<Vec<Feature>, FeatureError> {
    if let Some(why) = loaded.degraded() {
        return Err(FeatureError::Degraded(why));
    }
    let recognition = quiddity::correspondence::recognise(&loaded.part);
    let found = &recognition.features;
    let records = |family: &str, reason: String| FeatureError::Records {
        family: family.to_string(),
        reason,
    };
    let json = serde_json::to_value(found).map_err(|e| records("every", e.to_string()))?;
    let Value::Object(families) = json else {
        return Err(records("every", "not a JSON object".into()));
    };

    let mut features = Vec::new();
    // Each base family's feature ids, record by record (None where a record had no faces), so
    // patterns can name their members.
    let mut ids: Vec<(&str, Vec<Option<String>>)> = Vec::new();
    for (family, list) in &families {
        if DERIVED.iter().any(|(d, _, _)| d == family) || NOT_FEATURES.contains(&family.as_str()) {
            continue;
        }
        let list = list
            .as_array()
            .ok_or_else(|| records(family, "not a list of records".into()))?;
        let defining = found
            .defining
            .get(family.as_str())
            .ok_or_else(|| records(family, "no defining faces".into()))?;
        if defining.len() != list.len() {
            return Err(records(
                family,
                format!(
                    "{} records, {} defining face lists",
                    list.len(),
                    defining.len()
                ),
            ));
        }
        let mut family_ids = Vec::with_capacity(list.len());
        for (record, faces) in list.iter().zip(defining) {
            let mut faces = faces.clone();
            faces.sort_unstable();
            faces.dedup();
            // As Python: a record with no faces in the part is not a feature.
            if faces.is_empty() {
                family_ids.push(None);
                continue;
            }
            let feature = Feature {
                id: feature_id(family, &faces),
                family: family.clone(),
                record: published_record(family, object(family, record)?)?,
                faces,
                members: Vec::new(),
                notes: Vec::new(),
            };
            family_ids.push(Some(feature.id.clone()));
            features.push(feature);
        }
        ids.push((family.as_str(), family_ids));
    }

    // Patterns after their members, with the kind the variant names.
    let kinds: Vec<(&str, Vec<&str>)> = vec![(
        "hole_patterns",
        found.hole_patterns.iter().map(hole_pattern_kind).collect(),
    )];
    for (family, field, base) in DERIVED {
        let list = families
            .get(family)
            .and_then(Value::as_array)
            .ok_or_else(|| records(family, "not a list of records".into()))?;
        let pool = families
            .get(base)
            .and_then(Value::as_array)
            .ok_or_else(|| records(base, "not a list of records".into()))?;
        let pool_ids = ids
            .iter()
            .find(|(b, _)| *b == base)
            .map(|(_, v)| v.as_slice())
            .unwrap_or_default();
        // DERIVED contains only hole_patterns, the family constructed in kinds immediately above.
        let family_kinds = &kinds.iter().find(|(f, _)| *f == family).expect("listed").1;
        let mut taken = vec![false; pool.len()];
        for (n, record) in list.iter().enumerate() {
            let mut record = object(family, record)?;
            let kind = family_kinds.get(n).copied().unwrap_or_default();
            let mut members = Vec::new();
            let mut notes = Vec::new();
            let listed = record.get(field).and_then(Value::as_array);
            for (i, member) in listed.into_iter().flatten().enumerate() {
                // The first record of the base family equal to the member, not yet taken
                // (quiddity-rust's `fingerprint::derived_faces`).
                // taken is allocated with pool.len(); k ranges over that same length.
                let found = (0..pool.len()).find(|&k| !taken[k] && pool[k] == *member);
                match found.and_then(|k| {
                    // k was found in 0..pool.len(); taken has the same length.
                    taken[k] = true;
                    pool_ids.get(k).cloned().flatten()
                }) {
                    Some(id) => members.push(id),
                    None => notes.push(format!("member {i} has no faces in this part")),
                }
            }
            let mut faces: Vec<usize> = members
                .iter()
                .filter_map(|m| features.iter().find(|f: &&Feature| &f.id == m))
                .flat_map(|f| f.faces.iter().copied())
                .collect();
            faces.sort_unstable();
            faces.dedup();
            if faces.is_empty() {
                continue;
            }
            record.insert("kind".into(), Value::String(kind.to_string()));
            let record = published_record(family, record)?;
            features.push(Feature {
                id: feature_id(&format!("{family}.{kind}"), &faces),
                family: family.to_string(),
                record,
                faces,
                members,
                notes,
            });
        }
    }
    let sets = hole_sets(&features);
    features.extend(sets);
    Ok(features)
}

/// Decode a schema-6 measuring direction, or a legacy file-axis letter. No provenance
/// frame participates in geometry. Existing vector-valued directions use this decoder too.
pub(crate) fn axis_direction(value: &Value) -> Option<V3> {
    match value {
        Value::String(axis) => match axis.as_str() {
            "x" => Some([1.0, 0.0, 0.0]),
            "y" => Some([0.0, 1.0, 0.0]),
            "z" => Some([0.0, 0.0, 1.0]),
            _ => None,
        },
        Value::Array(items) if items.len() == 3 => {
            let mut out = [0.0; 3];
            for (slot, item) in out.iter_mut().zip(items) {
                *slot = item.as_f64().filter(|x| x.is_finite())?;
            }
            Some(out)
        }
        _ => None,
    }
}

type EndPlanes = (V3, [V3; 2]);

/// File-coordinate end planes from the published record alone. Also used on saved analysis
/// JSON, which can predate the derived end_* fields. A bad coordinate is a named refusal.
pub(crate) fn end_planes(
    family: &str,
    record: &Map<String, Value>,
) -> Result<Option<EndPlanes>, FeatureError> {
    if !matches!(family, "slots" | "channels" | "oriented_slots") {
        return Ok(None);
    }
    let bad = |field: &str| FeatureError::Records {
        family: family.into(),
        reason: format!("invalid or missing published {field} for end planes"),
    };
    if record
        .get("_local")
        .and_then(Value::as_array)
        .is_some_and(|fields| fields.iter().any(|field| field == "long_axis"))
    {
        return Err(FeatureError::Records {
            family: family.into(),
            reason: "legacy frame-local end planes need reanalysis with recognition schema 6"
                .into(),
        });
    }
    let vector = |field: &str| {
        record
            .get(field)
            .and_then(axis_direction)
            .ok_or_else(|| bad(field))
    };
    let number = |field: &str| {
        record
            .get(field)
            .and_then(Value::as_f64)
            .filter(|x| x.is_finite())
            .ok_or_else(|| bad(field))
    };
    let (direction, origin, levels) = if family == "oriented_slots" {
        // The oriented family already publishes an explicit centre and long direction.
        let direction = vector("long_direction")?;
        let norm = haecceity::geom::dot(direction, direction).sqrt();
        if (norm - 1.0).abs() > 1e-5 {
            return Err(bad("long_direction (unit vector)"));
        }
        (
            direction.map(|x| x / norm),
            vector("center")?,
            [-number("length")? / 2.0, number("length")? / 2.0],
        )
    } else {
        let direction = vector("long_axis")?;
        let origin = if record.contains_key("origin") {
            // Converted records can mix letters and vectors; every scalar shares this origin.
            vector("origin")?
        } else if record.get("long_axis").is_some_and(Value::is_string) {
            // Ordinary file-axis records retain absolute coordinates, including legacy analyses.
            [0.0; 3]
        } else {
            return Err(bad("origin"));
        };
        (direction, origin, [number("lo")?, number("hi")?])
    };
    Ok(Some((
        direction,
        levels.map(|level| std::array::from_fn(|i| origin[i] + level * direction[i])),
    )))
}

fn published_record(
    family: &str,
    mut record: Map<String, Value>,
) -> Result<Map<String, Value>, FeatureError> {
    if let Some((direction, [lo, hi])) = end_planes(family, &record)?
        && (record.contains_key("origin") || !record.get("long_axis").is_some_and(Value::is_string))
    {
        // Keep ordinary file-axis payloads unchanged; the datum rule decodes them directly.
        record.insert("end_direction".into(), serde_json::json!(direction));
        record.insert("end_lo".into(), serde_json::json!(lo));
        record.insert("end_hi".into(), serde_json::json!(hi));
    }
    Ok(record)
}

/// A record as a JSON object, or a refusal naming the family.
fn object(family: &str, record: &Value) -> Result<Map<String, Value>, FeatureError> {
    match record {
        Value::Object(map) => Ok(map.clone()),
        other => Err(FeatureError::Records {
            family: family.to_string(),
            reason: format!("a record is not an object: {other}"),
        }),
    }
}

/// The short kind of a hole pattern, from its variant (Python's `HOLE_PATTERN_KINDS`).
fn hole_pattern_kind(pattern: &HolePattern) -> &'static str {
    let variant = match pattern {
        HolePattern::BoltCircle { .. } => "BoltCircle",
        HolePattern::LinearArray { .. } => "LinearArray",
        HolePattern::RectGrid { .. } => "RectGrid",
        HolePattern::RectangularHoleSet { .. } => "RectangularHoleSet",
    };
    HOLE_PATTERN_KINDS
        .iter()
        .find(|(v, _)| *v == variant)
        .map_or(variant, |(_, k)| k)
}

/// Groups of identical holes that no recognised pattern claims (Python's `_hole_sets`).
///
/// Quiddity recognises layouts (circles, rows, grids, rectangles); a pair of holes, or equal
/// holes scattered over a part, are none of those, but they are specified together: one
/// question, one position tolerance. Holes group when diameter, depth, bottom, entry treatment
/// and drilling direction are all the same. These are sets of equal holes, not recognised
/// layouts: nothing is claimed about their spacing.
fn hole_sets(features: &[Feature]) -> Vec<Feature> {
    let claimed: BTreeSet<&str> = features
        .iter()
        .filter(|f| f.family == "hole_patterns")
        .flat_map(|f| f.members.iter().map(String::as_str))
        .collect();
    // Groups in order of first appearance, as Python's dict.
    let mut groups: Vec<(String, Vec<&Feature>)> = Vec::new();
    for hole in features {
        if hole.family != "holes" || claimed.contains(hole.id.as_str()) {
            continue;
        }
        let r = &hole.record;
        let axis: Vec<Value> = r
            .get("axis")
            .and_then(axis_direction)
            .map(|axis| {
                axis.into_iter()
                    .map(|x| rounded_value(&serde_json::json!(x)))
                    .collect()
            })
            .unwrap_or_default();
        let key = serde_json::json!([
            rounded_value(r.get("diameter").unwrap_or(&Value::Null)),
            rounded_value(r.get("depth").unwrap_or(&Value::Null)),
            r.get("bottom").unwrap_or(&Value::Null),
            truthy(r.get("cbore")),
            truthy(r.get("spotface")),
            truthy(r.get("csink")),
            axis,
        ])
        .to_string();
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(hole),
            None => groups.push((key, vec![hole])),
        }
    }
    groups
        .into_iter()
        .filter(|(_, members)| members.len() >= 2)
        .map(|(_, members)| {
            let mut faces: Vec<usize> = members.iter().flat_map(|h| h.faces.clone()).collect();
            faces.sort_unstable();
            let mut record = Map::new();
            record.insert("kind".into(), Value::String("set".into()));
            record.insert(
                "diameter".into(),
                // This branch has already required at least two members.
                members[0]
                    .record
                    .get("diameter")
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            record.insert(
                "holes".into(),
                Value::Array(
                    members
                        .iter()
                        .map(|h| Value::Object(h.record.clone()))
                        .collect(),
                ),
            );
            Feature {
                id: feature_id("hole_patterns.set", &faces),
                family: "hole_patterns".into(),
                record,
                faces,
                members: members.iter().map(|h| h.id.clone()).collect(),
                notes: Vec::new(),
            }
        })
        .collect()
}

/// Python's `_rounded`: a number rounded to 6 decimals (`None` stays `None`). Negative zero is
/// made zero, as Python's tuple keys compare `-0.0 == 0.0`.
fn rounded_value(value: &Value) -> Value {
    match value.as_f64() {
        Some(x) => serde_json::json!(round_half_even(x, 6) + 0.0),
        None => Value::Null,
    }
}

/// Python's `bool(x)` of a JSON value.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn published_end_planes_decode_letters_vectors_and_refuse_missing_origins() {
        let aligned = json!({"long_axis":"y", "lo":2.0, "hi":8.0});
        assert_eq!(
            end_planes("slots", aligned.as_object().unwrap()).unwrap(),
            Some(([0.0, 1.0, 0.0], [[0.0, 2.0, 0.0], [0.0, 8.0, 0.0]]))
        );
        let mut converted =
            json!({"long_axis":[0.6,0.8,0.0], "origin":[10.0,20.0,30.0], "lo":-5.0, "hi":5.0});
        assert_eq!(
            end_planes("channels", converted.as_object().unwrap()).unwrap(),
            Some(([0.6, 0.8, 0.0], [[7.0, 16.0, 30.0], [13.0, 24.0, 30.0]]))
        );
        converted["long_axis"] = json!("z");
        assert_eq!(
            end_planes("slots", converted.as_object().unwrap()).unwrap(),
            Some(([0.0, 0.0, 1.0], [[10.0, 20.0, 25.0], [10.0, 20.0, 35.0]]))
        );
        converted["long_axis"] = json!([0.6, 0.8, 0.0]);
        converted.as_object_mut().unwrap().remove("origin");
        assert!(
            end_planes("slots", converted.as_object().unwrap())
                .unwrap_err()
                .to_string()
                .contains("origin")
        );
        converted["long_axis"] = json!("a");
        assert!(
            end_planes("slots", converted.as_object().unwrap())
                .unwrap_err()
                .to_string()
                .contains("long_axis")
        );
    }

    fn hole(id: &str, diameter: f64, axis: [f64; 3]) -> Feature {
        Feature {
            id: id.into(),
            family: "holes".into(),
            record: json!({"axis": axis, "diameter": diameter, "depth": 10.0,
                           "bottom": "through", "cbore": null, "spotface": null, "csink": null})
            .as_object()
            .unwrap()
            .clone(),
            faces: vec![id[6..].parse().unwrap()],
            members: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn feature_ids_join_sorted_faces() {
        assert_eq!(feature_id("holes", &[3, 5]), "holes:3.5");
        assert_eq!(feature_id("hole_patterns.set", &[1]), "hole_patterns.set:1");
    }

    #[test]
    fn equal_holes_form_a_set_and_negative_zero_axes_match() {
        let features = vec![
            hole("holes:1", 5.5, [0.0, 0.0, -1.0]),
            hole("holes:2", 5.5, [-0.0, 0.0, -1.0000000001]),
            hole("holes:3", 6.0, [0.0, 0.0, -1.0]),
        ];
        let sets = hole_sets(&features);
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].id, "hole_patterns.set:1.2");
        assert_eq!(sets[0].members, ["holes:1", "holes:2"]);
        assert_eq!(sets[0].record["kind"], "set");
        assert_eq!(sets[0].record["diameter"], 5.5);
    }

    #[test]
    fn python_truthiness() {
        assert!(!truthy(None) && !truthy(Some(&Value::Null)));
        assert!(truthy(Some(&json!({"diameter": 1.0}))));
        assert!(!truthy(Some(&json!({}))));
    }
}
