use serde_json::json;
use specify_core_wasm::session::Session;

#[test]
fn open_part_scene_faces_use_the_same_indices_and_frame_as_snapshot() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/parts/assembly.step"),
    )
    .unwrap();
    let parts = Session::parts("assembly.step", &bytes).unwrap();
    for row in parts.as_array().unwrap() {
        let part = row["part"].as_u64().unwrap() as usize;
        let engine = Session::open_part("assembly.step", &bytes, part).unwrap();
        let snapshot = engine.snapshot();
        let face = snapshot["analysis"]["faces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["kind"] == "plane")
            .unwrap()["id"]
            .as_u64()
            .unwrap();
        let answers = json!({"extra.1":{"kind":"flatness", "faces":[face], "value":"0.05"}})
            .as_object()
            .unwrap()
            .clone();
        let scene = engine.pmi_scene(&answers).unwrap();
        assert_eq!(scene["specify"]["flatness:extra.1"]["question"], "extra.1");
        let control = &scene["pmi"]["pmi"]["tolerances"][0]["value"]["target"]["feature"];
        let linked = scene["pmi"]["pmi"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == *control)
            .unwrap();
        assert_eq!(linked["value"]["items"], json!([{"face": face}]));
        let count = snapshot["mesh"]["faces"].as_array().unwrap().len();
        for feature in scene["pmi"]["pmi"]["features"].as_array().unwrap() {
            if let Some(items) = feature["value"]["items"].as_array() {
                for anchor in items {
                    if let Some(face) = anchor.get("face") {
                        assert!(face.as_u64().unwrap() < count as u64);
                        assert_eq!(
                            snapshot["analysis"]["faces"][face.as_u64().unwrap() as usize]["id"],
                            json!(face)
                        );
                    }
                }
            }
        }
    }
}
