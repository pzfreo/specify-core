//! Where a label's leader lands on the part, and which way it leaves (port of `anchors.py`).
//!
//! For each set of faces a label is about: the largest of them, a point on its real trim, and a
//! direction whose leader leaves through free space rather than through the part
//! ([`crate::sightlines`]). A flat face or a shaft is read from its outward normal; a hole from
//! along its axis, out of either end, since a leader straight off a bore's wall would cross the
//! hole into its far side.
//!
//! What differs from Python:
//!
//! - The point. Python takes Quiddity's `inspect_face(face).anchor`, a point proved to lie on the
//!   face's trim. haecceity has no such point (upstream need U6), so the stand-in is
//!   `Part::face_centre` (build123d's `Face.center()`) when the face's domain contains it, else
//!   the parameter-space centroid of the face's largest triangle ([`crate::triangulate`]) mapped
//!   onto the surface, else no anchor (the page falls back to its own guess). The points are
//!   therefore not Python's; both lie on the same face.
//! - The reach. It is [`REACH`] of the caller's diagonal; the Rust caller passes the exact box's
//!   ([`crate::mesh::diagonal`]) where Python's `serve._diagonal` is OpenCascade's loose
//!   `BRepBndLib` box, so a hole's anchor, moved to its rim, and every leader's clear run scale by
//!   the ratio of the two diagonals.
//! - Trust. A part rebuilt by the U1 stand-in with unresolved faces or edges
//!   ([`LoadedPart::degraded`]) has no trustworthy solid to classify against, so none of its
//!   anchors is reported clear. A face whose surface did not resolve gets no anchor, and nor does
//!   a group whose largest face is such a face, or has a face whose area haecceity cannot
//!   measure (Python's areas are OpenCascade's, always known): the largest face is then not
//!   known, and a smaller one is not anchored in its place.

use haecceity::geom::{Surface, V3};
use serde_json::{Map, Value, json};

use crate::mesh;
/// The exact box's diagonal that scales the reach (the mesh's, `mesh::diagonal`).
pub use crate::mesh::diagonal;
use crate::part::LoadedPart;
use crate::pyfmt::round_half_even;
use crate::sightlines::{SightTester, normalise};

/// How far a leader must run clear, as a share of the part's diagonal.
pub const REACH: f64 = 0.12;

/// The key a set of faces' anchor is found by: its ids, sorted.
pub fn key(faces: &[usize]) -> String {
    let mut sorted = faces.to_vec();
    sorted.sort_unstable();
    sorted
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// An anchor (`{at, out, clear}`, rounded to 6 decimals) for each set of face ids in `groups`
/// that has one, keyed by [`key`]. `diagonal` scales the leaders' reach.
pub fn anchors(loaded: &LoadedPart, groups: &[Vec<usize>], diagonal: f64) -> Map<String, Value> {
    let tester = SightTester::new(&loaded.part, diagonal * REACH);
    let trusted = loaded.degraded().is_none();
    let mut out = Map::new();
    for faces in groups {
        let k = key(faces);
        if faces.is_empty() || out.contains_key(&k) {
            continue;
        }
        if let Some(found) = anchor(loaded, faces, &tester, trusted) {
            out.insert(k, found);
        }
    }
    out
}

fn anchor(
    loaded: &LoadedPart,
    faces: &[usize],
    tester: &SightTester<'_>,
    trusted: bool,
) -> Option<Value> {
    let part = &loaded.part;
    let unresolved =
        |i: usize| loaded.unresolved_faces.contains(&i) || part.unresolved_faces().contains(&i);
    // The largest face, the first of equals (Python's `max`), over every face of the group in
    // range, as Python's: an unresolved face is not passed over for a smaller one. If it is the
    // largest, or a face's area cannot be measured (so the largest is not known), the group
    // gets no anchor rather than one on a face Python would not have chosen.
    let mut face = None;
    let mut largest = f64::NEG_INFINITY;
    for &i in faces.iter().filter(|&&i| i < loaded.binding.face_count) {
        let area = part
            .face_moments(i)
            .map(|m| m.area)
            .or_else(|| part.face_mass(i).map(|m| m[0]))?;
        if face.is_none() || area > largest {
            (face, largest) = (Some(i), area);
        }
    }
    if face.is_some_and(unresolved) {
        return None;
    }
    let face = face?;
    let (at, uv) = point_on(loaded, face)?;
    leader(loaded, face, at, uv, tester, trusted)
}

/// The anchor of `face` landing at `at` (a point on it, with its parameters `uv`): the leader
/// from there, and for a hole the rim it leaves by.
fn leader(
    loaded: &LoadedPart,
    face: usize,
    at: V3,
    (u, v): (f64, f64),
    tester: &SightTester<'_>,
    trusted: bool,
) -> Option<Value> {
    let part = &loaded.part;
    let normal = normalise(part.face_normal(face, u, v)?)?;
    let mut preferred = vec![normal];
    let mut hole = None;
    if let Surface::Cylinder { frame, .. } = part.faces[face].surface
        && let Some(axis) = normalise(frame.z)
        && inward(frame.origin, axis, at, normal)
    {
        // A hole's normal points at its own axis: read it from an end instead. (Not tested
        // against the face's centre: a bore's can lie on its wall.)
        hole = Some((frame.origin, axis));
        preferred = vec![axis, [-axis[0], -axis[1], -axis[2]], normal];
    }
    let (direction, clear) = tester.choose(at, &preferred);
    let at = match hole {
        // Landing part way down the bore, a label a short way out would still be in the hole,
        // hidden by its walls: it lands on the rim it leaves by.
        Some((origin, axis)) => rim(loaded, face, origin, axis, at, direction),
        None => at,
    };
    let round = |p: V3| p.map(|x| round_half_even(x, 6)).to_vec();
    Some(json!({
        "at": round(at),
        "out": round(direction),
        "clear": clear && trusted,
    }))
}

/// The anchor `face` would have if it landed at `at` (a point on its surface), with leaders
/// `diagonal * REACH` long: for comparing with another engine's anchor point, the point being
/// the one thing the U6 stand-in chooses differently. None where the face has no normal there.
pub fn anchor_from_point(loaded: &LoadedPart, face: usize, at: V3, diagonal: f64) -> Option<Value> {
    let uv = loaded.part.faces[face].surface.parameters(at, None)?;
    let tester = SightTester::new(&loaded.part, diagonal * REACH);
    leader(loaded, face, at, uv, &tester, loaded.degraded().is_none())
}

/// A point on `face`'s trimmed region and its parameters (the U6 stand-in): the face's centre
/// when its domain contains it, else the parameter centroid of its largest triangle on the
/// surface, else None.
fn point_on(loaded: &LoadedPart, face: usize) -> Option<(V3, (f64, f64))> {
    let part = &loaded.part;
    let surface = &part.faces[face].surface;
    let domain = part.domain(face)?;
    if let Some(centre) = part.face_centre(face)
        && let Some((u, v)) = surface.parameters(centre, None)
        && domain.contains(u, v)
    {
        return Some((centre, (u, v)));
    }
    let triangles = mesh::face_mesh(loaded, face, mesh::deflection(loaded)).ok()?;
    let mut largest = None;
    let mut area = 0.0;
    for t in 0..triangles.triangles.len() {
        let a = triangles.triangle_area(t);
        if a > area {
            (largest, area) = (Some(t), a);
        }
    }
    let corners = triangles.triangles[largest?].map(|k| triangles.uv[k]);
    let u = corners.iter().map(|c| c.0).sum::<f64>() / 3.0;
    let v = corners.iter().map(|c| c.1).sum::<f64>() / 3.0;
    domain.contains(u, v).then(|| (surface.value(u, v), (u, v)))
}

/// Whether `normal` at `at` on a cylinder (axis through `origin` along unit `axis`) points
/// toward its axis: a hole.
fn inward(origin: V3, axis: V3, at: V3, normal: V3) -> bool {
    let rel = [at[0] - origin[0], at[1] - origin[1], at[2] - origin[2]];
    let v = rel[0] * axis[0] + rel[1] * axis[1] + rel[2] * axis[2];
    let radial = [
        rel[0] - v * axis[0],
        rel[1] - v * axis[1],
        rel[2] - v * axis[2],
    ];
    radial[0] * normal[0] + radial[1] * normal[1] + radial[2] * normal[2] < 0.0
}

/// `at` slid along a cylinder's axis to the end of the face `direction` leaves by; `at` itself
/// if `direction` is not along the axis (or the face's range is not known).
fn rim(loaded: &LoadedPart, face: usize, origin: V3, axis: V3, at: V3, direction: V3) -> V3 {
    let along = direction[0] * axis[0] + direction[1] * axis[1] + direction[2] * axis[2];
    if (along.abs() - 1.0).abs() > 1e-6 {
        return at;
    }
    let Some((_, _, vmin, vmax)) = loaded.part.uv_bounds(face) else {
        return at;
    };
    let v = (at[0] - origin[0]) * axis[0]
        + (at[1] - origin[1]) * axis[1]
        + (at[2] - origin[2]) * axis[2];
    let run = if along > 0.0 { vmax - v } else { v - vmin };
    [
        at[0] + direction[0] * run,
        at[1] + direction[1] * run,
        at[2] + direction[2] * run,
    ]
}
