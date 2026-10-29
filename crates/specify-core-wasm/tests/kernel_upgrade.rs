//! The browser session exposes saved-answer recovery through its existing snapshot and review.
use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use serde_json::{Value, json};
use specify_core::{part, resume};
use specify_core_wasm::session::Session;
use std::path::Path;

fn saved(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/fixtures/resume-alpha6/{name}.step.gz"));
    part::load(&path, None).unwrap().bytes.to_vec()
}

#[test]
fn browser_sessions_resume_all_87_alpha6_answers() {
    let mut count = 0;
    for name in ["plate", "plate-moved", "shaft", "corners", "pin"] {
        let bytes = saved(name);
        let expected = &resume::stored(&bytes)[&0]["answers"];
        let session = Session::open(name, &bytes).unwrap();
        let snapshot = session.snapshot();
        assert_eq!(&snapshot["analysis"]["resume"]["answers"], expected);
        count += expected.as_object().unwrap().len();
        // #21's optional depth override adds no review work to fully answered old files.
        let review = session.review(expected.as_object().unwrap()).unwrap();
        assert!(review["open"].as_array().unwrap().is_empty());
    }
    assert_eq!(count, 87);
}

#[test]
fn browser_reports_unmatched_values_and_leaves_current_question_open() {
    let bytes = saved("pin");
    let mut payload = json!(resume::stored(&bytes)[&0]);
    let answers = payload["answers"].as_object_mut().unwrap();
    let key = answers
        .keys()
        .find(|key| key.starts_with("hole.function:"))
        .unwrap()
        .clone();
    let value = answers.shift_remove(&key).unwrap();
    answers.insert("hole.function:holes:999".into(), value.clone());
    let doc = Document::parse(bytes).unwrap();
    let mut edit = Edit::new();
    for id in doc.ids() {
        if let Some(RawEntity::Simple {
            name, attributes, ..
        }) = doc.get(id)
            && name == "DESCRIPTIVE_REPRESENTATION_ITEM"
            && matches!(attributes.first(), Some(Attribute::String(s)) if s == resume::NAME)
        {
            let mut attributes = attributes.clone();
            attributes[1] = Attribute::String(resume::encode(&payload));
            edit.replace(id, haecceity::p21::simple(name, attributes));
        }
    }
    let session = Session::open("orphan.step", &doc.apply(&edit).unwrap().bytes).unwrap();
    let snapshot = session.snapshot();
    let resumed = &snapshot["analysis"]["resume"];
    assert_eq!(resumed["unresolved"][0]["answer"], value);
    assert_eq!(resumed["unresolved"][0]["id"], "hole.function:holes:999");
    let review: Value = session
        .review(resumed["answers"].as_object().unwrap())
        .unwrap();
    assert!(review["open"].as_array().unwrap().contains(&json!(key)));
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["code"] == "saved_answer_unresolved")
    );
}

#[test]
fn browser_exposes_real_alpha6_pattern_id_migration_and_new_questions() {
    let bytes = saved("ftc10-reid");
    let original = &resume::stored(&bytes)[&0]["answers"];
    let session = Session::open("ftc10.step", &bytes).unwrap();
    let snapshot = session.snapshot();
    let resumed = &snapshot["analysis"]["resume"];
    assert_eq!(resumed["answers"].as_object().unwrap().len(), 29);
    // #100 claims drill-point cones: six bore/row answers now move in addition to
    // the two schema-6 counterbore-pattern answers. Native kernel_upgrade's exact
    // support/adjacency checks prove these are the same physical features.
    assert_eq!(resumed["migrated"].as_array().unwrap().len(), 8);
    let moves = [
        ("holes:175", "holes:174.175"),
        ("holes:177", "holes:176.177"),
        (
            "hole_patterns.linear:138.140.142.144.146.147.148.149",
            "hole_patterns.linear:130.132.134.136.138.140.142.144.146.147.148.149",
        ),
        (
            "hole_patterns.linear:189.191.193",
            "hole_patterns.linear:188.189.190.191.192.193",
        ),
    ];
    let expected: Vec<_> = original
        .as_object()
        .unwrap()
        .keys()
        .filter_map(|id| {
            let (question, old) = id.split_once(':')?;
            let (_, new) = moves.iter().find(|(from, _)| *from == old)?;
            Some(json!([id, format!("{question}:{new}")]))
        })
        .collect();
    assert_eq!(resumed["migrated"], json!(expected));
    assert!(resumed["unresolved"].is_null());
    for pair in resumed["migrated"].as_array().unwrap() {
        assert_ne!(pair[0], pair[1]);
        assert_eq!(
            resumed["answers"][pair[1].as_str().unwrap()],
            original[pair[0].as_str().unwrap()]
        );
    }
    let review = session
        .review(resumed["answers"].as_object().unwrap())
        .unwrap();
    assert_eq!(
        review["open"],
        json!([
            "hole.function:hole_patterns.linear:194.195.196.197.198.199",
            "hole.position:hole_patterns.linear:194.195.196.197.198.199"
        ])
    );
}

#[test]
fn browser_sessions_resume_all_84_alpha7_answers_after_schema6_upgrade() {
    let mut total = 0;
    for (name, count, moved) in [
        ("golden_oriented_slots", 13, 0),
        ("cgb217", 37, 3),
        ("ctc02_ap242", 34, 15),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../tests/fixtures/resume_alpha7/{name}.step.gz"));
        let loaded = part::load(&path, None).unwrap();
        let expected = &resume::stored(&loaded.bytes)[&0]["answers"];
        let session = Session::open(name, &loaded.bytes).unwrap();
        let snapshot = session.snapshot();
        let resumed = &snapshot["analysis"]["resume"];
        // #100 adds terminal cones to cgb217/CTC-02's existing bores and derived
        // patterns. #91 restores the golden slot walls, but its saved answers are
        // all part/datum settings. Keep every value, explicitly checking changed IDs
        // against the saved physical supports rather than expecting unchanged keys.
        let metadata: Value = serde_json::from_slice(
            &std::fs::read(path.with_file_name(format!("{name}.json"))).unwrap(),
        )
        .unwrap();
        let migrations = resumed["migrated"].as_array().cloned().unwrap_or_default();
        assert_eq!(migrations.len(), moved, "{name}");
        assert_eq!(
            resumed["answers"].as_object().unwrap().len(),
            count,
            "{name}"
        );
        for (old, value) in expected.as_object().unwrap() {
            let new = migrations
                .iter()
                .find(|pair| pair[0] == *old)
                .and_then(|pair| pair[1].as_str())
                .unwrap_or(old);
            assert_eq!(&resumed["answers"][new], value, "{name}: {old} -> {new}");
        }
        for pair in migrations {
            let old = pair[0].as_str().unwrap().split_once(':').unwrap().1;
            let new = pair[1].as_str().unwrap().split_once(':').unwrap().1;
            let before = metadata["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["id"] == old)
                .unwrap();
            let after = snapshot["analysis"]["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["id"] == new)
                .unwrap();
            assert_eq!(before["family"], after["family"]);
            let old_faces = before["faces"].as_array().unwrap();
            let new_faces = after["faces"].as_array().unwrap();
            assert!(old_faces.iter().all(|f| new_faces.contains(f)));
            let extra: Vec<_> = new_faces
                .iter()
                .filter(|f| !old_faces.contains(f))
                .collect();
            assert!(!extra.is_empty());
            for face in extra {
                let i = face.as_u64().unwrap() as usize;
                assert!(matches!(
                    loaded.part.faces[i].surface,
                    haecceity::geom::Surface::Cone { .. }
                ));
                assert!(
                    loaded
                        .part
                        .neighbours(i)
                        .iter()
                        .any(|j| old_faces.contains(&json!(j)))
                );
            }
        }
        assert_eq!(expected.as_object().unwrap().len(), count);
        assert!(resumed["unresolved"].is_null(), "{name}: {resumed}");
        assert!(resumed["unverified"].is_null(), "{name}: {resumed}");
        let review = session
            .review(resumed["answers"].as_object().unwrap())
            .unwrap();
        assert!(
            !review["problems"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["code"] == "saved_answer_unresolved"
                    || p["code"] == "saved_answers_unverified")
        );
        total += count;
    }
    assert_eq!(total, 84);
}

#[test]
fn browser_refuses_suppressed_od_boss_answers_in_saved_and_shared_projects() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/quiddity023/turned_body_disc.step");
    let loaded = part::load(&path, None).unwrap();
    // #123: face 1 is still the Ø22 OD, co-owned by turned_bodies, but no boss.
    let key = "diameter.fit:bosses:1";
    let answers = json!({key:"g6", "part.material":"steel"})
        .as_object()
        .unwrap()
        .clone();
    let session = Session::open("disc.step", &loaded.bytes).unwrap();
    let evidence = session.evidence().unwrap();
    for e in [Some(&evidence), None] {
        let out = session.rematch_answers(&answers, e, None).unwrap();
        assert_eq!(out["unresolved"].as_array().unwrap().len(), 1);
        assert_eq!(out["unresolved"][0]["id"], key);
        assert_eq!(out["unresolved"][0]["answer"], "g6");
        assert!(out["answers"].get(key).is_none());
        assert!(out["migrated"].is_null());
    }
    let a = specify_core::api::analyse(&loaded).unwrap();
    let intent = specify_core::rules::apply_with_defaults(
        &a,
        json!({"part.material":"steel"}).as_object().unwrap(),
        true,
    )
    .unwrap();
    let (bytes, _) = specify_core::writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
    let session = Session::open("saved.step", &bytes).unwrap();
    let snapshot = session.snapshot();
    let out = &snapshot["analysis"]["resume"];
    assert_eq!(out["unresolved"][0]["id"], key);
    assert_eq!(out["unresolved"][0]["answer"], "g6");
    assert!(out["answers"].get(key).is_none());
    let review = session.review(out["answers"].as_object().unwrap()).unwrap();
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["code"] == "saved_answer_unresolved")
    );
}
