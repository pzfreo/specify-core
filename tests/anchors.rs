//! Label anchors and their leaders (`src/anchors.rs`, `src/sightlines.rs`, ports of
//! `anchors.py` and `sightlines.py`): specify-core's `tests/test_anchors.py` (its preview and
//! serve tests are stage 4's), and every captured case's anchors against the oracle's.
//!
//! Anchor points are not Python's (Python's come from Quiddity's `inspect_face`, the Rust ones
//! from the U6 stand-in), so an anchor is compared by what it means: whether its leader is
//! clear, which way it leaves, and which face of its group its point lies on. The diagonal that
//! scales the reach is compared too (Python's is OpenCascade's `BRepBndLib` box, Rust's the exact
//! box).

mod common;

use std::path::{Path, PathBuf};

use common::{
    Difference, Tolerances, cases, check_known_with, compare, glob_match, is_error,
    known_differences_file, load_known,
};
use haecceity::geom::{self, V3};
use serde_json::{Map, Value, json};
use specify_core::anchors::{anchor_from_point, anchors, key};
use specify_core::mesh;
use specify_core::part::{self, LoadedPart};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parts")
        .join(name)
}

fn point(value: &Value) -> V3 {
    let p: Vec<f64> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect();
    [p[0], p[1], p[2]]
}

/// The first face of `kind` (and `radius`, if given) in the oracle's analysis of `case`.
fn face(case: &str, kind: &str, radius: Option<f64>) -> usize {
    let analysis = common::case(case).artifact("analysis").unwrap();
    analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| {
            f["kind"] == kind
                && radius.is_none_or(|r| (f["radius"].as_f64().unwrap_or(0.0) - r).abs() < 1e-6)
        })
        .map(|f| f["id"].as_u64().unwrap() as usize)
        .unwrap()
}

#[test]
fn a_shaft_is_labelled_from_its_side_and_a_hole_from_its_open_end() {
    let loaded = part::load(&fixture("pin.step"), None).unwrap();
    let shaft = face("parts/pin", "cylinder", Some(3.0));
    let hole = face("parts/pin", "cylinder", Some(2.1));
    let found = anchors(&loaded, &[vec![shaft], vec![hole]], 40.0);
    let (s, h) = (&found[&key(&[shaft])], &found[&key(&[hole])]);
    // The pin stands along Z: the shaft's leader leaves sideways, square to it.
    let (s_at, s_out) = (point(&s["at"]), point(&s["out"]));
    assert!(s_out[2].abs() < 1e-6 && s["clear"] == true, "{s}");
    assert!((s_at[0].hypot(s_at[1]) - 3.0).abs() < 1e-3, "{s}"); // on its surface
    // The tapped hole opens at the top (Z = 32): its leader leaves up, out of it, from its rim,
    // so the label is not left inside the hole.
    let h_at = point(&h["at"]);
    assert_eq!(h["out"], json!([0.0, 0.0, 1.0]));
    assert_eq!(h["clear"], true);
    assert!((h_at[2] - 32.0).abs() < 1e-6, "{h}");
    assert!((h_at[0].hypot(h_at[1]) - 2.1).abs() < 1e-3, "{h}");
}

/// As in a turned part lying along X: the bore's centre, as build123d gives it, lies on its
/// wall, so a hole must be told by its normal and axis.
#[test]
fn a_hole_along_x_is_labelled_from_its_rim() {
    let name = "a_hole_along_x_is_labelled_from_its_rim";
    let loaded = part::load(&fixture(&format!("{name}.step")), None).unwrap();
    let case = format!("parts/{name}");
    let hole = face(&case, "cylinder", Some(0.8));
    let found = &anchors(&loaded, &[vec![hole]], 21.0)[&key(&[hole])];
    // The mouth: the face square to X with the least centre X (Python's `filter_by(Axis.X)`).
    let analysis = common::case(&case).artifact("analysis").unwrap();
    let mouth = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "plane" && point(&f["direction"])[0].abs() > 1.0 - 1e-9)
        .map(|f| point(&f["centroid"])[0])
        .fold(f64::INFINITY, f64::min);
    let (at, out) = (point(&found["at"]), point(&found["out"]));
    assert!(
        (out[0].abs() - 1.0).abs() < 1e-9 && found["clear"] == true,
        "{found}"
    );
    assert!(
        (at[0] - mouth).abs() < 1e-6,
        "{found}, mouth at x = {mouth}"
    );
}

#[test]
fn keys_are_sorted_ids() {
    assert_eq!(key(&[3, 1, 12]), "1,3,12");
    assert_eq!(key(&[]), "");
}

#[test]
fn an_empty_group_and_a_repeated_one_are_skipped() {
    let loaded = part::load(&fixture("pin.step"), None).unwrap();
    let found = anchors(&loaded, &[vec![], vec![1, 3], vec![3, 1], vec![99]], 40.0);
    assert_eq!(found.keys().collect::<Vec<_>>(), vec!["1,3"]);
}

/// An unresolved face gets no anchor, nor does a group whose largest face it is (a smaller face
/// is not anchored in its place); a part whose solid is not trusted (rebuilt with unresolved
/// faces, `LoadedPart::degraded`) has no clear leader.
#[test]
fn untrusted_geometry_gives_no_anchor_or_no_clear_leader() {
    let mut loaded = part::load(&fixture("pin.step"), None).unwrap();
    let area = |f: usize| loaded.part.face_moments(f).unwrap().area;
    let (big, small) = if area(1) > area(0) { (1, 0) } else { (0, 1) };
    loaded.unresolved_faces.push(big);
    assert!(anchors(&loaded, &[vec![big]], 40.0).is_empty());
    assert!(anchors(&loaded, &[vec![small, big]], 40.0).is_empty());
    assert_eq!(anchors(&loaded, &[vec![small]], 40.0).len(), 1);
    loaded.unresolved_faces.clear();
    loaded.unresolved_faces.push(1);
    loaded.rebuilt = true;
    assert!(loaded.degraded().is_some());
    let found = anchors(&loaded, &[vec![0], vec![3]], 40.0);
    assert_eq!(found.len(), 2);
    assert!(found.values().all(|a| a["clear"] == false), "{found:?}");
}

#[test]
fn anchors_are_deterministic() {
    let loaded = part::load(&fixture("plate.step"), None).unwrap();
    let groups: Vec<Vec<usize>> = (0..loaded.binding.face_count).map(|f| vec![f]).collect();
    let d = mesh::diagonal(&loaded);
    let a = serde_json::to_string(&anchors(&loaded, &groups, d)).unwrap();
    let again = part::load(&fixture("plate.step"), None).unwrap();
    assert_eq!(
        a,
        serde_json::to_string(&anchors(&again, &groups, d)).unwrap()
    );
}

/// The distance from `p` to the polyline.
fn polyline_distance(p: V3, line: &[V3]) -> f64 {
    line.windows(2)
        .map(|w| {
            let ab = geom::sub(w[1], w[0]);
            let len2 = geom::dot(ab, ab);
            let t = if len2 > 0.0 {
                (geom::dot(geom::sub(p, w[0]), ab) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            geom::dist(p, geom::add(w[0], geom::scale(ab, t)))
        })
        .fold(f64::INFINITY, f64::min)
}

/// Whether `p` (rounded to 6 decimals) lies on face `f`: on its surface and in its trimmed
/// domain, or on its boundary (a hole's anchor is moved to its rim).
fn on_face(loaded: &LoadedPart, f: usize, p: V3) -> bool {
    let part = &loaded.part;
    let surface = &part.faces[f].surface;
    let Some((u, v)) = surface.parameters(p, None) else {
        return false;
    };
    let tol = 1e-5 * (1.0 + geom::norm(p));
    if geom::dist(surface.value(u, v), p) > tol {
        return false;
    }
    part.domain(f).is_some_and(|d| d.contains(u, v))
        || part
            .face_edges(f)
            .iter()
            .any(|&e| polyline_distance(p, &part.edges[e].samples) <= tol + 2e-4)
}

/// An anchor by what it means: clear or not, its direction, and the first face of its group its
/// point lies on (null for none).
fn meaning(loaded: &LoadedPart, group: &[usize], anchor: &Value) -> Value {
    let at = point(&anchor["at"]);
    let on = group
        .iter()
        .copied()
        .find(|&f| f < loaded.binding.face_count && on_face(loaded, f, at));
    json!({"clear": anchor["clear"], "out": anchor["out"], "face": on})
}

/// The groups `serve._label_anchors` anchors for a preview: its datums' and requirements'
/// faces, then the existing PMI's.
fn groups(analysis: &Value, preview: &Value) -> Vec<Vec<usize>> {
    let faces = |items: &Value| -> Vec<Vec<usize>> {
        items
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|i| {
                        i["faces"]
                            .as_array()
                            .map(|f| f.iter().map(|x| x.as_u64().unwrap() as usize).collect())
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut out = faces(&preview["datums"]);
    out.extend(faces(&preview["requirements"]));
    out.extend(faces(&analysis["existing"]));
    out.retain(|g| !g.is_empty());
    out
}

/// Every captured case's anchors against the oracle's.
#[test]
fn anchors_match_the_oracle() {
    let tolerances = Tolerances::default().with("anchors.*.out*", 0.0, 1e-6);
    let mut differences = Vec::new();
    let (mut compared, mut anchored) = (Vec::new(), 0);
    for case in cases() {
        let Some(python) = case.artifact("anchors") else {
            continue;
        };
        assert!(!is_error(&python), "{}: Python failed: {python}", case.name);
        let (Some(analysis), Some(preview)) = (case.artifact("analysis"), case.artifact("preview"))
        else {
            continue;
        };
        let Some(input) = case.input() else { continue };
        compared.push(case.name.clone());
        let loaded = match part::load(&input, case.part()) {
            Ok(loaded) => loaded,
            Err(e) => {
                differences.push(Difference {
                    case: case.name.clone(),
                    path: String::new(),
                    python: Some(python),
                    rust: Some(json!({"error": e.to_string()})),
                });
                continue;
            }
        };
        if let Some(why) = loaded.degraded() {
            println!("{}: degraded, no leader reported clear: {why}", case.name);
        }
        let groups = groups(&analysis, &preview);
        let diagonal = mesh::diagonal(&loaded);
        let rust = anchors(&loaded, &groups, diagonal);
        anchored += rust.len();
        let by_key: Map<String, Value> = groups.iter().map(|g| (key(g), json!(g))).collect();
        let means = |found: &Map<String, Value>| -> Map<String, Value> {
            found
                .iter()
                .map(|(k, a)| {
                    let group: Vec<usize> = by_key
                        .get(k)
                        .and_then(|g| g.as_array())
                        .map(|g| g.iter().map(|x| x.as_u64().unwrap() as usize).collect())
                        .unwrap_or_default();
                    (k.clone(), meaning(&loaded, &group, a))
                })
                .collect()
        };
        let python_diagonal = python["diagonal"].as_f64().unwrap();
        if (python_diagonal - diagonal).abs() > 1e-9 * diagonal {
            println!(
                "{}: diagonal Python {python_diagonal} Rust {diagonal}, ratio {}",
                case.name,
                python_diagonal / diagonal
            );
        }
        let python_anchors = python["anchors"].as_object().unwrap().clone();
        let python_means = means(&python_anchors);
        let rust_means = means(&rust);
        let python = json!({"diagonal": python_diagonal, "anchors": python_means});
        let rust = json!({"diagonal": diagonal, "anchors": rust_means});
        for mut difference in compare(&case.name, &python, &rust, &tolerances) {
            explain(
                &loaded,
                &python_anchors,
                &python["anchors"],
                &rust["anchors"],
                python_diagonal,
                &mut difference,
            );
            differences.push(difference);
        }
    }
    println!("compared {} cases, {anchored} anchors", compared.len());
    // Entries for cases not compared in this run (corpus absent) are left out of the stale check.
    let path = known_differences_file("anchors");
    let entries: Vec<_> = load_known(&path)
        .into_iter()
        .filter(|e| compared.iter().any(|c| glob_match(&e.case, c)))
        .collect();
    check_known_with(&path.display().to_string(), &entries, &differences);
}

/// A face's exact area (haecceity's), if known.
fn area(loaded: &LoadedPart, face: usize) -> Option<f64> {
    loaded.part.face_moments(face).map(|m| m.area)
}

/// Classifies one anchor difference by its cause, appending to its path:
///
/// - `.equal_area_face`: the two engines anchored on different faces of the group whose areas
///   are equal to 1e-6 (identical features; which counts as largest is rounding); for a clear
///   flag or direction, only where Rust's leader from Python's face and point is Python's;
/// - `.same_from_python_point`: Rust's leader from Python's anchor point (and with Python's
///   diagonal) is Python's, so the difference is the anchor point's (upstream need U6);
/// - `.opposite_axis`: both leaders are clear and point opposite ways (a hole open at both
///   ends, its axis direction taken with opposite signs by the two kernels).
fn explain(
    loaded: &LoadedPart,
    python_anchors: &Map<String, Value>,
    python: &Value,
    rust: &Value,
    python_diagonal: f64,
    difference: &mut Difference,
) {
    let Some(rest) = difference.path.strip_prefix("anchors.") else {
        return;
    };
    let Some(k) = rest.split('.').next() else {
        return;
    };
    let (p, r) = (&python[k], &rust[k]);
    if p.is_null() || r.is_null() {
        return;
    }
    let face = |v: &Value| v["face"].as_u64().map(|f| f as usize);
    let Some(pf) = face(p) else {
        return;
    };
    // Rust's leader from Python's own point on Python's face, with Python's diagonal.
    let again = anchor_from_point(loaded, pf, point(&python_anchors[k]["at"]), python_diagonal);
    let same = again.is_some_and(|a| {
        a["clear"] == p["clear"]
            && (0..3).all(|i| {
                (a["out"][i].as_f64().unwrap() - p["out"][i].as_f64().unwrap()).abs() <= 1e-6
            })
    });
    let equal_faces = face(r).is_some_and(|rf| {
        rf != pf
            && matches!((area(loaded, pf), area(loaded, rf)),
                (Some(a), Some(b)) if (a - b).abs() <= 1e-6 * a.max(b))
    });
    let opposite = p["clear"] == true
        && r["clear"] == true
        && (0..3)
            .all(|i| (p["out"][i].as_f64().unwrap() + r["out"][i].as_f64().unwrap()).abs() <= 1e-6);
    // Another face of equal area explains the face, and the leader only if Rust's leader from
    // Python's face is Python's: identical bores in a pattern can differ in obstruction, so a
    // clear flag or direction is not excused by the face choice alone.
    let suffix = if equal_faces && (difference.path.ends_with(".face") || same) {
        Some("equal_area_face")
    } else if same {
        Some("same_from_python_point")
    } else if opposite {
        Some("opposite_axis")
    } else {
        None
    };
    if let Some(suffix) = suffix {
        difference.path = format!("{}.{suffix}", difference.path);
    }
}
