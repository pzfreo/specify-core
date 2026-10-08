//! Datums: which are proposed and asked, what the answers make of them, how datum B and C are
//! held to the datums before them, and the frames a position may be measured from.

use serde_json::{Map, Value};

use super::py::{self, dot};
use super::records::{Ctx, Datum, DatumKind, Datums, DefaultDatums, Face, Faces};
use super::{EXTRA_DATUMS, Error, NONE, Question, Result, invalid};

/// The letters A to C, then the extra datums.
pub(crate) const LETTERS: [char; 8] = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H'];

/// `datum_kind`: `plane` for coplanar faces, `axis` for the coaxial cylinders of one hole or
/// boss; anything else cannot be a datum here (Python's `ValueError`, with its message).
pub fn datum_kind(face_ids: &[i64], faces: &Faces) -> Result<DatumKind> {
    let shown: Vec<Value> = face_ids.iter().map(|&i| Value::from(i)).collect();
    kind_of(face_ids, &shown, faces)
}

/// `datum_kind` on faces already read as ids; `shown` is the list as given, for the message.
pub(crate) fn kind_of(face_ids: &[i64], shown: &[Value], faces: &Faces) -> Result<DatumKind> {
    classify(face_ids, faces)?.ok_or_else(|| {
        Error::Invalid(format!(
            "faces {} are neither coplanar faces nor one hole or boss, so they cannot form one \
             datum",
            py::repr(&Value::Array(shown.to_vec()))
        ))
    })
}

/// What `face_ids` form: a plane, an axis, or neither (None: Python's `ValueError`, which
/// callers may catch). A face that is not there, or lacks what the test needs, is an error
/// (Python's `KeyError`, which no caller catches).
pub(crate) fn classify(face_ids: &[i64], faces: &Faces) -> Result<Option<DatumKind>> {
    let chosen: Vec<&Face> = face_ids
        .iter()
        .map(|&i| faces.at(i))
        .collect::<Result<_>>()?;
    let Some(first) = chosen.first() else {
        // Python's chosen[0] raises IndexError; no caller passes an empty datum.
        return invalid("a datum needs at least one face");
    };
    if chosen.iter().all(|f| f.is_plane()) && all(&chosen, |f| coplanar(f, first))? {
        return Ok(Some(DatumKind::Plane));
    }
    if chosen.iter().all(|f| f.is_cylinder()) && all(&chosen, |f| coaxial(f, first))? {
        return Ok(Some(DatumKind::Axis));
    }
    Ok(None)
}

/// Python's `all(test(f) for f in faces)`, stopping at the first false.
fn all(faces: &[&Face], test: impl Fn(&Face) -> Result<bool>) -> Result<bool> {
    for f in faces {
        if !test(f)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `_coplanar`: same normal and the same level along it.
pub(crate) fn coplanar(f: &Face, g: &Face) -> Result<bool> {
    let n = f.direction()?;
    let (a, b) = (f.centroid()?, g.centroid()?);
    let offset = 0.0 + (a[0] - b[0]) * n[0] + (a[1] - b[1]) * n[1] + (a[2] - b[2]) * n[2];
    Ok(same_direction(&n, &g.direction()?) && offset.abs() < 1e-4)
}

/// `_coaxial`: the same radius on one axis.
pub(crate) fn coaxial(f: &Face, g: &Face) -> Result<bool> {
    if (f.radius()? - g.radius()?).abs() > 1e-6 || !parallel(&f.direction()?, &g.direction()?) {
        return Ok(false);
    }
    let (o, p) = (f.origin()?, g.origin()?);
    let d = [o[0] - p[0], o[1] - p[1], o[2] - p[2]];
    let u = g.direction()?;
    let cross = [
        d[1] * u[2] - d[2] * u[1],
        d[2] * u[0] - d[0] * u[2],
        d[0] * u[1] - d[1] * u[0],
    ];
    Ok(hypot(&cross) < 1e-4)
}

/// `math.hypot` of three components, scaled so it neither overflows nor underflows.
fn hypot(v: &[f64; 3]) -> f64 {
    let scale = v.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    if scale == 0.0 || !scale.is_finite() {
        return scale;
    }
    let s = v.iter().map(|x| (x / scale) * (x / scale)).sum::<f64>();
    scale * s.sqrt()
}

fn same_direction(a: &[f64; 3], b: &[f64; 3]) -> bool {
    dot(a, b) > 1.0 - 1e-6
}

/// `_parallel`: the same line, either way along it.
pub(crate) fn parallel(a: &[f64; 3], b: &[f64; 3]) -> bool {
    (dot(a, b).abs() - 1.0).abs() < 1e-6
}

/// `_perpendicular`.
pub(crate) fn perpendicular(a: &[f64; 3], b: &[f64; 3]) -> bool {
    dot(a, b).abs() < 1e-6
}

/// The planes by area, largest first (a stable sort, as Python's `sorted(key=-area)`).
fn planes_by_area(faces: &Faces) -> Result<Vec<&Face>> {
    let mut planes: Vec<(&Face, f64)> = faces
        .iter()
        .filter(|f| f.is_plane())
        .map(|f| Ok((f, f.area()?)))
        .collect::<Result<_>>()?;
    py::sort_by_float(&mut planes, |(_, area)| -area);
    Ok(planes.into_iter().map(|(f, _)| f).collect())
}

/// `_default_datums`: A the face the part rests on in its assembly, else the largest plane,
/// with every face coplanar with it; on a turned part B is the largest cylinder on the axis A
/// is square to (and every face coaxial with it); otherwise B is the largest plane square to A
/// and C the largest square to both. `turned`: the part has turned steps.
pub fn default_datums(faces: &Faces, turned: bool) -> Result<DefaultDatums> {
    let planes = planes_by_area(faces)?;
    let Some(&largest) = planes.first() else {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    };
    let a = resting_face(&planes)?.unwrap_or(largest);
    let mut a_ids = Vec::new();
    for p in &planes {
        if coplanar(p, a)? {
            a_ids.push(p.id);
        }
    }
    a_ids.sort_unstable();
    let normal = a.direction()?;
    if turned {
        let mut cylinders: Vec<(&Face, f64)> = Vec::new();
        for f in faces.iter().filter(|f| f.is_cylinder()) {
            if parallel(&f.direction()?, &normal) {
                cylinders.push((f, f.area()?));
            }
        }
        py::sort_by_float(&mut cylinders, |(_, area)| -area);
        // With no cylinder on A's axis there is no B (Python's comprehension over none never
        // reads cylinders[0]).
        let Some(&(widest, _)) = cylinders.first() else {
            return Ok((a_ids, Vec::new(), Vec::new()));
        };
        let mut b_ids = Vec::new();
        for (f, _) in &cylinders {
            if coaxial(f, widest)? {
                b_ids.push(f.id);
            }
        }
        b_ids.sort_unstable();
        return Ok((a_ids, b_ids, Vec::new()));
    }
    let mut square = Vec::new();
    for p in &planes {
        if perpendicular(&p.direction()?, &normal) {
            square.push(*p);
        }
    }
    let Some(&b) = square.first() else {
        return Ok((a_ids, Vec::new(), Vec::new()));
    };
    let b_normal = b.direction()?;
    let mut c = Vec::new();
    for p in &square[1..] {
        if perpendicular(&p.direction()?, &b_normal) {
            c.push(p.id);
            break;
        }
    }
    Ok((a_ids, vec![b.id], c))
}

/// `_resting_face`: in an assembly, the plane where the part meets another, the one with the
/// most contact area (the first of equals); None where no plane touches anything.
fn resting_face<'f>(planes: &[&'f Face]) -> Result<Option<&'f Face>> {
    let touching: Vec<&Face> = planes
        .iter()
        .copied()
        .filter(|p| py::truthy_opt(p.contact.as_ref()))
        .collect();
    let mut best: Option<(&Face, f64)> = None;
    for &plane in &touching {
        let mut area = 0.0;
        for p in &touching {
            if coplanar(p, plane)? {
                area += contact_area(p)?;
            }
        }
        // max() keeps the first of equal keys.
        if best.is_none_or(|(_, top)| area > top) {
            best = Some((plane, area));
        }
    }
    Ok(best.map(|(f, _)| f))
}

fn contact_area(face: &Face) -> Result<f64> {
    face.contact
        .as_ref()
        .and_then(|c| c.get("area"))
        .and_then(Value::as_f64)
        .ok_or_else(|| Error::Invalid(format!("face {}: its 'contact' has no 'area'", face.id)))
}

/// `_datum_a_doubt`: why the largest plane may not be the primary datum, when another plane
/// (coplanar faces together), facing elsewhere, is nearly as large.
pub(crate) fn datum_a_doubt(faces: &Faces) -> Result<String> {
    let mut groups: Vec<(&Face, f64)> = Vec::new();
    for f in planes_by_area(faces)? {
        let mut joined = false;
        for group in groups.iter_mut() {
            if coplanar(f, group.0)? {
                group.1 += f.area()?;
                joined = true;
                break;
            }
        }
        if !joined {
            groups.push((f, f.area()?));
        }
    }
    py::sort_by_float(&mut groups, |(_, area)| -area);
    if groups.len() > 1 && groups[1].1 >= 0.8 * groups[0].1 {
        return Ok(format!(
            "face {} is nearly as large as face {}; the primary datum is whichever the part is \
             mounted on",
            groups[1].0.id, groups[0].0.id
        ));
    }
    Ok(String::new())
}

/// `_datum_questions`: the letters in `propose`, A to C with their defaults (a letter beyond C
/// asked empty); any letter the person has given or removed, always. A file with datums has its
/// frame: only the letters it lacks are asked, and none is proposed.
pub(crate) fn datum_questions(
    ctx: &Ctx<'_>,
    answers: &Map<String, Value>,
    propose: &str,
) -> Result<Vec<Question>> {
    let why = "Datums fix the part for measurement; tolerances refer to them. A datum is one \
               face, several coplanar faces, or a hole or boss (its axis).";
    let turned = ctx.has_family("turned_steps");
    let mut basis_a =
        "The largest flat face: usually what the part sits on or is clamped by.".to_string();
    let basis_b = if turned {
        "The part's axis: the largest diameter square to A."
    } else {
        "The largest flat face square to A."
    };
    let basis_c = "The largest flat face square to A and B.";
    // Which letters have a default, and what it is.
    let mut defaults: Vec<(char, Vec<i64>)> = Vec::new();
    let mut resting = String::new();
    if !propose.is_empty() && ctx.in_file.is_empty() {
        let (a, b, c) = ctx.default_datums()?;
        for (x, d) in [('A', a), ('B', b), ('C', c)] {
            if propose.contains(x) {
                defaults.push((x, d.clone()));
            }
        }
        let mut contact = None;
        for &i in a {
            let face = ctx.faces.at(i)?;
            if py::truthy_opt(face.contact.as_ref()) {
                contact = face.contact.as_ref();
                break;
            }
        }
        if let Some(contact) = contact {
            let part = contact
                .get("part")
                .ok_or_else(|| Error::Invalid("a face's 'contact' has no 'part'".to_string()))?;
            resting = format!(
                "it is where the part meets {}: what locates it there",
                py::str_of(part)
            );
            basis_a = py::sentence(&resting);
        }
        for x in propose.chars() {
            if !"ABC".contains(x) && !defaults.iter().any(|(y, _)| *y == x) {
                defaults.push((x, Vec::new()));
            }
        }
    }
    let mut out = Vec::new();
    for letter in LETTERS {
        let known = defaults.iter().find(|(x, _)| *x == letter);
        if ctx.file_datum(letter).is_some()
            || (known.is_none() && !answers.contains_key(&format!("datum.{letter}")))
        {
            continue;
        }
        let default = known.map(|(_, d)| d.clone()).unwrap_or_default();
        let prompt = if letter == 'A' {
            "Which face(s), hole or boss is datum A (primary)?".to_string()
        } else {
            format!("Which face(s), hole or boss is datum {letter}? (empty for none)")
        };
        let basis = match letter {
            _ if default.is_empty() => String::new(),
            'A' => basis_a.clone(),
            'B' => basis_b.to_string(),
            'C' => basis_c.to_string(),
            _ => String::new(),
        };
        out.push(Question {
            default: Some(Value::from(default.clone())),
            why: if "ABC".contains(letter) {
                why.to_string()
            } else {
                "An extra datum, for features located from something other than A.".to_string()
            },
            basis,
            mate: if letter == 'A' {
                resting.clone()
            } else {
                String::new()
            },
            ..Question::new(format!("datum.{letter}"), "faces", prompt, default)
        });
    }
    Ok(out)
}

/// `_datums`: each datum in letter order, the file's own first (one whose faces cannot be read
/// cannot be cited), then those the answers (or the questions' defaults) choose.
pub(crate) fn chosen_datums(
    ctx: &Ctx<'_>,
    answers: &Map<String, Value>,
    qs: &[Question],
) -> Result<Datums> {
    let mut out = Vec::new();
    for letter in LETTERS {
        if let Some(ids) = ctx.file_datum(letter) {
            if !ids.is_empty() {
                for &i in ids {
                    if !ctx.faces.contains(i) {
                        // Python's KeyError: the file's datum names a face the part lacks.
                        return invalid(format!(
                            "the file's datum {letter} is on face {i}, which the part does not have"
                        ));
                    }
                }
                // A datum whose faces form neither a plane nor an axis is not citable.
                if let Some(kind) = classify(ids, &ctx.faces)? {
                    out.push((
                        letter,
                        Datum {
                            faces: ids.to_vec(),
                            kind,
                            existing: true,
                        },
                    ));
                }
            }
            continue;
        }
        let qid = format!("datum.{letter}");
        let Some(chosen) = super::answer(answers, qs, &qid) else {
            continue;
        };
        if !py::truthy(chosen) {
            continue;
        }
        let Some(items) = py::iter_of(chosen) else {
            // Python's TypeError: a number is not a list of faces.
            return invalid(format!(
                "datum {letter}: {} is not a list of faces",
                py::repr(chosen)
            ));
        };
        let mut ids = Vec::with_capacity(items.len());
        for item in &items {
            let id = py::int_of(item)
                .map_err(|e| Error::Invalid(format!("datum {letter}: {}", e.message())))?;
            if !ctx.faces.contains(id) {
                return invalid(format!(
                    "datum {letter}: face {} does not exist",
                    py::str_of(item)
                ));
            }
            ids.push(id);
        }
        let kind = kind_of(&ids, &items, &ctx.faces).map_err(|e| match e {
            Error::Invalid(m) => Error::Invalid(format!("datum {letter}: {m}")),
            other => other,
        })?;
        ids.sort_unstable();
        out.push((
            letter,
            Datum {
                faces: ids,
                kind,
                existing: false,
            },
        ));
    }
    Ok(Datums(out))
}

/// The tolerance that relates datum B or C to the datums before it (`_datum_control`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    /// `perpendicularity` or `position`.
    pub kind: &'static str,
    pub datums: Vec<&'static str>,
    pub diametral: bool,
}

/// `_datum_control`: a tertiary hole is located from A|B; otherwise the feature is held square
/// to the earlier datums it is square to. None when there is no characteristic for the
/// geometry (a feature parallel to A would need parallelism).
pub fn datum_control(letter: char, datums: &Datums, faces: &Faces) -> Result<Option<Control>> {
    if !"BC".contains(letter) || !datums.contains(letter) || !datums.contains('A') {
        return Ok(None);
    }
    if letter == 'C' && !datums.contains('B') {
        return Ok(None);
    }
    let datum = datums.get(letter).expect("checked above");
    let axis = datum.kind == DatumKind::Axis;
    if letter == 'C' && axis {
        return Ok(Some(Control {
            kind: "position",
            datums: vec!["A", "B"],
            diametral: true,
        }));
    }
    let candidates: &[(char, &'static str)] = if letter == 'B' {
        &[('A', "A")]
    } else {
        &[('A', "A"), ('B', "B")]
    };
    let mut refs = Vec::new();
    for &(r, name) in candidates {
        let other = datums.get(r).expect("checked above");
        if square(datum, other, faces)? {
            refs.push(name);
        }
    }
    if refs.first() != Some(&"A") {
        return Ok(None);
    }
    Ok(Some(Control {
        kind: "perpendicularity",
        datums: refs,
        diametral: axis,
    }))
}

/// `_square`: planes by their normals; an axis to a plane when it runs along the normal.
fn square(feature: &Datum, datum: &Datum, faces: &Faces) -> Result<bool> {
    let first = |d: &Datum| -> Result<[f64; 3]> {
        let Some(&i) = d.faces.first() else {
            return invalid("a datum needs at least one face");
        };
        faces.at(i)?.direction()
    };
    let (u, v) = (first(feature)?, first(datum)?);
    Ok(if feature.kind == datum.kind {
        perpendicular(&u, &v)
    } else {
        parallel(&u, &v)
    })
}

/// `_datum_control_questions`: how well each datum feature must be made (flatness of A, the
/// control of B and C); not asked of a datum the file already tolerances.
pub(crate) fn datum_control_questions(ctx: &Ctx<'_>, datums: &Datums) -> Result<Vec<Question>> {
    let zones = || {
        [NONE, "0.01", "0.02", "0.05", "0.1"]
            .map(String::from)
            .to_vec()
    };
    let mut out = Vec::new();
    if let Some(a) = datums.get('A')
        && a.kind == DatumKind::Plane
        && !a.faces.iter().any(|i| ctx.toleranced.contains(i))
    {
        out.push(Question {
            default: Some(Value::from(NONE)),
            options: zones(),
            why: "Datum A is usually the face the part is clamped or mounted on.".to_string(),
            ..Question::new(
                "datum.A.flatness".to_string(),
                "choice",
                "Flatness required on datum A?".to_string(),
                a.faces.clone(),
            )
        });
    }
    for letter in ['B', 'C'] {
        let Some(control) = datum_control(letter, datums, &ctx.faces)? else {
            continue;
        };
        let datum = datums.get(letter).expect("a control has its datum");
        if datum.faces.iter().any(|i| ctx.toleranced.contains(i)) {
            continue;
        }
        let prompt = format!(
            "{} {}required on datum {letter}, relative to {}?",
            capitalize(control.kind),
            if control.diametral { "(Ø) " } else { "" },
            control.datums.join("|")
        );
        out.push(Question {
            default: Some(Value::from(NONE)),
            options: zones(),
            why: "Orients (and for a tertiary hole, locates) the datum feature, so the frame it \
                  builds is repeatable."
                .to_string(),
            ..Question::new(
                format!("datum.{letter}.control"),
                "choice",
                prompt,
                datum.faces.clone(),
            )
        });
    }
    Ok(out)
}

/// Python's `str.capitalize` of an ASCII word.
fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => format!(
            "{}{}",
            first.to_ascii_uppercase(),
            chars.as_str().to_ascii_lowercase()
        ),
        None => String::new(),
    }
}

/// `_frames`: the frames to offer, the primary frame then each extra datum in A's place; none
/// without A (a position needs a primary datum).
pub(crate) fn frames(letters: &[char]) -> Vec<String> {
    if !letters.contains(&'A') {
        return Vec::new();
    }
    let primary: Vec<char> = letters
        .iter()
        .copied()
        .filter(|x| "ABC".contains(*x))
        .collect();
    let join = |xs: &[char]| xs.iter().map(char::to_string).collect::<Vec<_>>().join("|");
    let mut out = vec![join(&primary)];
    for &x in letters {
        if EXTRA_DATUMS.contains(x) {
            let mut frame = vec![x];
            frame.extend_from_slice(&primary[1..]);
            out.push(join(&frame));
        }
    }
    out
}

/// `_datum_of`: the axis datum `bores` form, if the hole was chosen as one.
pub(crate) fn datum_of(bores: &[i64], datums: &Datums) -> Option<char> {
    if bores.is_empty() {
        return None;
    }
    datums
        .iter()
        .find(|(_, d)| d.kind == DatumKind::Axis && bores.iter().all(|i| d.faces.contains(i)))
        .map(|(x, _)| *x)
}

/// `_hole_frames`: the frames a hole can be positioned from. A datum hole whose control
/// locates or orients it gets none; one without a control keeps its position, from the frames
/// with its own letter taken out.
pub(crate) fn hole_frames(
    bores: &[i64],
    datums: &Datums,
    controls: &[char],
    frames: &[String],
) -> Vec<String> {
    let Some(letter) = datum_of(bores, datums) else {
        return frames.to_vec();
    };
    if controls.contains(&letter) {
        return Vec::new();
    }
    let own = letter.to_string();
    let mut out: Vec<String> = Vec::new();
    for frame in frames {
        let kept: Vec<&str> = frame.split('|').filter(|x| *x != own).collect();
        let kept = kept.join("|");
        if !kept.is_empty() && !out.contains(&kept) {
            out.push(kept);
        }
    }
    out
}
