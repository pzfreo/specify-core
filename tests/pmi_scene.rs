use serde_json::{Map, Value, json};
use specify_core::{
    api, part,
    pmi_scene::{self, Scene},
    writer,
};
use std::{collections::BTreeSet, path::Path};
mod common;

fn plate() -> part::LoadedPart {
    part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/plate.step"),
        None,
    )
    .unwrap()
}
fn given() -> Map<String, Value> {
    json!({"part.general_tolerance":"ISO 2768-m", "hole.function:holes:9":"fit:H7"})
        .as_object()
        .unwrap()
        .clone()
}
fn validate(scene: &Value) {
    haecceity::json::validate_schema(&scene["pmi"]).unwrap();
    haecceity::json::presentation::validate(&scene["presentation"]).unwrap();
    let semantic = haecceity::json::from_value(
        &scene["pmi"],
        &haecceity::json::Anchors {
            names: vec![None; 100000],
            edge_count: 100000,
            ..Default::default()
        },
    )
    .unwrap();
    let graphics = serde_json::from_value(scene["presentation"].clone()).unwrap();
    haecceity::json::presentation::validate_links(&graphics, &semantic).unwrap();
    for id in scene["presentation"]["items"].as_object().unwrap().keys() {
        assert!(scene["specify"].get(id).is_some(), "{id}");
    }
}
#[test]
fn requirement_ids_are_stable_and_statuses_use_review_and_stored_origin() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let cache = Scene::new(&loaded).unwrap();
    let answers = given();
    let first = cache
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    validate(&first);
    let metadata = first["specify"].as_object().unwrap();
    assert_eq!(metadata["size:holes:9"]["status"], "answered");
    assert_eq!(
        metadata["size:holes:9"]["question"],
        "hole.function:holes:9"
    );
    assert!(metadata.values().any(|m| m["status"] == "general"));
    let accepted_defaults = specify_core::rules::with_all_defaults(&analysis, &answers).unwrap();
    let state = specify_core::tolerancing::State {
        auto: Some(
            accepted_defaults
                .keys()
                .filter(|q| !answers.contains_key(*q))
                .cloned()
                .collect(),
        ),
        answers: accepted_defaults,
        gdt: None,
    };
    let defaults = cache
        .scene_with_state(&loaded, Some(&analysis), &state)
        .unwrap();
    assert!(
        defaults["specify"]
            .as_object()
            .unwrap()
            .values()
            .any(|m| m["status"] == "default")
    );
    let mut changed = answers.clone();
    changed.insert("hole.function:holes:9".into(), json!("fit:H8"));
    let second = cache
        .scene(&loaded, Some(&analysis), Some(&changed))
        .unwrap();
    validate(&second);
    for collection in ["dimensions", "datums", "tolerances"] {
        let ids = |scene: &Value| {
            scene["pmi"]["pmi"][collection]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["id"].clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&first), ids(&second), "{collection}");
    }
    // Real writer agreement for fully accepted answers and file-only reopening.
    let accepted = answers.clone();
    let intent = api::preview(&analysis, &accepted).unwrap();
    let bytes = writer::write_bytes(&loaded, &intent, Some(&accepted))
        .unwrap()
        .0;
    let stored = pmi_scene::pmi_scene(&bytes, None).unwrap();
    validate(&stored);
    assert_eq!(
        stored["specify"]["size:holes:9"]["number"],
        metadata["size:holes:9"]["number"]
    );
    assert_eq!(stored["specify"]["size:holes:9"]["origin"], "requirement");
    let general: Vec<_> = stored["specify"]
        .as_object()
        .unwrap()
        .values()
        .filter(|m| m["origin"] == "general_tolerance")
        .collect();
    assert!(!general.is_empty());
    assert!(general.iter().all(|m| m["status"] == "general"));
}
#[test]
fn scene_face_anchors_are_snapshot_mesh_indices_even_when_step_faces_have_names() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let scene = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&given()))
        .unwrap();
    let features = scene["pmi"]["pmi"]["features"].as_array().unwrap();
    let size = scene["pmi"]["pmi"]["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == "size:holes:9")
        .unwrap();
    let fid = &size["value"]["kind"]["size"]["feature"];
    let feature = features.iter().find(|f| f["id"] == *fid).unwrap();
    // The plate's hole is face 9 in analyse/mesh, regardless of its STEP entity number.
    assert_eq!(feature["value"], json!({"items":[{"face":9}]}));
    assert_eq!(
        api::mesh(&loaded)["faces"].as_array().unwrap().len(),
        loaded.definition.faces.len()
    );
}
#[test]
fn foreign_nist_ctc01_is_view_only_and_keeps_as_drawn_graphics() {
    use std::io::Read;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz");
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_end(&mut bytes)
        .unwrap();
    let scene = pmi_scene::pmi_scene(&bytes, Some(0)).unwrap();
    validate(&scene);
    assert!(
        scene["specify"]
            .as_object()
            .unwrap()
            .values()
            .all(|m| m["status"] == "existing")
    );
    let graphics: Vec<_> = scene["presentation"]["items"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|v| v.as_array().unwrap())
        .chain(scene["presentation"]["unlinked"].as_array().unwrap())
        .collect();
    assert!(!graphics.is_empty());
    assert!(graphics.iter().all(|g| g["source"] == "file"));
    assert!(
        graphics
            .iter()
            .any(|g| !g["polylines"].as_array().unwrap().is_empty())
    );
}
#[test]
fn python_schemas_match_the_pinned_kernel() {
    for (name, schema) in [
        ("pmi", haecceity::json::PMI_SCHEMA),
        ("presentation", haecceity::json::PRESENTATION_SCHEMA),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "crates/specify-core-py/python/specify_core/schema/haecceity.{name}-1.json"
        ));
        assert_eq!(std::fs::read_to_string(path).unwrap(), schema);
    }
}
#[test]
fn patterned_requirements_put_the_count_in_notation_and_first_cell() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let mut answers = given();
    let q = api::questions(&analysis, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id.starts_with("hole.function:hole_patterns"))
        .unwrap();
    answers.insert(q.id.clone(), json!("fit:H7"));
    let scene = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    let ids: BTreeSet<_> = scene["specify"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, m)| m["question"] == q.id)
        .map(|(id, _)| id)
        .collect();
    assert!(!ids.is_empty());
    for id in ids {
        for graphic in scene["presentation"]["items"][id].as_array().unwrap() {
            assert!(graphic["name"].as_str().unwrap().starts_with("6×"));
            assert!(graphic["text"][0].as_str().unwrap().starts_with("6×"));
        }
    }
}
