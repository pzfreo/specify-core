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
    assert_eq!(scene["pmi"]["schema"], "haecceity.pmi/1");
    assert_eq!(scene["presentation"]["schema"], "haecceity.presentation/1");
    let mut ids = BTreeSet::new();
    for rows in scene["pmi"]["pmi"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_array)
        .chain(
            scene["pmi"]["units"]
                .as_object()
                .unwrap()
                .values()
                .filter_map(Value::as_array),
        )
    {
        for row in rows {
            assert!(
                ids.insert(row["id"].as_str().unwrap()),
                "duplicate {}",
                row["id"]
            );
        }
    }
    let listed: BTreeSet<_> = scene["presentation"]["semantic_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    assert_eq!(ids, listed);
    let planes: BTreeSet<_> = scene["presentation"]["planes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    let mut graphics = BTreeSet::new();
    for (id, rows) in scene["presentation"]["items"].as_object().unwrap() {
        assert!(ids.contains(id.as_str()));
        assert!(scene["specify"].get(id).is_some(), "{id}");
        for graphic in rows.as_array().unwrap() {
            validate_graphic(graphic, &planes, &mut graphics);
        }
    }
    for graphic in scene["presentation"]["unlinked"].as_array().unwrap() {
        validate_graphic(graphic, &planes, &mut graphics);
    }
    for view in scene["presentation"]["views"]
        .as_array()
        .into_iter()
        .flatten()
    {
        for id in view["graphics"].as_array().unwrap() {
            assert!(graphics.contains(id.as_str().unwrap()));
        }
    }
}
fn validate_graphic<'a>(graphic: &'a Value, planes: &BTreeSet<&str>, ids: &mut BTreeSet<&'a str>) {
    if let Some(id) = graphic["id"].as_str() {
        ids.insert(id);
    }
    if let Some(id) = graphic["plane_id"].as_str() {
        assert!(planes.contains(id));
    }
    for placement in graphic["text_placements"].as_array().into_iter().flatten() {
        if let Some(index) = placement["text_index"].as_u64() {
            assert!(index < graphic["text"].as_array().unwrap().len() as u64);
        }
        for field in ["origin", "x_axis", "normal"] {
            let vector = placement[field].as_array().unwrap();
            assert_eq!(vector.len(), 3);
            assert!(vector.iter().all(|v| v.as_f64().unwrap().is_finite()));
        }
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
    for collection in first["pmi"]["pmi"]
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(key, value)| value.is_array().then_some(key.as_str()))
    {
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
    // Resolve the actual locked dependency without enabling json-import just to
    // access its schema constants. Production and test builds stay serialise-only.
    let output = std::process::Command::new("cargo")
        .args(["metadata", "--locked", "--offline", "--format-version", "1"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cargo: Value = serde_json::from_slice(&output.stdout).unwrap();
    let kernel = cargo["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "haecceity")
        .unwrap();
    let node = cargo["resolve"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == kernel["id"])
        .unwrap();
    assert!(
        node["features"]
            .as_array()
            .unwrap()
            .contains(&json!("json"))
    );
    assert!(
        !node["features"]
            .as_array()
            .unwrap()
            .contains(&json!("json-import"))
    );
    let directory = Path::new(kernel["manifest_path"].as_str().unwrap())
        .parent()
        .unwrap();
    for name in ["pmi", "presentation"] {
        let file = format!("haecceity.{name}-1.json");
        let packaged = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/specify-core-py/python/specify_core/schema")
            .join(&file);
        assert_eq!(
            std::fs::read(packaged).unwrap(),
            std::fs::read(directory.join("schema").join(file)).unwrap()
        );
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
        .filter(|(id, m)| {
            m["question"] == q.id && scene["presentation"]["items"].get(*id).is_some()
        })
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

#[test]
fn generated_graphics_expose_model_space_placement_and_plane_ids() {
    let loaded = plate();
    let analysis = api::analyse(&loaded).unwrap();
    let answers = specify_core::rules::with_all_defaults(&analysis, &given()).unwrap();
    let scene = Scene::new(&loaded)
        .unwrap()
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    validate(&scene);
    assert!(
        !scene["presentation"]["planes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    for rows in scene["presentation"]["items"].as_object().unwrap().values() {
        for graphic in rows.as_array().unwrap() {
            assert!(graphic["id"].is_string());
            assert!(graphic["plane_id"].is_string());
            assert!(!graphic["text_placements"].as_array().unwrap().is_empty());
        }
    }
    for datum in scene["pmi"]["pmi"]["datums"].as_array().unwrap() {
        assert!(
            !scene["presentation"]["items"][datum["id"].as_str().unwrap()]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn live_foreign_review_keeps_original_datum_ids_and_file_placement() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz");
    let loaded = part::load(&path, Some(0)).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let cache = Scene::new(&loaded).unwrap();
    let file = cache.scene(&loaded, None, None).unwrap();
    validate(&file);
    let review = cache
        .scene(&loaded, Some(&analysis), Some(&Map::new()))
        .unwrap();
    validate(&review);
    assert_eq!(file["pmi"]["pmi"]["datums"], review["pmi"]["pmi"]["datums"]);
    assert_eq!(
        file["presentation"]["views"],
        review["presentation"]["views"]
    );
    assert!(
        review["presentation"]["items"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|rows| rows.as_array().unwrap())
            .all(|g| g["source"] == "file")
    );
    assert!(!file["presentation"]["views"].as_array().unwrap().is_empty());
    assert!(
        !file["presentation"]["planes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        file["pmi"]["pmi"]["datums"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |datum| file["presentation"]["items"][datum["id"].as_str().unwrap()]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty())
            )
    );
}

#[test]
fn new_foreign_control_preserves_saved_views_planes_and_datum_callouts() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz");
    let loaded = part::load(&path, Some(0)).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let cache = Scene::new(&loaded).unwrap();
    let file = cache.scene(&loaded, None, None).unwrap();
    let face = analysis["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "plane")
        .unwrap()["id"]
        .clone();
    let answers = json!({"extra.1":{"kind":"flatness", "faces":[face], "value":"0.05"}})
        .as_object()
        .unwrap()
        .clone();
    let review = cache
        .scene(&loaded, Some(&analysis), Some(&answers))
        .unwrap();
    validate(&review);
    assert_eq!(review["specify"]["flatness:extra.1"]["status"], "answered");
    assert_eq!(file["pmi"]["pmi"]["datums"], review["pmi"]["pmi"]["datums"]);
    assert_eq!(
        file["presentation"]["views"],
        review["presentation"]["views"]
    );
    for plane in file["presentation"]["planes"].as_array().unwrap() {
        assert!(
            review["presentation"]["planes"]
                .as_array()
                .unwrap()
                .contains(plane)
        );
    }
    for datum in file["pmi"]["pmi"]["datums"].as_array().unwrap() {
        let id = datum["id"].as_str().unwrap();
        assert_eq!(
            file["presentation"]["items"][id],
            review["presentation"]["items"][id]
        );
        assert_eq!(review["specify"][id]["status"], "existing");
    }
}
