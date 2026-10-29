use super::*;
use serde_json::json;

fn report() -> Revised {
    let size = Size {
        volume: None,
        area: None,
        min: [0.0; 3],
        max: [1000.0; 3],
        faces: 0,
    };
    Revised {
        verdict: "ok".into(),
        summary: String::new(),
        answers: Map::new(),
        migrated: vec![],
        unresolved: vec![],
        check: vec![],
        features: vec![],
        measures: Measures {
            old: size.clone(),
            new: size,
        },
        pmi: Pmi::default(),
        questions: vec![],
        problems: Problems::default(),
        faces: BTreeMap::new(),
        gone: vec![],
        correspondence: Some(Correspondence {
            fingerprint_version: String::new(),
            alignment: qc::Alignment {
                found: true,
                rotation: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                translation: [0.; 3],
                anchors: 0,
                inliers: 0,
                symmetric: false,
                axisymmetric: false,
                axis: None,
                fold: None,
                rms: 0.,
            },
            features: vec![],
            new_features: vec![],
            faces: vec![],
            new_faces: vec![],
            questions: vec![],
        }),
    }
}

fn motion(rotation: [[f64; 3]; 3], translation: [f64; 3]) -> Option<String> {
    let mut r = report();
    let a = &mut r.correspondence.as_mut().unwrap().alignment;
    a.rotation = rotation;
    a.translation = translation;
    r.motion()
}

#[test]
fn quarter_turn_about_z() {
    assert_eq!(
        motion([[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]], [0.; 3]).as_deref(),
        Some("turned 90° about Z")
    );
}
#[test]
fn half_turn_about_z() {
    assert_eq!(
        motion([[-1., 0., 0.], [0., -1., 0.], [0., 0., 1.]], [0.; 3]).as_deref(),
        Some("turned 180° about Z")
    );
}
#[test]
fn half_turn_about_x() {
    assert_eq!(
        motion([[1., 0., 0.], [0., -1., 0.], [0., 0., -1.]], [0.; 3]).as_deref(),
        Some("turned 180° about X")
    );
}
#[test]
fn half_turn_about_a_skew_axis() {
    // n = (1,1,0)/sqrt(2): R = 2nnᵀ - I.
    assert_eq!(
        motion([[0., 1., 0.], [1., 0., 0.], [0., 0., -1.]], [0.; 3]).as_deref(),
        Some("turned 180° about a skew axis")
    );
}
#[test]
fn near_half_turn_about_y() {
    let angle = std::f64::consts::PI - 1e-8;
    let (s, c) = angle.sin_cos();
    assert_eq!(
        motion([[c, 0., s], [0., 1., 0.], [-s, 0., c]], [0.; 3]).as_deref(),
        Some("turned 180° about Y")
    );
}
#[test]
fn pure_translation() {
    assert_eq!(
        motion([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]], [8., 15., 3.]).as_deref(),
        Some("moved X +8, Y +15, Z +3")
    );
}

fn feature(family: &str, id: &str, record: Value, faces: Vec<usize>) -> Feature {
    Feature {
        family: family.into(),
        id: id.into(),
        record: record.as_object().unwrap().clone(),
        faces,
        members: vec![],
        notes: vec![],
    }
}

#[test]
fn hole_finishes_include_their_sizes_in_holes_and_patterns() {
    for (finish, words) in [
        (
            json!({"cbore":{"diameter":12.,"depth":3.}}),
            "counterbored Ø12 × 3 deep",
        ),
        (
            json!({"spotface":{"diameter":10.,"depth":1.}}),
            "spotfaced Ø10 × 1 deep",
        ),
        (
            json!({"csink":{"major_diameter":12.,"included_angle":90.,"depth":3.}}),
            "countersunk Ø12 × 90°, 3 deep",
        ),
        (
            json!({"cbore":{"diameter":12.,"depth":3.},"outer_steps":[{"diameter":18.,"depth":2.},{"diameter":15.,"depth":1.}]}),
            "counterbored Ø12 × 3 deep, counterbored Ø18 × 2 deep, counterbored Ø15 × 1 deep",
        ),
    ] {
        let mut h = json!({"diameter":6.,"depth":7.,"bottom":"through"});
        h.as_object_mut()
            .unwrap()
            .extend(finish.as_object().unwrap().clone());
        assert_eq!(
            describe(&feature("holes", "holes:1", h.clone(), vec![1])).0,
            format!("Ø6 through hole, {words}")
        );
        let pattern = feature(
            "hole_patterns",
            "hole_patterns.bolt_circle:1.2",
            json!({"diameter":60.,"holes":[h.clone(),h]}),
            vec![1, 2],
        );
        assert_eq!(
            describe(&pattern).0,
            format!("pattern of 2 × Ø6 through holes, {words} on a Ø60 circle")
        );
    }
}

fn change(f: &Feature, class: &str) -> FeatureChange {
    let (what, at) = describe(f);
    FeatureChange {
        class: class.into(),
        family: f.family.clone(),
        old: (class == "orphaned").then(|| f.id.clone()),
        new: (class == "new").then(|| f.id.clone()),
        candidates: vec![],
        changes: BTreeMap::new(),
        what,
        became: String::new(),
        at,
        place: Place::default(),
        axis: along(&f.record).map(|(k, _, _)| k),
        there: vec![],
        new_faces: vec![],
        old_faces: vec![],
    }
}

#[test]
fn small_holes_on_a_large_part_only_fold_in_the_same_place() {
    // Positions on a metre-scale part, 10 mm apart: the former box-scaled tolerance
    // folded these Ø6 holes. Folding no longer receives the part's bounds at all.
    let old = feature(
        "holes",
        "holes:1",
        json!({"diameter":6.,"depth":10.,"bottom":"through", "axis":[0.,0.,1.],"location":[520.,520.,0.]}),
        vec![1],
    );
    let mut new = feature(
        "holes",
        "holes:2",
        json!({"diameter":6.,"depth":7.,"bottom":"through", "cbore":{"diameter":12.,"depth":3.}, "axis":[0.,0.,1.],"location":[530.,520.,3.]}),
        vec![2],
    );
    let mut changes = vec![change(&old, "orphaned"), change(&new, "new")];
    fold(
        &mut changes,
        std::slice::from_ref(&old),
        std::slice::from_ref(&new),
    );
    assert_eq!(
        changes.iter().map(|c| c.class.as_str()).collect::<Vec<_>>(),
        ["orphaned", "new"]
    );
    // The mouth can move along the bore axis; rounding by 0.2 mm across it still pairs.
    new.record
        .insert("location".into(), json!([520.2, 520., 3.]));
    let mut changes = vec![change(&old, "orphaned"), change(&new, "new")];
    fold(&mut changes, &[old], &[new]);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].class, "replaced");
}

#[test]
fn distant_small_patterns_on_a_large_part_do_not_fold() {
    let old = feature(
        "hole_patterns",
        "hole_patterns.bolt_circle:1.2",
        json!({"diameter":12.,"center":[520.,520.,0.]}),
        vec![1, 2],
    );
    let new = feature(
        "hole_patterns",
        "hole_patterns.bolt_circle:3.4",
        json!({"diameter":12.,"center":[530.,520.,0.]}),
        vec![3, 4],
    );
    let mut changes = vec![change(&old, "orphaned"), change(&new, "new")];
    fold(&mut changes, &[old], &[new]);
    assert_eq!(
        changes.iter().map(|c| c.class.as_str()).collect::<Vec<_>>(),
        ["orphaned", "new"]
    );
}

#[test]
fn chamfer_continuation_requires_carried_faces_matching_sizes_and_tangent_fillet_connections() {
    let bevel = |id: &str, face, leg| {
        feature(
            "chamfers",
            id,
            json!({"leg1":leg,"leg2":leg,"angle":45.}),
            vec![face],
        )
    };
    let old = bevel("chamfers:1", 1, 1.);
    let anchor = bevel("chamfers:10", 10, 1.);
    let corner1 = bevel("chamfers:11", 11, 1.);
    let bridge = bevel("chamfers:12", 12, 1.);
    let corner2 = bevel("chamfers:13", 13, 1.);
    let separate = bevel("chamfers:30", 30, 1.);
    let resized = bevel("chamfers:31", 31, 2.);
    let sharp_join = bevel("chamfers:32", 32, 1.);
    let fillet = feature("fillets", "fillets:20", json!({"radius":5.}), vec![20]);
    let new = vec![
        anchor, corner1, bridge, corner2, separate, resized, sharp_join, fillet,
    ];
    let mut corr = report().correspondence.unwrap();
    corr.faces.push(qc::Entry {
        old: 1,
        class: Class::Carried,
        new: Some(10),
        candidates: vec![],
        changes: BTreeMap::new(),
        cost: None,
    });
    let face = |index, ns: &[(usize, &str)]| qc::FaceFingerprint {
        index,
        surface: "plane".into(),
        parameters: BTreeMap::new(),
        axis: None,
        support: None,
        area: None,
        centroid: [0.; 3],
        normal: None,
        adjacency: vec![],
        neighbours: ns.iter().map(|(i, a)| (*i, (*a).into())).collect(),
    };
    let mut prints = qc::Fingerprints {
        fingerprint_version: String::new(),
        scale: 1000.,
        features: vec![],
        faces: vec![
            face(10, &[(11, "smooth"), (31, "smooth"), (32, "convex")]),
            face(11, &[(10, "smooth"), (12, "smooth"), (20, "convex")]),
            face(12, &[(11, "smooth"), (13, "smooth")]),
            face(13, &[(12, "smooth"), (20, "convex")]),
            face(30, &[(20, "convex")]),
            face(31, &[(10, "smooth"), (20, "convex")]),
            face(32, &[(10, "convex"), (20, "convex")]),
        ],
    };
    let original: Vec<_> = new.iter().skip(1).map(|f| change(f, "new")).collect();
    let mut changes = original.clone();
    continue_chamfers(
        &mut changes,
        std::slice::from_ref(&old),
        &new,
        &corr,
        &prints,
    );
    let carried: Vec<_> = changes.iter().filter(|c| c.class == "continued").collect();
    assert_eq!(carried.len(), 1);
    assert_eq!(carried[0].old.as_deref(), Some("chamfers:1"));
    assert_eq!(
        carried[0].candidates,
        ["chamfers:10", "chamfers:11", "chamfers:12", "chamfers:13"]
    );
    for id in ["chamfers:30", "chamfers:31", "chamfers:32"] {
        assert!(
            changes
                .iter()
                .any(|c| c.class == "new" && c.new.as_deref() == Some(id)),
            "{id}"
        );
    }
    // Mere proximity to old chamfers, without fillet borders, is not continuation.
    let with_fillet_borders = prints.clone();
    for f in &mut prints.faces {
        f.neighbours.retain(|(i, _)| *i != 20);
    }
    let mut changes = original.clone();
    continue_chamfers(
        &mut changes,
        std::slice::from_ref(&old),
        &new,
        &corr,
        &prints,
    );
    assert_eq!(
        changes
            .iter()
            .filter(|c| c.class == "new" && c.family == "chamfers")
            .count(),
        6
    );
    assert!(changes.iter().all(|c| c.class != "continued"));
    // No definite old-to-new face correspondence means no claim about continuation.
    prints = with_fillet_borders;
    corr.faces[0].class = Class::Ambiguous;
    let mut changes = original;
    continue_chamfers(&mut changes, &[old], &new, &corr, &prints);
    assert!(changes.iter().all(|c| c.class != "continued"));
}
