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

use std::collections::{BTreeMap, BTreeSet};

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
    /// Its value in full notation, as a drawing states it (`⌀3.3 H11`, `⌖ ⌀0.3Ⓜ A`): the
    /// `existing` entry's `text`.
    pub text: String,
    /// It states something only its text holds (`existing`'s `text_only`): it cannot be
    /// changed field by field.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub text_only: bool,
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
    /// Stated: the requirements that state it. A location gap: those that locate the feature
    /// in some of its directions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<Requirement>,
    /// Gap: Specify's suggestion (`null` when it has none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<Value>,
    /// A location gap the file already locates in some directions: the directions still to
    /// locate (`x`, `y`, `z`), the only ones its suggestion covers (D8).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
    /// Beside `missing`: the directions the file already locates the feature in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub located: Vec<String>,
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
    /// The table of a part whose feature-and-kinds could not be listed: every requirement the
    /// file states is file PMI, and nothing is asked on the table's account.
    pub fn unread(analysis: &Value) -> FeatureTable {
        FeatureTable {
            rows: Vec::new(),
            file_pmi: analysis["existing"]
                .as_array()
                .into_iter()
                .flatten()
                .map(requirement)
                .collect(),
            decided_none: Vec::new(),
        }
    }

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

/// Whether an answer leaves no PMI of its own behind ("none"): such an answer is stored. A
/// hole left to the general tolerance (`general`) writes its size dimension, so it is not.
pub fn leaves_nothing(value: &Value) -> bool {
    is_none_value(value)
}

/// Whether the answer `id` writes a requirement (a question's feature-and-kind, or an added
/// requirement, `extra.*`) rather than a choice that writes no PMI of its own, or bookkeeping.
pub fn is_requirement(id: &str) -> bool {
    !(id.starts_with('_') || id.starts_with("bom.") || NOT_REQUIREMENTS.contains(&id))
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

/// The notation of an `existing` entry: its own full `text`; for an entry without one (an
/// analysis from before it had one), a short notation of its common fields.
fn text_of(item: &Value) -> String {
    if let Some(text) = item["text"].as_str().filter(|t| !t.is_empty()) {
        return text.to_string();
    }
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
        text_only: item["text_only"] == Value::Bool(true),
    }
}

/// Whether the `existing` entry `item` is a requirement of kind `kind` on `faces`.
fn states(kind: &str, faces: &BTreeSet<usize>, item: &Value) -> bool {
    let item_kind = item["kind"].as_str().unwrap_or_default();
    let r#type = item["type"].as_str().unwrap_or_default();
    let on = |key: &str| faces_of(item, key).iter().any(|f| faces.contains(f));
    // A location's located side is its second set (`faces2`, the "to"); one with a single
    // set locates what it is on. Its reference side locates nothing (D8).
    let on_located = if faces_of(item, "faces2").is_empty() {
        on("faces")
    } else {
        on("faces2")
    };
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
        // A hole's location, whichever scheme the file uses: a position tolerance, or a
        // location dimension (basic, toleranced or general) whose located side it is on. The
        // part's locating setting only decides what is suggested for a hole the file does not
        // locate; a hole is never located twice by two schemes (Paul, 2026-11-21). Holes are
        // located per direction ([`Located`]); this is the match for what is not a hole.
        "position" | "frame" => {
            (item_kind == "geometric_tolerance" && r#type == "Position" && on("faces"))
                || (item_kind == "dimension" && r#type.starts_with("Location_") && on_located)
        }
        "location" => item_kind == "dimension" && r#type.starts_with("Location_") && on_located,
        // A hole's or a diameter's size: its dimension, or the thread or knurl callout that
        // states it.
        "function" | "fit" | "size" => {
            (item_kind == "dimension" && r#type.starts_with("Size_") && on("faces"))
                || (item_kind == "note"
                    && (r#type.ends_with("thread") || r#type == "knurl")
                    && on("faces"))
        }
        "full_thread" | "thread" => {
            item_kind == "note" && r#type.ends_with("thread") && on("faces")
        }
        "knurl" => item_kind == "note" && r#type == "knurl" && on("faces"),
        "note" => item_kind == "note" && on("faces"),
        "finish" => item_kind == "note" && r#type.starts_with("surface") && on("faces"),
        "flatness" => item_kind == "geometric_tolerance" && r#type == "Flatness" && on("faces"),
        "runout" | "total_runout" => {
            item_kind == "geometric_tolerance" && r#type.contains("Runout") && on("faces")
        }
        _ => item_kind != "datum" && on("faces"),
    }
}

/// The measuring axes, by index.
const AXES: [&str; 3] = ["x", "y", "z"];
/// Coordinates (mm) closer than this are on the same line.
const ON_LINE: f64 = 1e-3;
/// A dimension's value and a distance measured on the part (mm) closer than this agree.
const SAME_VALUE: f64 = 1e-2;

/// Whether a question's kind is a hole's location (`hole.position`, `hole.frame`,
/// `hole.location`, or a direct intent's `requirement.location`), matched per direction.
fn is_location_kind(kind: &str) -> bool {
    !kind.starts_with("datum.")
        && matches!(
            kind.rsplit('.').next().unwrap_or(kind),
            "position" | "frame" | "location"
        )
}

fn vector(face: &Value, key: &str) -> Option<[f64; 3]> {
    match face.get(key)?.as_array()?.as_slice() {
        [x, y, z] => Some([x.as_f64()?, y.as_f64()?, z.as_f64()?]),
        _ => None,
    }
}

fn measured(face: &Value) -> bool {
    face.get("unmeasured") != Some(&Value::Bool(true))
}

/// A point and a direction.
type Line = ([f64; 3], [f64; 3]);

/// A surface of revolution's axis line (a hole's bore): a point on it and its direction.
fn axis_line(face: &Value) -> Option<Line> {
    if !measured(face) || !matches!(face["kind"].as_str()?, "cylinder" | "cone" | "torus") {
        return None;
    }
    Some((vector(face, "origin")?, vector(face, "direction")?))
}

/// A measured plane's centroid and normal.
fn plane(face: &Value) -> Option<Line> {
    if !measured(face) || face["kind"] != "plane" {
        return None;
    }
    Some((vector(face, "centroid")?, vector(face, "direction")?))
}

/// Where the file locates the part's holes, direction by direction (D8, Paul 2026-11-21). A
/// location dimension locates, along its measuring axis (its reference face's normal, by
/// [`crate::pmi_origin::axis_of`]), every hole on its located side (`faces2`, the "to"), and
/// every other hole whose axis lies on the same line from the same reference; its reference
/// side (`faces`, the "from") locates nothing. A datum hole is the origin the part is measured
/// from in the directions no other datum plane measures: it is not asked its location there.
pub(crate) struct Located<'a> {
    faces: &'a [Value],
    /// (face, axis) → the `existing` items that locate that face along that axis.
    by: BTreeMap<(usize, usize), BTreeSet<usize>>,
}

/// How the file locates one feature's holes.
pub(crate) struct Location {
    /// The axes some hole of the feature needs and nothing locates it in.
    pub missing: BTreeSet<usize>,
    /// The axes some hole of the feature needs.
    pub needed: BTreeSet<usize>,
    /// The `existing` items that locate it in the others.
    pub items: BTreeSet<usize>,
}

impl<'a> Located<'a> {
    /// The holes `existing` locates, on the part whose faces (the analysis's) are `faces`.
    pub(crate) fn new(existing: &[&Value], faces: &'a [Value]) -> Located<'a> {
        let mut located = Located {
            faces,
            by: BTreeMap::new(),
        };
        for (i, item) in existing.iter().enumerate() {
            let r#type = item["type"].as_str().unwrap_or_default();
            match item["kind"].as_str() {
                Some("dimension") if r#type.starts_with("Location_") => {
                    located.dimension(i, item);
                }
                Some("datum") => located.origin(i, item, existing),
                _ => {}
            }
        }
        located
    }

    fn face(&self, i: usize) -> Option<&Value> {
        self.faces.get(i)
    }

    fn cover(&mut self, face: usize, axis: usize, item: usize) {
        self.by.entry((face, axis)).or_default().insert(item);
    }

    fn dimension(&mut self, i: usize, item: &Value) {
        let (reference, located) = match faces_of(item, "faces2") {
            to if to.is_empty() => (Vec::new(), faces_of(item, "faces")),
            to => (faces_of(item, "faces"), to),
        };
        let value = item["value"].as_f64();
        let axis = if item["type"] == "Location_Angular" {
            None
        } else {
            self.measuring_axis(&reference, &located, value)
        };
        match axis {
            // A direction the part cannot tell (an angle, a diagonal): it locates what it is
            // on in every direction.
            None => {
                for &f in &located {
                    (0..3).for_each(|a| self.cover(f, a, i));
                }
            }
            Some(a) => {
                for &f in &located {
                    self.cover(f, a, i);
                }
                self.on_the_line(i, a, &reference, &located, value);
            }
        }
    }

    /// The axis a location from `reference` to `located` measures along: the reference
    /// plane's normal's (or the located plane's); between two holes, the axis across both
    /// whose offset is the value.
    pub(crate) fn measuring_axis(
        &self,
        reference: &[usize],
        located: &[usize],
        value: Option<f64>,
    ) -> Option<usize> {
        let first = |faces: &[usize], f: fn(&Value) -> Option<Line>| {
            faces.iter().find_map(|&i| self.face(i).and_then(f))
        };
        if let Some((_, normal)) = first(reference, plane).or_else(|| first(located, plane)) {
            return Some(crate::pmi_origin::axis_of(normal));
        }
        let (from, from_axis) = first(reference, axis_line)?;
        let (to, to_axis) = first(located, axis_line)?;
        let skip = [
            crate::pmi_origin::axis_of(from_axis),
            crate::pmi_origin::axis_of(to_axis),
        ];
        let offset = |a: usize| (to[a] - from[a]).abs();
        let across = (0..3).filter(|a| !skip.contains(a));
        let mut fitting: Vec<usize> = match value {
            Some(v) => across
                .filter(|&a| (offset(a) - v).abs() < SAME_VALUE)
                .collect(),
            None => across.filter(|&a| offset(a) > ON_LINE).collect(),
        };
        (fitting.len() == 1).then(|| fitting.remove(0))
    }

    /// Every other hole whose axis lies, along `axis`, on the line the dimension `i` locates.
    fn on_the_line(
        &mut self,
        i: usize,
        axis: usize,
        reference: &[usize],
        located: &[usize],
        value: Option<f64>,
    ) {
        let from = reference.iter().find_map(|&f| {
            let face = self.face(f)?;
            plane(face)
                .or_else(|| axis_line(face))
                .map(|(p, _)| p[axis])
        });
        let across = |(p, d): Line| (crate::pmi_origin::axis_of(d) != axis).then_some(p[axis]);
        let lines: Vec<f64> = located
            .iter()
            .filter_map(|&f| self.face(f).and_then(axis_line).and_then(across))
            .collect();
        // The line the value measures to, when it tells (a pattern's centre line among its
        // members' lines).
        let measured: Vec<f64> = match (from, value) {
            (Some(from), Some(v)) => lines
                .iter()
                .copied()
                .filter(|c| ((c - from).abs() - v).abs() < SAME_VALUE)
                .collect(),
            _ => Vec::new(),
        };
        let lines = if measured.is_empty() { lines } else { measured };
        let on: Vec<usize> = (0..self.faces.len())
            .filter(|g| !located.contains(g) && !reference.contains(g))
            .filter(|&g| {
                self.face(g)
                    .and_then(axis_line)
                    .and_then(across)
                    .is_some_and(|c| lines.iter().any(|l| (c - l).abs() < ON_LINE))
            })
            .collect();
        for g in on {
            self.cover(g, axis, i);
        }
    }

    /// A datum on a hole: the origin of the directions across its axis that no other datum
    /// plane measures.
    fn origin(&mut self, i: usize, item: &Value, existing: &[&Value]) {
        let measured_from: BTreeSet<usize> = existing
            .iter()
            .enumerate()
            .filter(|(j, e)| *j != i && e["kind"] == "datum")
            .flat_map(|(_, e)| faces_of(e, "faces"))
            .filter_map(|f| self.face(f).and_then(plane))
            .map(|(_, normal)| crate::pmi_origin::axis_of(normal))
            .collect();
        for f in faces_of(item, "faces") {
            if let Some((_, direction)) = self.face(f).and_then(axis_line) {
                let along = crate::pmi_origin::axis_of(direction);
                for a in (0..3).filter(|a| *a != along && !measured_from.contains(a)) {
                    self.cover(f, a, i);
                }
            }
        }
    }

    /// The holes among `faces`, coaxial faces (a counterbore and its drill) as one: (the axis
    /// each runs along, its faces).
    fn holes(&self, faces: &BTreeSet<usize>) -> Vec<(usize, Vec<usize>)> {
        let mut holes: Vec<([f64; 3], usize, Vec<usize>)> = Vec::new();
        for &f in faces {
            let Some((point, direction)) = self.face(f).and_then(axis_line) else {
                continue;
            };
            let along = crate::pmi_origin::axis_of(direction);
            let same = holes.iter_mut().find(|(p, a, _)| {
                *a == along && (0..3).all(|k| k == along || (p[k] - point[k]).abs() < ON_LINE)
            });
            match same {
                Some((_, _, members)) => members.push(f),
                None => holes.push((point, along, vec![f])),
            }
        }
        holes.into_iter().map(|(_, a, f)| (a, f)).collect()
    }

    /// How the file locates the holes among `faces`, in every direction across their axes;
    /// none when `faces` has no hole.
    pub(crate) fn location(&self, faces: &BTreeSet<usize>) -> Option<Location> {
        let holes = self.holes(faces);
        if holes.is_empty() {
            return None;
        }
        let mut location = Location {
            missing: BTreeSet::new(),
            needed: BTreeSet::new(),
            items: BTreeSet::new(),
        };
        for (along, members) in holes {
            for a in (0..3).filter(|a| *a != along) {
                location.needed.insert(a);
                let by: BTreeSet<usize> = members
                    .iter()
                    .filter_map(|&f| self.by.get(&(f, a)))
                    .flatten()
                    .copied()
                    .collect();
                if by.is_empty() {
                    location.missing.insert(a);
                }
                location.items.extend(by);
            }
        }
        Some(location)
    }

    /// Whether every hole among `faces` is located along `axis` (every direction it needs,
    /// when the axis is not known); none when `faces` has no hole.
    pub(crate) fn locates(&self, faces: &BTreeSet<usize>, axis: Option<usize>) -> Option<bool> {
        let holes = self.holes(faces);
        if holes.is_empty() {
            return None;
        }
        Some(holes.iter().all(|(along, members)| {
            let needed: Vec<usize> = match axis {
                Some(a) => vec![a],
                None => (0..3).filter(|a| a != along).collect(),
            };
            needed
                .iter()
                .all(|&a| members.iter().any(|&f| self.by.contains_key(&(f, a))))
        }))
    }
}

/// Whether the file's `existing` PMI states the feature-and-kind a writer's requirement
/// (`intent.requirements` or `intent.general`) fills: the table's matching, by the faces it
/// sits on and its kind (the question it answers, or its own kind for a direct intent).
/// `geometry` is the part's faces as the analysis lists them. A location dimension is stated
/// when the file already locates every hole it measures along its axis (D8: never a second
/// location in a direction); a hole's position when the file locates it in every direction.
pub(crate) fn requirement_stated(
    req: &serde_json::Map<String, Value>,
    existing: &[Value],
    geometry: &[Value],
) -> bool {
    let ids = |key: &str| -> Vec<usize> {
        req.get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .map(|f| f as usize)
            .collect()
    };
    let items: Vec<&Value> = existing.iter().collect();
    let located = Located::new(&items, geometry);
    if req.get("kind").and_then(Value::as_str) == Some("location") {
        let on = ids("faces");
        let axis = located.measuring_axis(
            &ids("reference"),
            &on,
            req.get("distance").and_then(Value::as_f64),
        );
        if let Some(stated) = located.locates(&on.into_iter().collect(), axis) {
            return stated;
        }
    }
    // An added requirement (`extra.*`) names no feature-and-kind: its own kind does.
    let question = req
        .get("question")
        .and_then(Value::as_str)
        .filter(|q| !q.starts_with(crate::extras::PREFIX));
    let kind = match question {
        Some(question) => kind_and_feature(question).0,
        // The dimension planner's own rows (a depth, a pitch) are no feature-and-kind of the
        // table; the writer compares them measure by measure.
        None if req.contains_key("planner") => return false,
        None => match req.get("kind").and_then(Value::as_str) {
            // A control on a datum feature (perpendicularity, parallelism, ...).
            Some(k) if !KNOWN_KINDS.contains(&k) => "datum.X.control".to_string(),
            Some(k) => format!("requirement.{k}"),
            None => return false,
        },
    };
    let faces: BTreeSet<usize> = req
        .get("faces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .map(|f| f as usize)
        .collect();
    if faces.is_empty() || kind.starts_with("part.") {
        return false;
    }
    if is_location_kind(&kind)
        && let Some(location) = located.location(&faces)
    {
        return location.missing.is_empty()
            || existing
                .iter()
                .any(|item| is_position(item) && on_any(item, &faces));
    }
    existing.iter().any(|item| states(&kind, &faces, item))
}

/// Whether `item` is a position tolerance.
fn is_position(item: &Value) -> bool {
    item["kind"] == "geometric_tolerance" && item["type"] == "Position"
}

/// Whether `item` sits on any of `faces` (its first set).
fn on_any(item: &Value, faces: &BTreeSet<usize>) -> bool {
    faces_of(item, "faces").iter().any(|f| faces.contains(f))
}

/// The requirement kinds [`states`] knows by name; any other is a datum control.
const KNOWN_KINDS: [&str; 12] = [
    "note",
    "size",
    "location",
    "position",
    "flatness",
    "runout",
    "total_runout",
    "thread",
    "knurl",
    "finish",
    "fit",
    "frame",
];

/// The setting key a part question reads (`part.notes` → `notes`).
fn setting(id: &str) -> Option<&str> {
    id.strip_prefix("part.")
}

pub(crate) fn setting_text(value: &Value) -> String {
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
    let (bare, given) = set_aside(analysis, answers);
    let all = crate::rules::with_all_defaults(&bare, &given).unwrap_or_else(|_| given.clone());
    let questions =
        crate::rules::questions(&bare, &all).or_else(|_| crate::rules::questions(&bare, &given))?;

    let geometry: Vec<Value> = analysis["faces"].as_array().cloned().unwrap_or_default();
    let located = Located::new(&existing, &geometry);
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
        let mut missing = Vec::new();
        let mut located_in = Vec::new();
        let location = is_location_kind(&kind)
            .then(|| located.location(&faces))
            .flatten();
        if let Some(location) = location {
            // A hole's location, per direction (D8): stated by a position tolerance, or when
            // every direction it needs is located; otherwise a gap for the directions left.
            let positions: BTreeSet<usize> = if kind.ends_with(".location") {
                BTreeSet::new()
            } else {
                (0..existing.len())
                    .filter(|&i| is_position(existing[i]) && on_any(existing[i], &faces))
                    .collect()
            };
            for &i in positions.union(&location.items) {
                claimed[i] = true;
                requirements.push(requirement(existing[i]));
            }
            if positions.is_empty() && !location.missing.is_empty() {
                if !location.items.is_empty() {
                    missing = location
                        .missing
                        .iter()
                        .map(|&a| AXES[a].to_string())
                        .collect();
                    located_in = location
                        .needed
                        .difference(&location.missing)
                        .map(|&a| AXES[a].to_string())
                        .collect();
                }
            } else {
                value = Some(Value::String(
                    requirements
                        .iter()
                        .map(|r| r.text.as_str())
                        .collect::<Vec<_>>()
                        .join("; "),
                ));
            }
        } else if q.id.starts_with("part.") {
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
                missing: Vec::new(),
                located: Vec::new(),
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
                missing: Vec::new(),
                located: Vec::new(),
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
                missing,
                located: located_in,
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
                text_only: false,
            });
        }
    }
    Ok(FeatureTable {
        rows,
        file_pmi,
        decided_none,
    })
}

/// Every numbered requirement the table does not list yet, added where it belongs, so each can
/// be removed by its number: a finish on the whole part (`Ra`, a surface texture on no faces)
/// on the `part.surface_finish` row when that row is stated, anything else as file PMI.
pub fn add_numbered(table: &mut FeatureTable, numbers: &[crate::characteristics::Characteristic]) {
    let listed: BTreeSet<String> = table
        .rows
        .iter()
        .flat_map(|r| &r.requirements)
        .chain(&table.file_pmi)
        .filter_map(|r| r.number.clone())
        .collect();
    for c in numbers {
        let tag = format!("#{}", c.number);
        if listed.contains(&tag) {
            continue;
        }
        let mut faces = c.reference.clone();
        faces.extend(&c.faces);
        let mut requirement = Requirement {
            number: Some(tag),
            kind: c.kind.clone(),
            r#type: c.r#type.clone(),
            faces,
            text: c.r#type.clone(),
            text_only: false,
        };
        let finish = table.rows.iter_mut().find(|r| {
            r.id == "part.surface_finish" && r.state == State::Stated && c.faces.is_empty()
        });
        match finish {
            Some(row) if c.kind == "surface_texture" => {
                if let Some(Value::String(text)) = &row.value {
                    requirement.text = text.clone();
                }
                row.requirements.push(requirement);
            }
            _ => table.file_pmi.push(requirement),
        }
    }
}

/// `analysis` with the file's PMI set aside, so its questions are every feature-and-kind the
/// part has (a stated feature is not asked, but keeps its row), and `answers` with the file's
/// datums standing for their questions, so the controls are on the faces the file gives them.
fn set_aside(analysis: &Value, answers: &Map<String, Value>) -> (Value, Map<String, Value>) {
    let mut bare = analysis.clone();
    if let Some(object) = bare.as_object_mut() {
        object.insert("existing".into(), Value::Array(Vec::new()));
        object.insert("part".into(), Value::Object(Map::new()));
        object.remove("resume");
        object.remove("table");
        object.remove("dimensioning_stated");
        object.remove("process");
    }
    let mut given: Map<String, Value> = answers
        .iter()
        .filter(|(id, _)| !id.starts_with('_'))
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect();
    for item in analysis["existing"].as_array().into_iter().flatten() {
        if item["kind"] == "datum"
            && let Some(letter) = item["type"].as_str()
        {
            let faces = faces_of(item, "faces");
            let id = format!("datum.{letter}");
            if !faces.is_empty() && !given.contains_key(&id) {
                // A file datum the rules cannot take as an answer (faces that form no one
                // datum feature, NIST FTC-09's E) still names its row; only its controls'
                // faces fall back to the suggestion's.
                let mut trial = given.clone();
                trial.insert(id, Value::from(faces));
                if crate::rules::questions(&bare, &trial).is_ok() {
                    given = trial;
                }
            }
        }
    }
    (bare, given)
}

/// The analysis and answers that plan what the file states, as nearly as they can be told from
/// it: the part with its PMI set aside, and the stored `answers`, the file's datums and every
/// other question's suggestion. A file keeps no answer that writes PMI (ADR P11), so the scene
/// names a file's requirements by this plan.
pub(crate) fn planned_answers(
    analysis: &Value,
    answers: &Map<String, Value>,
) -> (Value, Map<String, Value>) {
    let (bare, given) = set_aside(analysis, answers);
    let all = crate::rules::with_all_defaults(&bare, &given).unwrap_or(given);
    (bare, all)
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
