//! Fold recognition measurements only after proving that their faces were trimmed by a new
//! turned chamfer. Raw correspondence (and answer migration) remains available unchanged.
use super::*;
use haecceity::brep::Edge;
use haecceity::geom::{self, Bounds, Curve, Surface, V3};
use haecceity::sampling::{edge_interval, extremes_along};

const TOL: f64 = 1e-5;
const DIRECTIONS: [V3; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];

fn parallel(a: V3, b: V3) -> bool {
    geom::norm(geom::cross(a, b)) < TOL
}

// Positions of planar faces are centroids, not plane offsets. Compare the actual supports.
fn same_support(a: &haecceity::brep::Face, b: &haecceity::brep::Face) -> bool {
    if a.reversed != b.reversed {
        return false;
    }
    match (&a.surface, &b.surface) {
        (Surface::Plane { frame: a }, Surface::Plane { frame: b }) => {
            geom::dist(a.z, b.z) < TOL && geom::dot(geom::sub(a.origin, b.origin), a.z).abs() < TOL
        }
        (
            Surface::Cylinder {
                frame: a,
                radius: r,
            },
            Surface::Cylinder {
                frame: b,
                radius: s,
            },
        ) => {
            (r - s).abs() < TOL
                && parallel(a.z, b.z)
                && geom::norm(geom::cross(geom::sub(a.origin, b.origin), a.z)) < TOL
        }
        _ => false,
    }
}

fn same_curve(a: &Curve, b: &Curve) -> bool {
    match (a, b) {
        (Curve::Line { origin: a, dir: u }, Curve::Line { origin: b, dir: v }) => {
            parallel(*u, *v) && geom::norm(geom::cross(geom::sub(*a, *b), *u)) < TOL
        }
        (
            Curve::Circle {
                frame: a,
                radius: r,
            },
            Curve::Circle {
                frame: b,
                radius: s,
            },
        ) => (r - s).abs() < TOL && geom::dist(a.origin, b.origin) < TOL && parallel(a.z, b.z),
        // Refuse unknown/approximated curves rather than infer that they were only trimmed.
        _ => false,
    }
}

fn interval(e: &Edge) -> (f64, f64) {
    let (a, b) = edge_interval(&e.curve, e.start, e.end, e.same_sense, e.is_closed());
    (a.min(b), a.max(b))
}

// A new non-chamfer boundary must be an unchanged portion of an old boundary. Every shortened
// endpoint must now meet the chamfer, and the old endpoint must lie in its local envelope.
fn retained_edge(
    old: &Edge,
    new: &Edge,
    on_chamfer: &impl Fn(V3) -> bool,
    region: &Bounds,
) -> bool {
    if !same_curve(&old.curve, &new.curve) {
        return false;
    }
    let (lo, hi) = interval(old);
    let (a, b) = interval(new);
    let mid = new.curve.value((a + b) / 2.);
    let unwrap = |p| {
        let t = old.curve.parameter(p);
        old.curve.period().map_or(t, |period| {
            t + (((lo + hi) / 2. - t) / period).round() * period
        })
    };
    let middle = unwrap(mid);
    let around_middle = |p| {
        let t = old.curve.parameter(p);
        old.curve
            .period()
            .map_or(t, |period| t + ((middle - t) / period).round() * period)
    };
    let (x, y) = (around_middle(new.start), around_middle(new.end));
    let (x, y) = (x.min(y), x.max(y));
    if x < lo - TOL || y > hi + TOL {
        return false;
    }
    // Full circles have coincident endpoints: only an equal interval can be retained here.
    if new.is_closed() {
        return old.is_closed() && ((b - a) - (hi - lo)).abs() < TOL;
    }
    if ((y - x) - (b - a)).abs() > TOL {
        return false;
    }
    [(lo, x), (hi, y)].iter().all(|&(was, now)| {
        (was - now).abs() < TOL
            || (region.contains(old.curve.value(was), TOL) && on_chamfer(old.curve.value(now)))
    })
}

fn trimmed_face(
    old: &Part,
    new: &Part,
    i: usize,
    j: usize,
    cone: usize,
    supporters: &BTreeSet<usize>,
) -> bool {
    if !same_support(&old.faces[i], &new.faces[j]) {
        return false;
    }
    let region = new.face_bounds(cone);
    let cone_edges = new.face_edges(cone);
    let vertices: Vec<_> = cone_edges
        .iter()
        .flat_map(|&e| [new.edges[e].start, new.edges[e].end])
        .collect();
    let on_chamfer = |p| vertices.iter().any(|&q| geom::dist(p, q) < TOL);
    let old_edges = old.face_edges(i);
    let new_edges = new.face_edges(j);
    let mut retained = BTreeSet::new();
    for e in new_edges {
        if cone_edges.contains(&e) {
            continue;
        }
        let Some(was) = old_edges
            .iter()
            .find(|&&o| retained_edge(&old.edges[o], &new.edges[e], &on_chamfer, &region))
        else {
            return false;
        };
        retained.insert(*was);
    }
    old_edges.iter().all(|&e| {
        if retained.contains(&e) {
            return true;
        }
        // An entire sharp edge can disappear only between two of the chamfer's supports,
        // within the chamfer's envelope. This excludes lost slots, holes and remote corners.
        let owners = &old.edge_faces()[e];
        owners.len() == 2 && owners.iter().all(|f| supporters.contains(f)) && {
            let edge = &old.edges[e];
            if !matches!(edge.curve, Curve::Line { .. } | Curve::Circle { .. }) {
                return false;
            }
            let (a, b) = interval(edge);
            [edge.start, edge.end]
                .into_iter()
                .chain(extremes_along(&edge.curve, (a, b), &DIRECTIONS))
                .all(|p| region.contains(p, TOL))
        }
    })
}

pub(super) fn fold_end_chamfers(
    changes: &mut Vec<FeatureChange>,
    old_features: &[Feature],
    new_features: &[Feature],
    old: &Part,
    new: &Part,
    corr: &Correspondence,
) {
    let chamfers: Vec<_> = new_features
        .iter()
        .filter(|f| {
            f.family == "chamfers"
                && f.record.get("turned") == Some(&Value::Bool(true))
                && f.faces.len() == 1
                && changes
                    .iter()
                    .any(|c| c.class == "new" && c.new.as_ref() == Some(&f.id))
        })
        .collect();
    if chamfers.is_empty() {
        return;
    }
    // All geometry tests take place in the new frame, including when the part moved.
    let a = &corr.alignment;
    let placement = std::array::from_fn(|i| {
        [
            a.rotation[i][0],
            a.rotation[i][1],
            a.rotation[i][2],
            a.translation[i],
        ]
    });
    let Ok(old) = old.transformed(&placement) else {
        return;
    };
    for chamfer in chamfers {
        let cone = chamfer.faces[0];
        if !matches!(new.faces[cone].surface, Surface::Cone { .. }) {
            continue;
        }
        let neighbours = new.neighbours(cone);
        let mut mapped = BTreeMap::new();
        for &j in &neighbours {
            let paired = corr.faces.iter().find(|e| {
                e.new == Some(j)
                    && matches!(e.class, Class::Carried | Class::Adapted)
                    && same_support(&old.faces[e.old], &new.faces[j])
            });
            if let Some(e) = paired {
                mapped.insert(e.old, j);
                continue;
            }
            let candidates: Vec<_> = old
                .faces
                .iter()
                .enumerate()
                .filter(|(i, f)| {
                    same_support(f, &new.faces[j])
                        && !corr.faces.iter().any(|e| e.old == *i && e.new.is_some())
                })
                .map(|(i, _)| i)
                .collect();
            if let [i] = candidates.as_slice() {
                mapped.insert(*i, j);
            }
        }
        // A cone must connect to a retained cylinder and end plane. Any unsupported neighbour
        // makes the proof incomplete (e.g. another blend or split coplanar faces).
        if mapped.len() != neighbours.len()
            || !neighbours
                .iter()
                .any(|&j| matches!(new.faces[j].surface, Surface::Cylinder { .. }))
            || !neighbours
                .iter()
                .any(|&j| matches!(new.faces[j].surface, Surface::Plane { .. }))
        {
            continue;
        }
        let supporters = mapped.keys().copied().collect();
        let mut proven: BTreeSet<_> = mapped
            .iter()
            .filter(|&(i, j)| trimmed_face(&old, new, *i, *j, cone, &supporters))
            .map(|(&i, _)| i)
            .collect();
        for e in &corr.faces {
            if e.class == Class::Carried
                && let Some(j) = e.new
                && same_support(&old.faces[e.old], &new.faces[j])
            {
                mapped.insert(e.old, j);
                proven.insert(e.old);
            }
        }
        for c in changes.iter_mut() {
            let Some(was) = old_features.iter().find(|f| c.old.as_ref() == Some(&f.id)) else {
                continue;
            };
            if was.faces.is_empty() || !was.faces.iter().all(|f| proven.contains(f)) {
                continue;
            }
            // Across/normal changes still report. A centroid shift is explained only
            // when the mapped flat owns exactly these proven retained faces.
            if c.family == "flats"
                && c.class == "adapted"
                && new_features.iter().any(|f| {
                    c.new.as_ref() == Some(&f.id)
                        && f.faces.len() == was.faces.len()
                        && was.faces.iter().all(|i| f.faces.contains(&mapped[i]))
                })
            {
                c.changes.remove("position");
            }
        }
        // Locate the chamfer by its cylinder rather than by a point on the conical face.
        if let Some(boss) = new_features
            .iter()
            .find(|f| f.family == "bosses" && f.faces.iter().any(|j| neighbours.contains(j)))
            && let Some(d) = boss.record.get("diameter").and_then(Value::as_f64)
            && let Some(c) = changes
                .iter_mut()
                .find(|c| c.new.as_ref() == Some(&chamfer.id))
        {
            c.place = Place {
                on: format!("on the end of the Ø{} boss", num(d)),
                ..Place::default()
            };
        }
    }
    changes.retain(|c| {
        !(c.class == "adapted" && c.changes.keys().all(|k| CONTEXT.contains(&k.as_str())))
    });
}
