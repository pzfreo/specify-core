//! The browser's diff: two revisions in memory, compared as the command line compares files.
use serde_json::Map;
use specify_core::{part, revision};
use specify_core_wasm::session;
use std::path::Path;

fn bytes(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/fixtures/revisions/{name}.step.gz"));
    part::load(&path, None).unwrap().bytes.to_vec()
}

#[test]
fn the_browser_s_diff_is_the_command_line_s_with_what_a_viewer_draws() {
    let (old, new) = (bytes("bearing_flange.old"), bytes("bearing_flange.new"));
    let got = session::diff(
        ("old.step", &old),
        ("new.step", &new),
        &Map::new(),
        "flange",
    )
    .unwrap();
    let (before, after) = (
        part::load_bytes("old.step", old.clone()).unwrap(),
        part::load_bytes("new.step", new.clone()).unwrap(),
    );
    let revised = revision::diff(&before, &after, &Map::new()).unwrap();
    assert_eq!(got["report"], serde_json::to_value(&revised).unwrap());
    assert_eq!(got["markdown"], revised.markdown("flange", None));
    let view = &got["view"];
    // Both meshes, one face range per face, and the classes the picture colours by.
    assert_eq!(
        view["new"]["faces"].as_array().unwrap().len(),
        after.part.faces.len()
    );
    assert_eq!(
        view["old"]["faces"].as_array().unwrap().len(),
        before.part.faces.len()
    );
    let faces = view["faces"].as_object().unwrap();
    assert!(!faces.is_empty());
    assert!(
        faces
            .values()
            .all(|c| ["changed", "new", "ambiguous"].contains(&c.as_str().unwrap()))
    );
    assert!(view["gone"].as_array().is_some());
    assert!(view["motion"].is_null() || view["motion"]["rotation"].as_array().unwrap().len() == 3);
}

#[test]
fn a_compressed_or_unreadable_revision_is_refused_by_name() {
    let new = bytes("bearing_flange.new");
    let gz = [0x1f, 0x8b, 0, 0];
    let error =
        session::diff(("old.step.gz", &gz), ("new.step", &new), &Map::new(), "x").unwrap_err();
    assert!(error.contains("Decompress"), "{error}");
    let error = session::diff(
        ("old.step", b"garbage"),
        ("new.step", &new),
        &Map::new(),
        "x",
    )
    .unwrap_err();
    assert!(error.contains("old.step"), "{error}");
}
