//! Turned part datums and coaxial hole location, using captured geometry plus live corpus.
mod common;

use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::problems::problems;
use specify_core::rules::records::Ctx;
use specify_core::rules::{DatumKind, Question, datum_kind, preview, questions};

fn captured(name: &str) -> Value {
    common::case(&format!("corpus/{name}"))
        .artifact("analysis")
        .unwrap()
}

fn q<'a>(qs: &'a [Question], id: &str) -> &'a Question {
    qs.iter()
        .find(|q| q.id == id)
        .unwrap_or_else(|| panic!("missing {id}"))
}

fn check(a: &Value, axis: &[i64], shoulder: &[i64], hole: Option<&str>) {
    let ctx = Ctx::new(a).unwrap();
    let defaults = ctx.default_datums().unwrap();
    assert_eq!(defaults, &(axis.to_vec(), shoulder.to_vec(), vec![]));
    assert_eq!(datum_kind(axis, &ctx.faces).unwrap(), DatumKind::Axis);
    assert_eq!(datum_kind(shoulder, &ctx.faces).unwrap(), DatumKind::Plane);
    if let Some(hole) = hole {
        let qs = questions(a, &Map::new()).unwrap();
        assert_eq!(
            q(&qs, &format!("hole.position:{hole}")).frame.as_deref(),
            Some("A")
        );
        assert_eq!(q(&qs, "datum.A").default, Some(json!(axis)));
        assert_eq!(q(&qs, "datum.B").default, Some(json!(shoulder)));
        assert!(q(&qs, "datum.A").basis.contains("largest turned diameter"));
        assert!(q(&qs, "datum.B").basis.contains("perpendicular to A"));
        assert!(!qs.iter().any(|q| q.id == "datum.A.flatness"));
        let intent = preview(a, &Map::new()).unwrap();
        let pos = intent
            .requirements
            .iter()
            .find(|r| {
                r.get("kind") == Some(&json!("position")) && r.get("feature") == Some(&json!(hole))
            })
            .unwrap();
        assert_eq!(pos["datums"], json!(["A"]));
        assert_eq!(pos["tolerance"], 0.2);
        assert!(
            !problems(a, &Map::new())
                .unwrap()
                .iter()
                .any(|p| p["message"].as_str().unwrap().contains("nothing locates"))
        );
    }
}

#[test]
fn real_turned_analyses_choose_an_axis_and_perpendicular_face() {
    check(
        &captured("gramel/string_post"),
        &[9],
        &[8],
        Some("holes:17.19"),
    );
    check(
        &captured("gramel/GRM-03_thumbwheel_drive_screw"),
        &[3],
        &[0],
        Some("holes:12.13"),
    );
    check(&captured("gramel/GRM-05_depth_lock_bolt"), &[0], &[2], None);
}

#[test]
fn turned_detection_rejects_different_principal_axes_and_prismatic_bodies() {
    let a = captured("gramel/string_post");
    for alteration in ["offset", "direction", "block", "no_steps"] {
        let mut a = a.clone();
        if alteration == "no_steps" {
            a["features"]
                .as_array_mut()
                .unwrap()
                .retain(|f| f["family"] != "turned_steps");
        } else {
            for f in a["faces"].as_array_mut().unwrap() {
                if f["id"] == 9 && alteration == "offset" {
                    f["origin"] = json!([1, 0, 0]);
                }
                if f["id"] == 9 && alteration == "direction" {
                    f["direction"] = json!([1, 0, 0]);
                }
                if f["id"] == 11 && alteration == "block" {
                    f["area"] = json!(10000);
                }
            }
        }
        let ctx = Ctx::new(&a).unwrap();
        assert_eq!(
            datum_kind(&ctx.default_datums().unwrap().0, &ctx.faces).unwrap(),
            DatumKind::Plane,
            "{alteration}"
        );
    }
}

#[test]
fn contact_selects_the_locating_shoulder_and_datum_answers_still_override() {
    let mut a = captured("gramel/string_post");
    for f in a["faces"].as_array_mut().unwrap() {
        if f["id"] == 3 {
            f["contact"] = json!({"area": 5, "part": "mount"});
        }
        // A much larger side-flat contact must not replace a turned axis or its shoulder.
        if f["id"] == 11 {
            f["contact"] = json!({"area": 100, "part": "clamp"});
        }
    }
    let ctx = Ctx::new(&a).unwrap();
    assert_eq!(ctx.default_datums().unwrap().1, [3]);
    let answers = json!({"datum.A": [0], "datum.B": [2]})
        .as_object()
        .unwrap()
        .clone();
    let intent = preview(&a, &answers).unwrap();
    assert_eq!(intent.datums[0]["faces"], json!([0]));
    assert_eq!(intent.datums[1]["faces"], json!([2]));
    let removed = json!({"datum.A": []}).as_object().unwrap().clone();
    assert!(
        problems(&a, &removed)
            .unwrap()
            .iter()
            .any(|p| p["about"] == "hole.function:holes:17.19"
                && p["message"].as_str().unwrap().contains("nothing locates"))
    );
}

#[test]
fn offset_parallel_and_cross_holes_are_not_treated_as_coaxial() {
    let a = captured("gramel/string_post");
    for direction in [false, true] {
        let mut a = a.clone();
        for face in a["faces"].as_array_mut().unwrap() {
            if face["id"] == 17 {
                if direction {
                    face["direction"] = json!([0, 1, 0]);
                } else {
                    face["origin"] = json!([1, 0, 0]);
                }
            }
        }
        let qs = questions(&a, &Map::new()).unwrap();
        assert_eq!(
            q(&qs, "hole.position:holes:17.19").frame.as_deref(),
            Some("A|B")
        );
    }
    let qs = questions(&a, &Map::new()).unwrap();
    assert_eq!(
        q(&qs, "hole.position:holes:5").frame.as_deref(),
        Some("A|B")
    );
}

#[test]
fn runout_uses_primary_axis_even_before_datums_are_proposed() {
    let a = captured("gramel/GRM-05_depth_lock_bolt");
    let qs = questions(&a, &Map::new()).unwrap();
    assert!(!qs.iter().any(|q| q.id == "diameter.runout:turned_steps:0"));
    assert_eq!(
        q(&qs, "diameter.runout:turned_steps:6").frame.as_deref(),
        Some("A")
    );
    let given = json!({"diameter.runout:turned_steps:6": "0.02"})
        .as_object()
        .unwrap()
        .clone();
    let intent = preview(&a, &given).unwrap();
    assert_eq!(intent.datums[0]["faces"], json!([0]));
    assert_eq!(
        intent
            .requirements
            .iter()
            .find(|r| r["kind"] == "runout")
            .unwrap()["datums"],
        json!(["A"])
    );
}

#[test]
fn live_corpus_turned_datums() {
    let Some(root) = common::corpus_dir() else {
        return;
    };
    for name in [
        "gramel/string_post",
        "gramel/GRM-03_thumbwheel_drive_screw",
        "gramel/GRM-05_depth_lock_bolt",
    ] {
        let a = api::analyse_file(&root.join(format!("{name}.step")), None).unwrap();
        // The same assertions run against the actual kernel recognition, not just the capture.
        match name {
            "gramel/string_post" => check(&a, &[9], &[8], Some("holes:17")),
            "gramel/GRM-03_thumbwheel_drive_screw" => check(&a, &[3], &[0], Some("holes:12")),
            _ => check(&a, &[0], &[2], None),
        }
    }
}

#[test]
fn an_axial_hole_needs_no_shoulder_but_removing_a_still_leaves_holes_unlocated() {
    let a = captured("gramel/string_post");
    let no_b = json!({"datum.B": []}).as_object().unwrap().clone();
    let qs = questions(&a, &no_b).unwrap();
    assert_eq!(
        q(&qs, "hole.position:holes:17.19").frame.as_deref(),
        Some("A")
    );
    assert!(
        !problems(&a, &no_b)
            .unwrap()
            .iter()
            .any(|p| p["message"].as_str().unwrap().contains("nothing locates"))
    );
    let no_a = json!({"datum.A": []}).as_object().unwrap().clone();
    let found = problems(&a, &no_a).unwrap();
    for hole in ["holes:17.19", "holes:5"] {
        assert!(
            found
                .iter()
                .any(|p| p["about"] == format!("hole.function:{hole}")
                    && p["message"].as_str().unwrap().contains("nothing locates"))
        );
    }
}

#[test]
fn cylinder_ties_use_total_seat_area_and_a_needs_no_plane() {
    use specify_core::rules::{Faces, default_datums};
    let faces = Faces::from_values(&[
        json!({"id": 0, "kind": "cylinder", "direction": [0,0,1], "origin": [0,0,0], "radius": 5, "area": 10}),
        json!({"id": 1, "kind": "cylinder", "direction": [0,0,-1], "origin": [0,0,5], "radius": 5, "area": 10}),
        json!({"id": 2, "kind": "cylinder", "direction": [0,0,1], "origin": [20,0,0], "radius": 5, "area": 15}),
    ]).unwrap();
    assert_eq!(
        default_datums(&faces, true).unwrap(),
        (vec![0, 1], vec![], vec![])
    );
}
