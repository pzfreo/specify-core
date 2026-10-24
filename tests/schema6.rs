//! Schema-6 published coordinates, checked against the actual slot walls and written PMI.
mod common;

use haecceity::brep_write::{WriteOptions, write_step};
use haecceity::geom::{V3, dot};
use haecceity::step::{IDENTITY, Placement};
use serde_json::{Map, Value, json};
use specify_core::{api, existing, part, rules};

fn vector(value: &Value) -> V3 {
    serde_json::from_value(value.clone()).unwrap()
}
fn direction(at: &Placement, p: V3) -> V3 {
    at.map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2])
}
fn point(at: &Placement, p: V3) -> V3 {
    let p = direction(at, p);
    std::array::from_fn(|i| p[i] + at[i][3])
}
fn near(a: V3, b: V3, tolerance: f64) {
    assert!(
        a.into_iter().zip(b).all(|(a, b)| (a - b).abs() < tolerance),
        "{a:?} != {b:?}"
    );
}
fn compare_numbers(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-5,
            "{a} != {b}"
        ),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                compare_numbers(a, b)
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (k, a) in a {
                compare_numbers(a, &b[k])
            }
        }
        _ => assert_eq!(a, b),
    }
}

#[test]
fn golden_oriented_slots_end_planes_questions_defaults_and_written_pmi_move_rigidly() {
    // Geometry of quiddity's golden_oriented_slots.step, preserved in the pre-upgrade fixture.
    let source = part::load(
        &common::fixtures().join("resume_alpha7/golden_oriented_slots.step.gz"),
        None,
    )
    .unwrap();
    let (s, c) = std::f64::consts::FRAC_PI_6.sin_cos();
    let z30 = [
        [c, -s, 0.0, 37.0],
        [s, c, 0.0, -19.0],
        [0.0, 0.0, 1.0, 11.0],
    ];
    // A proper generic rotation, Rz(30°) Ry(asin(0.6)), plus translation.
    let generic = [
        [0.8 * c, -s, 0.6 * c, 37.0],
        [0.8 * s, c, 0.6 * s, -19.0],
        [-0.6, 0.0, 0.8, 11.0],
    ];
    // A partially aligned converted frame: long_axis is a letter, transverse axes vectors.
    let (b, a) = 37_f64.to_radians().sin_cos();
    let mixed = [
        [-c, -s, 0.0, 37.0],
        [a * s, -a * c, -b, -19.0],
        [b * s, -b * c, a, 11.0],
    ];
    let scratch = std::env::temp_dir().join(format!("specify-schema6-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let mut reference: Option<(Value, Value, Map<String, Value>, Value)> = None;
    for (name, at) in [
        ("base", IDENTITY),
        ("z30", z30),
        ("generic", generic),
        ("mixed", mixed),
    ] {
        let moved = source.part.transformed(&at).unwrap();
        let (bytes, report) = write_step(
            &moved,
            &WriteOptions {
                name: "oriented slots",
                pmi: None,
            },
        )
        .unwrap();
        assert!(report.face_order_preserved);
        let loaded = part::load_bytes("slots.step", bytes).unwrap();
        let a = api::analyse(&loaded).unwrap();
        let slots: Vec<_> = a["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["family"] == "slots")
            .collect();
        assert_eq!(slots.len(), 3, "{name}: {slots:?}");
        for f in &slots {
            let r = &f["record"];
            assert_eq!(r["width"], 6.0);
            assert_eq!(r["length"], 24.0);
            assert!(r.get("_local").is_none() && r.get("_frame").is_none());
            let d = vector(&r["end_direction"]);
            // Each end plane agrees with an actual planar end wall (not a box or provenance).
            for end in [vector(&r["end_lo"]), vector(&r["end_hi"])] {
                assert!(
                    a["faces"].as_array().unwrap().iter().any(|p| {
                        p["kind"] == "plane"
                            && dot(vector(&p["direction"]), d).abs() > 0.999999
                            && dot(
                                std::array::from_fn(|i| vector(&p["centroid"])[i] - end[i]),
                                d,
                            )
                            .abs()
                                < 0.0051
                    }),
                    "{name}: end {end:?} of {} has no wall",
                    f["id"]
                );
            }
        }
        let qs = serde_json::to_value(
            api::questions(&a, &Map::new())
                .unwrap()
                .iter()
                .map(|q| q.to_dict())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let answers =
            rules::with_all_defaults(&a, json!({"part.material":"steel"}).as_object().unwrap())
                .unwrap();
        let intent = api::apply(&a, &answers, false).unwrap();
        let written = scratch.join(format!("{name}.step"));
        specify_core::writer::write(&loaded, &intent, &written, Some(&answers)).unwrap();
        let read = part::load(&written, None).unwrap();
        let pmi = serde_json::to_value(existing::read_existing(&read).unwrap()).unwrap();
        if let Some((base, base_qs, base_answers, base_pmi)) = &reference {
            assert_eq!(&qs, base_qs, "{name}: questions/defaults");
            assert_eq!(&answers, base_answers, "{name}: accepted defaults");
            compare_numbers(&pmi, base_pmi);
            for f in slots {
                let old = base["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|b| b["id"] == f["id"])
                    .unwrap();
                let d = vector(&f["record"]["end_direction"]);
                let expected_d = direction(&at, vector(&old["record"]["end_direction"]));
                assert!(dot(d, expected_d).abs() > 0.999999);
                let ends = [
                    vector(&f["record"]["end_lo"]),
                    vector(&f["record"]["end_hi"]),
                ];
                for old_end in [
                    vector(&old["record"]["end_lo"]),
                    vector(&old["record"]["end_hi"]),
                ] {
                    let expected = point(&at, old_end);
                    assert!(ends.iter().any(|end| {
                        dot(std::array::from_fn(|i| end[i] - expected[i]), d).abs() < 0.01
                    }));
                }
            }
            for (old, new) in base["faces"]
                .as_array()
                .unwrap()
                .iter()
                .zip(api::analyse(&read).unwrap()["faces"].as_array().unwrap())
            {
                near(
                    point(&at, vector(&old["centroid"])),
                    vector(&new["centroid"]),
                    3e-6,
                );
                near(
                    direction(&at, vector(&old["direction"])),
                    vector(&new["direction"]),
                    3e-8,
                );
            }
        } else {
            reference = Some((a, qs, answers, pmi));
        }
        eprintln!("{name}: 3 slots, 6 end planes, unchanged questions/defaults and PMI");
    }
    std::fs::remove_dir_all(scratch).unwrap();
}

/// Independent material-side evidence for the #81 recoveries and #89 corrections used
/// in the oracle ledger: labels must agree with the actual cylinder's outward sense.
#[test]
fn corrected_round_inventory_agrees_with_material_side() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    for (name, convex, concave, count) in [
        (
            "cgb202",
            vec![
                1248, 1250, 1252, 1254, 1256, 1258, 1450, 1453, 1464, 1473, 1475, 1477, 1623, 1631,
                2056, 2057,
            ],
            vec![2, 3, 11, 44, 46, 692, 697, 1215, 1219],
            279,
        ),
        (
            "cgb217",
            vec![177, 182, 205, 210, 233, 238, 261, 266],
            vec![],
            14,
        ),
        ("cgb241", vec![47], vec![27], 1),
    ] {
        let path = corpus.join(format!("cadgenbench_inputs/{name}.step"));
        let path = if path.is_file() {
            path
        } else {
            path.with_extension("step.gz")
        };
        let loaded = part::load(&path, None).unwrap();
        let features = specify_core::features::recognise(&loaded).unwrap();
        let fillets: Vec<_> = features.iter().filter(|f| f.family == "fillets").collect();
        assert_eq!(fillets.len(), count, "{name}");
        for face in convex {
            assert_eq!(
                loaded.part.frame_points_outward(face),
                Some(true),
                "{name}:{face}"
            );
            assert!(
                fillets.iter().any(|f| f.faces.contains(&face)),
                "{name}:{face}"
            );
        }
        for face in concave {
            assert_eq!(
                loaded.part.frame_points_outward(face),
                Some(false),
                "{name}:{face}"
            );
            assert!(
                !fillets.iter().any(|f| f.faces.contains(&face)),
                "{name}:{face}"
            );
        }
    }
    // Exact Python convex claims removed by #89 in this capture, checked against material.
    for (case, faces) in [
        ("corpus/mfcadpp/1000", vec![22]),
        ("corpus/mfcadpp/10033", vec![7]),
        ("corpus/mfcadpp/10103", vec![24]),
        ("corpus/mfcadpp/10127", vec![18]),
        ("corpus/mfcadpp/10156", vec![2]),
        ("corpus/mfcadpp/10163", vec![9]),
        ("corpus/mfcadpp_holdout/18016", vec![4]),
        ("corpus/mfcadpp_holdout/275", vec![15]),
        ("corpus/mfcadpp_holdout/363", vec![14, 25]),
        (
            "corpus/nist/nist_ctc_03_asme1_rc",
            vec![15, 21, 25, 31, 35, 38],
        ),
        ("nist/nist_ctc_03_asme1_ap242", vec![4, 59, 63, 67, 69, 8]),
    ] {
        let path = common::case(case)
            .input()
            .expect("the configured corpus/fixture");
        let loaded = part::load(&path, None).unwrap();
        let features = specify_core::features::recognise(&loaded).unwrap();
        for face in faces {
            assert_eq!(
                loaded.part.frame_points_outward(face),
                Some(false),
                "{case}:{face}"
            );
            assert!(
                !features
                    .iter()
                    .any(|f| f.family == "fillets" && f.faces.contains(&face)),
                "{case}:{face}"
            );
        }
    }
}
