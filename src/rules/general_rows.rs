//! General dimensions that have no question of their own.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::{Intent, Question};

type Dimension = Map<String, Value>;

fn face_ids<'a>(row: &'a Dimension, key: &str) -> impl Iterator<Item = i64> + 'a {
    row.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
}

fn kind(row: &Dimension) -> &str {
    row.get("kind").and_then(Value::as_str).unwrap_or_default()
}

pub(super) struct Owners<'a>(Vec<(&'static str, &'a str, BTreeSet<i64>)>);

impl<'a> Owners<'a> {
    pub(super) fn new(qs: &'a [Question]) -> Self {
        Self(
            qs.iter()
                .filter_map(|q| {
                    let (question, _) = q.id.split_once(':')?;
                    let kind = match question {
                        "hole.function" | "diameter.fit" => "size",
                        "hole.position" | "hole.location" => "location",
                        _ => return None,
                    };
                    let faces = q.faces.iter().chain(q.parts.iter().flat_map(|(_, f)| f));
                    Some((kind, q.id.as_str(), faces.copied().collect()))
                })
                .collect(),
        )
    }

    pub(super) fn of(&self, row: &Dimension) -> impl Iterator<Item = &'a str> {
        let kind = kind(row);
        let on: BTreeSet<_> = face_ids(row, "faces")
            .chain(face_ids(row, "reference").filter(|_| kind == "location"))
            .collect();
        self.0
            .iter()
            .filter(move |(k, _, faces)| *k == kind && !faces.is_disjoint(&on))
            .map(|(_, id, _)| *id)
    }
}

pub(crate) fn is_id(id: &str) -> bool {
    ["length:planes:", "location:holes:"].iter().any(|prefix| {
        id.strip_prefix(prefix).is_some_and(|faces| {
            !faces.is_empty() && faces.split(['.', '-']).all(|n| n.parse::<usize>().is_ok())
        })
    })
}

/// The value of a suggestion independent of the name of its reference face.
pub(crate) fn measure_text(value: &Value) -> Option<&str> {
    Some(value.as_str()?.split_once(" from ")?.0)
}

pub(crate) struct Suggestion {
    pub(crate) question: Question,
    pub(crate) dimension: Dimension,
}

fn support<T: ToString>(faces: &BTreeSet<T>) -> String {
    faces
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

pub(super) fn measurement_id(row: &Dimension) -> Option<String> {
    identity(
        row,
        &face_ids(row, "reference").collect(),
        &face_ids(row, "faces").collect(),
    )
}

fn identity(row: &Dimension, from: &BTreeSet<i64>, to: &BTreeSet<i64>) -> Option<String> {
    if kind(row) != "location" || from.is_empty() || to.is_empty() {
        return None;
    }
    let hole = row
        .get("feature")
        .and_then(Value::as_str)
        .is_some_and(|f| f.starts_with("holes:") || f.starts_with("hole_patterns"))
        || row.get("planner").is_some_and(|p| {
            let support = &p["required"]["kind"]["location"]["to"];
            support["plane"] == false
                && support["feature"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("hole:") || s.starts_with("pattern["))
        });
    let kind = if hole {
        "location:holes"
    } else {
        "length:planes"
    };
    // Distinct offsets can share the same pair of hole axes, but a plane fixes the axis.
    let axis = row.get("planner").and_then(|p| {
        let location = &p["required"]["kind"]["location"];
        (hole && location["from"]["plane"] == false)
            .then(|| location["axis"].as_u64())
            .flatten()
    });
    Some(row_id(kind, from, to, axis))
}

fn row_id<T: Ord + std::fmt::Display>(
    kind: &str,
    from: &BTreeSet<T>,
    to: &BTreeSet<T>,
    axis: Option<u64>,
) -> String {
    let (a, b) = if kind == "length:planes" && to < from {
        (to, from)
    } else {
        (from, to)
    };
    let mut id = format!("{kind}:{}-{}", support(a), support(b));
    if let Some(axis) = axis {
        id.push_str(&format!("-{axis}"));
    }
    id
}

/// Rebind both supports of a measurement to a new revision, keeping its direction.
pub(crate) fn remap_id(
    id: &str,
    map: impl Fn(usize) -> Result<usize, &'static str>,
) -> Result<String, &'static str> {
    let (kind, sides) = id.rsplit_once(':').ok_or("invalid measurement id")?;
    let mut sides = sides.split('-');
    let faces = |side: Option<&str>| {
        side.ok_or("missing measurement support")?
            .split('.')
            .map(|n| map(n.parse().map_err(|_| "invalid measurement support")?))
            .collect::<Result<BTreeSet<_>, _>>()
    };
    let from = faces(sides.next())?;
    let to = faces(sides.next())?;
    let axis = sides
        .next()
        .map(str::parse)
        .transpose()
        .map_err(|_| "invalid measurement axis")?;
    Ok(row_id(kind, &from, &to, axis))
}

fn suggestion(row: &Dimension, datums: &[Dimension]) -> Option<Suggestion> {
    let to: BTreeSet<_> = face_ids(row, "faces").collect();
    let from: BTreeSet<_> = face_ids(row, "reference").collect();
    let id = identity(row, &from, &to)?;
    let label = if id.starts_with("location:holes:") {
        "Hole location"
    } else {
        "Length"
    };
    let datum = datums.iter().find_map(|datum| {
        let faces: BTreeSet<_> = face_ids(datum, "faces").collect();
        (faces == from)
            .then(|| datum.get("letter")?.as_str())
            .flatten()
    });
    let reference = datum.map(str::to_owned).unwrap_or_else(|| {
        let noun = if from.len() == 1 { "face" } else { "faces" };
        format!(
            "{noun} {}",
            from.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    });
    let text = format!("{} from {reference}", notation(row, "distance")?);
    let mut question = Question::new(
        id,
        "choice",
        format!("{label} {text}?"),
        to.into_iter().collect(),
    );
    question.title = Some(label.into());
    question.basis = "Measured on the part under the general tolerance.".into();
    question.source = Some(&crate::rule_sources::GENERAL_DIMENSION);
    question.options = vec![text.clone(), super::NONE.into()];
    question.default = Some(Value::String(text));
    let mut dimension = row.clone();
    dimension.insert("question".into(), Value::String(question.id.clone()));
    Some(Suggestion {
        question,
        dimension,
    })
}

/// If the note itself was left as a gap, preserve the approved deviation on the dimension.
pub(super) fn explicit_limits(row: &mut Dimension) {
    if row.get("upper").is_none() {
        return;
    }
    row.shift_remove("general");
    if let Some(planner) = row.get_mut("planner") {
        planner["required"]["tolerance"] = serde_json::json!({"explicit": "approved limits"});
    }
}

fn notation(row: &Dimension, key: &str) -> Option<String> {
    let nominal = row.get(key)?.as_f64()?;
    let mut text = crate::pyfmt::g(nominal);
    if let Some(upper) = row.get("upper").and_then(Value::as_f64) {
        text.push_str(&format!(" ±{}", crate::pyfmt::g(upper)));
    }
    Some(text)
}

pub(super) fn suggestions(intent: &Intent, owners: &Owners<'_>) -> Vec<Suggestion> {
    intent
        .general
        .iter()
        .filter(|r| owners.of(r).next().is_none())
        .filter_map(|r| suggestion(r, &intent.datums))
        .collect()
}
