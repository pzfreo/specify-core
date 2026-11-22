//! The browser session shares the opt-in and provenance rules of the native writer: a parts list
//! is written only for an explicit value or accepted suggestion on an assembly part.
use serde_json::{Map, Value, json};
use specify_core::{bom, resume};
use specify_core_wasm::session::Session;

fn bytes(out: &Value) -> Vec<u8> {
    assert_eq!(out["written"], json!(true), "{out}");
    serde_json::from_value(out["bytes"].clone()).unwrap()
}

/// The parts-list questions with a suggestion: what accepting the BOM suggestions names.
fn bom_suggestions(session: &Session) -> Vec<String> {
    session.review(&Map::new()).unwrap()["questions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|q| q["id"].as_str().is_some_and(|id| id.starts_with("bom.")))
        .filter(|q| q["suggestions"].as_array().is_some_and(|s| !s.is_empty()))
        .filter_map(|q| q["id"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn bom_is_explicit_and_suggestions_do_not_become_answers() {
    let file = include_bytes!("../../../tests/fixtures/parts/assembly.step");
    let session = Session::open_part("assembly", file, 0).unwrap();
    let ordinary = bytes(&session.edit("{}"));
    assert!(
        bom::read_bytes(ordinary)
            .unwrap()
            .iter()
            .all(|r| r.item_number.value.is_null())
    );
    assert!(
        specify_core_wasm::accept_defaults(session.analysis(), &Map::new())
            .unwrap()
            .keys()
            .all(|k| !k.starts_with("bom."))
    );
    let accepted = json!({ "accept_suggestions": bom_suggestions(&session) }).to_string();
    let written = bytes(&session.edit(&accepted));
    let rows = bom::read_bytes(written.clone()).unwrap();
    assert_eq!(rows[0].item_number.value, 1);
    assert_eq!(rows[0].description.source, "default");
    assert!(
        resume::stored(&written)
            .values()
            .all(|p| p["answers"].as_object().unwrap().is_empty())
    );
    let session = Session::open_part("written", &written, 0).unwrap();
    let explicit = json!({"bom.stock":"from stock","bom.description":"Chosen description"});
    let edited = bytes(&session.edit(&json!({ "fill": explicit }).to_string()));
    let snapshot = Session::open_part("edited", &edited, 0).unwrap().snapshot();
    // The BOM attribute states these fields, so P11 stores no duplicate answers.
    assert!(resume::stored(&edited).is_empty());
    assert_eq!(
        snapshot["analysis"]["bom"]["description"]["value"],
        explicit["bom.description"]
    );
    assert_eq!(
        snapshot["analysis"]["bom"]["description"]["source"],
        "answered"
    );
    assert_eq!(
        snapshot["analysis"]["bom"]["stock"]["value"],
        explicit["bom.stock"]
    );
    let reopened = Session::open_part("edited", &edited, 0).unwrap();
    let accepted = json!({ "accept_suggestions": bom_suggestions(&reopened) }).to_string();
    let requested = bytes(&reopened.edit(&accepted));
    assert_eq!(
        bom::read_bytes(requested).unwrap()[0].stock.source,
        "answered"
    );
    assert_eq!(bom::read_bytes(edited).unwrap()[0].stock.source, "answered");
}

/// A file with no assembly structure refuses the parts list with the stable code the editor
/// matches (`bom::NO_ASSEMBLY`, docs/wasm.md "Errors"), not by its prose.
#[test]
fn bom_on_loose_parts_refuses_with_the_no_assembly_code() {
    let file = include_bytes!("../../../tests/fixtures/parts/two_parts_without_an_assembly.step");
    let session = Session::open_part("loose", file, 0).unwrap();
    let accepted = json!({ "accept_suggestions": ["bom.item_number"] }).to_string();
    let refused = session.edit(&accepted);
    assert_eq!(refused["written"], json!(false), "{refused}");
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("bom_no_assembly"),
        "{refused}"
    );
    assert_eq!(bom::NO_ASSEMBLY, "bom_no_assembly");
    // Without the BOM it writes.
    bytes(&session.edit("{}"));
}
