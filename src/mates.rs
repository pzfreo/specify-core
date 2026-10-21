//! How the parts of an assembly fit together, as answers to suggest (port of `mates.py`).
//!
//! A part on its own says only what its holes could be: a Ø4.2 hole is an M5 tap drill or just a
//! hole. In an assembly the part next to it says more. Every cylindrical face of every part is
//! placed where each of its instances is, and two that share an axis and meet along it, in
//! different parts (or different instances of one), are a mate:
//!
//! * a tap drill under a clearance hole for the same screw: tapped, and clearance;
//! * two clearance holes for the same bolt: clearance, a bolt through both;
//! * a shaft in a bore of its own size: a fit, H7 in the bore and h6 on the shaft;
//! * a shaft at a thread's size in its tap drill: a thread, and tapped.
//!
//! Each is a suggestion with its reason, by face index of the part analysed ([`mates`]). A face
//! whose mates disagree gets none. The rules offer a suggestion as the default, to be checked;
//! nothing is decided from an assembly alone. (`mates.suggestion`, which the rules use to read
//! these, is ported with the rules.)
//!
//! A flat face against another part's -- the two facing each other in one plane, overlapping --
//! is a contact ([`contacts`]): what locates the part there, and so its likely primary datum.
//!
//! What differs from Python: the overlap area of two facing flat faces. Python takes it from
//! OpenCascade's face boolean (`BRepAlgoAPI_Common`); haecceity has none (upstream need U5), so
//! here both faces' boundary loops are intersected as polygons with holes in the faces' common
//! plane (`overlap`). Straight boundaries give the exact area and circular ones are sampled
//! finely from their exact curves (`edge_points`), so both agree with Python to the rounded
//! digit; other curves use haecceity's edge samples (within 2e-4 mm of the curve) and can
//! differ in the last digit. A loop that cannot be closed is refused: no contact is reported
//! for that pair, and [`contacts_with_refusals`] says why.
//!
//! Faces whose surface haecceity could not resolve (`LoadedPart::unresolved_faces`) take no part
//! in mates or contacts, and a loop that runs along an edge whose curve did not resolve (whose
//! samples are only its chord) is refused like an open one.

// Assembly geometry paths are unreachable from Session::open, which refuses multiple parts/placements.
use std::collections::BTreeSet;
use std::sync::OnceLock;

use haecceity::brep::Edge;
use haecceity::geom::{Bounds, Curve, Surface, V3, add, cross, dot, scale, sub};
use haecceity::sampling::edge_interval;
use haecceity::step::Placement;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use serde_json::{Map, Value, json};

use crate::part::LoadedPart;
use crate::pyfmt;
use crate::standards;

/// How far apart two axes, or the ends of two faces along them, may be and mate; also how far
/// apart two facing planes may be and touch.
pub const REACH: f64 = 0.01;
/// How far a shaft and a bore may differ in diameter and still be one size.
pub const SAME_SIZE: f64 = 0.05;
/// The least overlap, mm², that counts as a contact (Python's `props.Mass() > 1e-3`).
const LEAST_AREA: f64 = 1e-3;
/// How far a chord of a circular boundary may stray from the arc, as a fraction of its radius
/// (see `edge_points`): about 70 000 chords to a full circle, so the area lost to the chords
/// is about 4e-9 of the circle's.
const ARC_SAGITTA: f64 = 1e-9;

/// A cylindrical face of a part, in the part's own frame or placed.
#[derive(Clone, Debug)]
struct Cylinder {
    face: usize,
    /// A bore (the material outside it), not a shaft.
    bore: bool,
    diameter: f64,
    origin: V3,
    direction: V3,
    /// Where the face starts and ends along `direction` from `origin`.
    span: (f64, f64),
}

impl Cylinder {
    fn placed(&self, placement: &Placement) -> Cylinder {
        Cylinder {
            origin: place_point(placement, self.origin),
            direction: place_direction(placement, self.direction),
            ..self.clone()
        }
    }
}

/// Suggested answers for the `part`-th of `parts` (all of one assembly, as
/// [`crate::part::load_all`] gives them), from its mates: face index (as a string) ->
/// `{"value": answer, "reason": why}`, in the order the faces were first found mated, as
/// Python's dict is.
pub fn mates(parts: &[LoadedPart], part: usize) -> Map<String, Value> {
    let shapes: Vec<Vec<Cylinder>> = parts.iter().map(cylinders).collect();
    // Face -> its suggestions, in the order faces were first found (Python's dict of sets).
    let mut found: Vec<(usize, BTreeSet<(String, String)>)> = Vec::new();
    for (i, placement) in parts[part].placements.iter().enumerate() {
        let mine: Vec<Cylinder> = shapes[part].iter().map(|c| c.placed(placement)).collect();
        for (other, theirs) in shapes.iter().enumerate() {
            for (j, their_placement) in parts[other].placements.iter().enumerate() {
                if (other, j) == (part, i) {
                    continue;
                }
                let placed: Vec<Cylinder> =
                    theirs.iter().map(|c| c.placed(their_placement)).collect();
                let name = display_name(&parts[other], other);
                for a in &mine {
                    for b in &placed {
                        if !mated(a, b) {
                            continue;
                        }
                        let Some(suggestion) = suggest(a, b, &name) else {
                            continue;
                        };
                        match found.iter_mut().find(|(face, _)| *face == a.face) {
                            Some((_, set)) => {
                                set.insert(suggestion);
                            }
                            None => found.push((a.face, BTreeSet::from([suggestion]))),
                        }
                    }
                }
            }
        }
    }
    let mut out = Map::new();
    for (face, suggestions) in found {
        let values: BTreeSet<&str> = suggestions.iter().map(|(v, _)| v.as_str()).collect();
        if values.len() == 1 {
            // Python's min() of the (value, reason) tuples: the first in the ordered set.
            // values.len() == 1 proves suggestions contains at least one tuple.
            let (value, reason) = suggestions.into_iter().next().expect("one suggestion");
            out.insert(face.to_string(), json!({"value": value, "reason": reason}));
        }
    }
    out
}

/// The flat faces of the `part`-th of `parts` against another part's: face index (as a string)
/// -> `{"part": its name, "area": how much of the two overlap, rounded to 3 decimals}`, the
/// largest where a face meets several. A pair whose overlap cannot be measured is left out; use
/// [`contacts_with_refusals`] to report them.
pub fn contacts(parts: &[LoadedPart], part: usize) -> Map<String, Value> {
    contacts_with_refusals(parts, part).0
}

/// [`contacts`], and why each pair of facing, coplanar, overlapping faces that could not be
/// measured was left out (one line per pair and instance, in the order met).
pub fn contacts_with_refusals(
    parts: &[LoadedPart],
    part: usize,
) -> (Map<String, Value>, Vec<String>) {
    let flats: Vec<Vec<Plane>> = parts.iter().map(planes).collect();
    let mut out = Map::new();
    let mut refusals = Vec::new();
    for (i, placement) in parts[part].placements.iter().enumerate() {
        let mine: Vec<Placed> = flats[part].iter().map(|p| p.placed(placement)).collect();
        for (other, theirs) in flats.iter().enumerate() {
            for (j, their_placement) in parts[other].placements.iter().enumerate() {
                if (other, j) == (part, i) {
                    continue;
                }
                let name = display_name(&parts[other], other);
                for b in theirs.iter().map(|p| p.placed(their_placement)) {
                    for a in &mine {
                        let area = match touching(&parts[part], a, &parts[other], &b) {
                            Ok(area) => area,
                            Err(why) => {
                                refusals.push(format!(
                                    "face {} (instance {i}) against face {} of {name} \
                                     (instance {j}): {why}",
                                    a.plane.index, b.plane.index
                                ));
                                continue;
                            }
                        };
                        let key = a.plane.index.to_string();
                        // Python compares with the area already stored, which is rounded.
                        let best = out
                            .get(&key)
                            .and_then(|c: &Value| c["area"].as_f64())
                            .unwrap_or(0.0);
                        if area > 0.0 && area > best {
                            out.insert(
                                key,
                                json!({"part": name, "area": pyfmt::round_half_even(area, 3)}),
                            );
                        }
                    }
                }
            }
        }
    }
    (out, refusals)
}

/// Python's `parts[other].name or f"part {other}"`.
fn display_name(part: &LoadedPart, index: usize) -> String {
    if part.name.is_empty() {
        format!("part {index}")
    } else {
        part.name.clone()
    }
}

/// A planar face of a part, in the part's own frame.
#[derive(Debug)]
struct Plane {
    index: usize,
    /// The outward normal.
    normal: V3,
    /// A point of the plane (its frame's origin).
    point: V3,
    /// The plane's in-plane axes (its frame's x and y), for projecting into it.
    x: V3,
    y: V3,
    /// The face's axis-aligned box (haecceity's `face_bounds`).
    bounds: Bounds,
    /// The face's boundary loops (`boundary_loops`), made only for a face that touches another
    /// so far as its box: they are finely sampled, and most faces never need them.
    loops: OnceLock<Result<Vec<Vec<V3>>, String>>,
}

impl Plane {
    fn placed<'a>(&'a self, placement: &'a Placement) -> Placed<'a> {
        // The box placed: the placed box of its eight corners, which holds the placed face (a
        // looser box under a turn that is not a quarter turn, as OpenCascade's `Bnd_Box` is).
        let (lo, hi) = (self.bounds.min, self.bounds.max);
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for corner in 0..8 {
            let c = [0, 1, 2].map(|k| if corner >> k & 1 == 0 { lo[k] } else { hi[k] });
            let p = place_point(placement, c);
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        Placed {
            plane: self,
            placement,
            normal: place_direction(placement, self.normal),
            point: place_point(placement, self.point),
            x: place_direction(placement, self.x),
            y: place_direction(placement, self.y),
            min: min.map(|v| v - REACH),
            max: max.map(|v| v + REACH),
        }
    }
}

/// A [`Plane`] where one instance of its part is placed; the box is enlarged by `REACH`
/// (Python's `Bnd_Box` of the face, `Enlarge(REACH)`).
struct Placed<'a> {
    plane: &'a Plane,
    placement: &'a Placement,
    normal: V3,
    point: V3,
    x: V3,
    y: V3,
    min: V3,
    max: V3,
}

/// The planar faces of `loaded`, in face order, in the part's own frame.
fn planes(loaded: &LoadedPart) -> Vec<Plane> {
    let part = &loaded.part;
    let mut out = Vec::new();
    for (index, face) in part.faces.iter().enumerate() {
        if loaded.unresolved_faces.contains(&index) {
            continue;
        }
        let Surface::Plane { frame } = face.surface else {
            continue;
        };
        let normal = if face.reversed {
            scale(frame.z, -1.0)
        } else {
            frame.z
        };
        out.push(Plane {
            index,
            normal,
            point: frame.origin,
            x: frame.x,
            y: frame.y,
            bounds: part.face_bounds(index),
            loops: OnceLock::new(),
        });
    }
    out
}

/// Face `index`'s boundary loops as closed polylines (`edge_points`), each edge run in
/// its loop's direction; a loop of one vertex (no area) is left out. Refused when an edge's
/// curve did not resolve or a loop does not close (consecutive edges, or its last and first
/// points, more than `REACH` apart).
fn boundary_loops(loaded: &LoadedPart, index: usize) -> Result<Vec<Vec<V3>>, String> {
    let part = &loaded.part;
    let mut out = Vec::new();
    for (k, lp) in part.faces[index].loops.iter().enumerate() {
        if lp.edges.is_empty() {
            continue;
        }
        let mut points: Vec<V3> = Vec::new();
        for &(e, forward) in &lp.edges {
            if loaded.unresolved_edges.contains(&e) {
                return Err(format!(
                    "loop {k} of face {index} runs along edge {e}, whose curve did not resolve"
                ));
            }
            let samples = edge_points(&part.edges[e]);
            let run: Vec<V3> = if forward {
                samples
            } else {
                samples.into_iter().rev().collect()
            };
            let Some(&first) = run.first() else {
                return Err(format!("edge {e} of face {index} has no samples"));
            };
            match points.last() {
                Some(&last) if distance(last, first) > REACH => {
                    return Err(format!(
                        "loop {k} of face {index} does not close: edge {e} starts {:.3} mm \
                         from where the edge before it ends",
                        distance(last, first)
                    ));
                }
                Some(_) => points.extend_from_slice(&run[1..]),
                None => points.extend_from_slice(&run),
            }
        }
        let (first, last) = (points[0], points[points.len() - 1]);
        if distance(first, last) > REACH {
            return Err(format!(
                "loop {k} of face {index} does not close: it ends {:.3} mm from its start",
                distance(first, last)
            ));
        }
        if points.len() > 1 {
            points.pop();
        }
        out.push(points);
    }
    Ok(out)
}

/// Points along `edge`, start to end. A circular arc is sampled afresh from its exact curve,
/// finely enough that the chord strays from it by at most `ARC_SAGITTA` of its radius, so the
/// common area of two round-holed faces is exact to the rounded digit. Every
/// other edge uses haecceity's samples (within its chord tolerance, 2e-4 mm, of the curve):
/// fine for the straight edges contacts usually have, approximate on other curves.
fn edge_points(edge: &Edge) -> Vec<V3> {
    let Curve::Circle { .. } = edge.curve else {
        return edge.samples.clone();
    };
    let (t0, t1) = edge_interval(
        &edge.curve,
        edge.start,
        edge.end,
        edge.same_sense,
        edge.is_closed(),
    );
    // A chord over angle θ strays r·(1 - cos(θ/2)) ≈ r·θ²/8 from its arc.
    let step = (8.0 * ARC_SAGITTA).sqrt();
    let n = (((t1 - t0).abs() / step).ceil() as usize).max(1);
    let mut points: Vec<V3> = (0..=n)
        .map(|i| edge.curve.value(t0 + (t1 - t0) * i as f64 / n as f64))
        .collect();
    // The ends are the edge's own vertices, so loops join exactly.
    points[0] = edge.start;
    points[n] = edge.end;
    points
}

/// How much of `a` (of `pa`) and `b` (of `pb`) overlap, facing each other in one plane; 0 if
/// they do not, or if the overlap is under `LEAST_AREA`. An error when they face, are coplanar
/// and their boxes meet, but a face's loops could not be made.
fn touching(pa: &LoadedPart, a: &Placed, pb: &LoadedPart, b: &Placed) -> Result<f64, String> {
    if dot(a.normal, b.normal) > -1.0 + 1e-6 {
        return Ok(0.0);
    }
    if dot(sub(b.point, a.point), a.normal).abs() > REACH {
        return Ok(0.0);
    }
    if (0..3).any(|k| a.min[k] > b.max[k] || b.min[k] > a.max[k]) {
        return Ok(0.0);
    }
    let loops_a = a
        .plane
        .loops
        .get_or_init(|| boundary_loops(pa, a.plane.index));
    let loops_b = b
        .plane
        .loops
        .get_or_init(|| boundary_loops(pb, b.plane.index));
    let (loops_a, loops_b) = (loops_a.as_ref()?, loops_b.as_ref()?);
    let area = overlap(a, loops_a, b, loops_b);
    Ok(if area > LEAST_AREA { area } else { 0.0 })
}

/// The U5 stand-in for `BRepAlgoAPI_Common` and its surface area: both faces' loops placed,
/// projected into `a`'s plane (its frame's x and y) and intersected as polygons with holes,
/// outer and inner loops alike under the even-odd rule (a face's holes lie inside its outer
/// loop, so the loops' directions do not matter); the area of the result.
fn overlap(a: &Placed, loops_a: &[Vec<V3>], b: &Placed, loops_b: &[Vec<V3>]) -> f64 {
    let project = |placement: &Placement, loops: &[Vec<V3>]| -> Vec<Vec<[f64; 2]>> {
        loops
            .iter()
            .map(|l| {
                l.iter()
                    .map(|&p| {
                        let d = sub(place_point(placement, p), a.point);
                        [dot(d, a.x), dot(d, a.y)]
                    })
                    .collect()
            })
            .collect()
    };
    let subject = project(a.placement, loops_a);
    let clip = project(b.placement, loops_b);
    let shapes = subject.overlay(&clip, OverlayRule::Intersect, FillRule::EvenOdd);
    // Each shape is an outer contour and its holes: the outer's area less the holes'.
    shapes
        .iter()
        .map(|shape| {
            let mut contours = shape.iter().map(|c| shoelace(c).abs());
            let outer = contours.next().unwrap_or(0.0);
            outer - contours.sum::<f64>()
        })
        .sum()
}

/// The signed area of a closed polygon (last point not repeated).
fn shoelace(points: &[[f64; 2]]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (p, q) = (points[i], points[(i + 1) % n]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum::<f64>()
        / 2.0
}

/// The cylindrical faces of `loaded`, in face order, in the part's own frame. A face whose
/// parameter range haecceity cannot give (so no span along its axis) is left out, as an
/// unresolved face is.
fn cylinders(loaded: &LoadedPart) -> Vec<Cylinder> {
    let part = &loaded.part;
    let mut out = Vec::new();
    for (index, face) in part.faces.iter().enumerate() {
        if loaded.unresolved_faces.contains(&index) {
            continue;
        }
        let Surface::Cylinder { frame, radius } = face.surface else {
            continue;
        };
        let Some((u0, u1, v0, v1)) = part.uv_bounds(index) else {
            continue;
        };
        // The face's normal against the way out from the axis, at the middle of its parameter
        // range: in, for a bore. (A cylinder's v is measured along its axis from the frame's
        // origin, as OpenCascade's is from the axis location.)
        let (u, v) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
        let point = face.surface.value(u, v);
        let (du, dv) = face.surface.partials(u, v);
        let mut normal = cross(du, dv);
        if face.reversed {
            normal = scale(normal, -1.0);
        }
        let (o, d) = (frame.origin, frame.z);
        let w = sub(point, o);
        let out_from_axis = sub(w, scale(d, dot(w, d)));
        out.push(Cylinder {
            face: index,
            bore: dot(normal, out_from_axis) < 0.0,
            diameter: 2.0 * radius,
            origin: o,
            direction: d,
            span: (v0, v1),
        });
    }
    out
}

/// Whether `a` and `b` share an axis and meet along it.
fn mated(a: &Cylinder, b: &Cylinder) -> bool {
    let along = dot(a.direction, b.direction);
    if along.abs() < 1.0 - 1e-6 {
        return false;
    }
    let w = sub(b.origin, a.origin);
    let t = dot(w, a.direction);
    let off = sub(w, scale(a.direction, t));
    if dot(off, off).sqrt() > REACH {
        return false;
    }
    let (p, q) = (t + along * b.span.0, t + along * b.span.1);
    let (lo, hi) = if p <= q { (p, q) } else { (q, p) };
    (lo - a.span.1).max(a.span.0 - hi) <= REACH
}

/// What `a` is, from `b` (of the part `name`) on its axis: (value, reason), the reason text
/// exactly Python's.
fn suggest(a: &Cylinder, b: &Cylinder, name: &str) -> Option<(String, String)> {
    let db = pyfmt::fmt3(b.diameter);
    if a.bore && b.bore {
        let tapped = standards::match_size(a.diameter, &standards::TAP_DRILL);
        if let Some(tapped) = tapped
            && standards::clearance_bolts(b.diameter).contains(&tapped)
        {
            return Some((
                format!("tapped:{tapped}"),
                format!(
                    "it lines up with the ⌀{db} {tapped} clearance hole in {name}: \
                     a screw through that threads in here"
                ),
            ));
        }
        let screw = standards::match_size(b.diameter, &standards::TAP_DRILL);
        if let Some(screw) = screw
            && standards::clearance_bolts(a.diameter).contains(&screw)
        {
            return Some((
                format!("clearance:{screw}"),
                format!(
                    "it lines up with the {screw} tap drill in {name}: \
                     a screw passes through it into that"
                ),
            ));
        }
        let theirs = standards::clearance_bolts(b.diameter);
        // Python's max() over the set of bolts both clear; bolt diameters are distinct, so the
        // set's order does not matter.
        let bolt = standards::clearance_bolts(a.diameter)
            .into_iter()
            .filter(|bolt| theirs.contains(bolt))
            .max_by(|x, y| standards::bolt_diameter(x).total_cmp(&standards::bolt_diameter(y)))?;
        return Some((
            format!("clearance:{bolt}"),
            format!("it lines up with the ⌀{db} hole in {name}: a {bolt} bolt passes through both"),
        ));
    }
    if a.bore {
        // b is a shaft in it.
        if (a.diameter - b.diameter).abs() <= SAME_SIZE {
            return Some((
                "fit:H7".into(),
                format!("the ⌀{db} shaft of {name} fits in it: H7 assumed, with h6 on the shaft"),
            ));
        }
        let thread = standards::thread_size(b.diameter)?;
        if standards::match_size(a.diameter, &standards::TAP_DRILL) == Some(thread) {
            return Some((
                format!("tapped:{thread}"),
                format!("the {thread} shaft of {name} screws into it"),
            ));
        }
        if standards::clearance_bolts(a.diameter).contains(&thread) {
            return Some((
                format!("clearance:{thread}"),
                format!("the ⌀{db} shaft of {name} passes through it"),
            ));
        }
        return None;
    }
    if b.bore {
        // a is a shaft in b.
        if (a.diameter - b.diameter).abs() <= SAME_SIZE {
            return Some((
                "h6".into(),
                format!("it fits the ⌀{db} bore in {name}: h6 assumed, with H7 in the bore"),
            ));
        }
        let thread = standards::thread_size(a.diameter)?;
        if standards::match_size(b.diameter, &standards::TAP_DRILL) == Some(thread) {
            return Some((
                format!("thread:{thread}"),
                format!("it screws into the {thread} tap drill in {name}"),
            ));
        }
    }
    None
}

/// `p` placed: `R p + t`.
fn place_point(placement: &Placement, p: V3) -> V3 {
    add(place_direction(placement, p), placement.map(|row| row[3]))
}

/// `d` turned: `R d`.
fn place_direction(placement: &Placement, d: V3) -> V3 {
    placement.map(|row| row[0] * d[0] + row[1] * d[1] + row[2] * d[2])
}

fn distance(a: V3, b: V3) -> f64 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unit_square_has_unit_area_either_way_round() {
        let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        assert_eq!(shoelace(&square), 1.0);
        let mut reversed = square;
        reversed.reverse();
        assert_eq!(shoelace(&reversed), -1.0);
    }

    #[test]
    fn placing_turns_then_moves() {
        // A quarter turn about z, then 5 along x.
        let placement = [
            [0.0, -1.0, 0.0, 5.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        assert_eq!(place_point(&placement, [1.0, 0.0, 0.0]), [5.0, 1.0, 0.0]);
        assert_eq!(
            place_direction(&placement, [1.0, 0.0, 0.0]),
            [0.0, 1.0, 0.0]
        );
    }

    fn cylinder(bore: bool, diameter: f64, z: f64, span: (f64, f64)) -> Cylinder {
        Cylinder {
            face: 0,
            bore,
            diameter,
            origin: [0.0, 0.0, z],
            direction: [0.0, 0.0, 1.0],
            span,
        }
    }

    #[test]
    fn coaxial_faces_mate_only_where_they_meet_along_the_axis() {
        let a = cylinder(true, 8.0, 0.0, (0.0, 10.0));
        assert!(mated(&a, &cylinder(false, 8.0, 10.0, (0.0, 6.0))));
        assert!(mated(&a, &cylinder(false, 8.0, 10.005, (0.0, 6.0))));
        assert!(!mated(&a, &cylinder(false, 8.0, 10.02, (0.0, 6.0))));
        let mut off_axis = cylinder(false, 8.0, 0.0, (0.0, 6.0));
        off_axis.origin = [0.02, 0.0, 0.0];
        assert!(!mated(&a, &off_axis));
    }

    #[test]
    fn suggestions_read_as_python_writes_them() {
        let tap = cylinder(true, 4.2, 0.0, (0.0, 10.0));
        let clearance = cylinder(true, 5.5, 10.0, (0.0, 6.0));
        assert_eq!(
            suggest(&tap, &clearance, "cover"),
            Some((
                "tapped:M5".into(),
                "it lines up with the ⌀5.5 M5 clearance hole in cover: a screw through that \
                 threads in here"
                    .into()
            ))
        );
        let shaft = cylinder(false, 5.0, 0.0, (0.0, 10.0));
        assert_eq!(
            suggest(&shaft, &tap, "base"),
            Some((
                "thread:M5".into(),
                "it screws into the M5 tap drill in base".into()
            ))
        );
        // Two holes no bolt clears both of: nothing.
        let wide = cylinder(true, 30.0, 0.0, (0.0, 10.0));
        assert_eq!(suggest(&wide, &clearance, "x"), None);
    }
}
