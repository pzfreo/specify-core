//! User-visible boundaries of the 0.5.5 recognition upgrade.
mod common;

use serde_json::{Map, Value, json};
use specify_core::{api, edits, part, rules, writer};

fn corpus_part(name: &str) -> Option<part::LoadedPart> {
    Some(part::load(&common::corpus_dir()?.join(name), None).unwrap())
}

fn rows(analysis: &Value) -> &[Value] {
    analysis["table"]["rows"].as_array().unwrap()
}

fn edit(part: &part::LoadedPart, list: Value) -> writer::apply::Applied {
    writer::apply::apply(part, &edits::read(&list.to_string()).unwrap()).unwrap()
}

#[test]
fn mfcad_10127_gets_a_length_on_the_y_level() {
    let Some(part) = corpus_part("mfcadpp/10127.step") else {
        return;
    };
    let analysis = api::analyse(&part).unwrap();
    let id = "length:planes:5-8.17";
    let row = rows(&analysis)
        .iter()
        .find(|r| r["id"] == id)
        .expect("10127: missing length:planes:5-8.17 on the Y level");
    assert!(
        analysis["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["id"] == "step_levels:8.17" && f["record"]["height_axis"] == "y")
    );
    assert_eq!(row["state"], "gap");
    assert_eq!(row["faces"], json!([8, 17]));
    assert_eq!(row["suggestion"], "15.0278 ±0.2 from B");
    let written = edit(&part, json!({"accept_suggestions": [id]}));
    let saved = api::analyse(&part::load_bytes("10127.step", written.bytes).unwrap()).unwrap();
    assert!(
        rows(&saved)
            .iter()
            .any(|r| r["id"] == id && r["state"] == "stated")
    );
}

#[test]
fn mfcad_10127_keeps_stated_tilted_location_as_other_pmi() {
    let Some(part) = corpus_part("mfcadpp/10127.step") else {
        return;
    };
    // A foreign linear dimension on the two tilted planes, without a Specify question marker.
    let mut intent = rules::Intent::new(serde_json::to_value(&part.binding).unwrap());
    intent.requirements.push(
        json!({
            "kind": "location", "faces": [25], "reference": [20],
            "distance": 4.23656, "upper": 0.1, "lower": -0.1
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    let (bytes, _) = api::write_bytes(part.bytes.to_vec(), &[(intent, None)]).unwrap();
    let annotated = part::load_bytes("10127.step", bytes).unwrap();
    let analysis = api::analyse(&annotated).unwrap();
    assert!(
        analysis["table"]["file_pmi"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["type"] == "Location_LinearDistance" && r["faces"] == json!([20, 25])),
        "10127: tilted location must appear under Other PMI"
    );
    assert!(
        !rows(&analysis)
            .iter()
            .any(|r| r["id"] == "length:planes:20-25")
    );

    let output = edit(&annotated, json!({"fill": {"part.coating": "anodised"}}));
    let read = |bytes: &[u8]| haecceity::p21::Document::parse(bytes.to_vec()).unwrap();
    let original = read(&annotated.bytes);
    let saved = read(&output.bytes);
    let item = analysis["table"]["file_pmi"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["type"] == "Location_LinearDistance")
        .unwrap();
    let entity = item["id"]
        .as_str()
        .unwrap()
        .strip_prefix("file_pmi:#")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        saved.bytes()[saved.span(entity).unwrap()],
        original.bytes()[original.span(entity).unwrap()]
    );
}

#[test]
fn threaded_connector_109_has_no_length_for_the_supporting_cap() {
    let Some(part) = corpus_part("cadgenbench/threaded_connector_109.step") else {
        return;
    };
    let analysis = api::analyse(&part).unwrap();
    assert!(
        !rows(&analysis)
            .iter()
            .any(|r| r["id"] == "length:planes:159-215"),
        "threaded_connector_109: cap 215 is only supporting evidence"
    );
    assert_eq!(analysis["dimensioning"]["supporting_faces"], json!([215]));
}

#[test]
fn string_post_tiny_fillet_is_optional_and_writes_only_on_request() {
    let Some(part) = corpus_part("gramel/string_post.step") else {
        return;
    };
    let analysis = api::analyse(&part).unwrap();
    let id = "fillet.radius:fillets:7";
    let row = rows(&analysis)
        .iter()
        .find(|r| r["id"] == id)
        .expect("string_post: missing optional R0.25 fillet row");
    assert_eq!(row["state"], "gap");
    assert_eq!(row["suggestion"], "none");
    assert_eq!(row["open"], false);
    let questions = api::questions(&analysis, &Map::new()).unwrap();
    assert!(
        !rules::open_questions(&questions, &Map::new())
            .iter()
            .any(|q| q.id == id)
    );

    let read = |output: writer::apply::Applied| {
        api::analyse(&part::load_bytes("string_post.step", output.bytes).unwrap()).unwrap()
    };
    let empty = read(edit(&part, json!({})));
    assert_eq!(empty["existing"], analysis["existing"]);
    let declined = read(edit(&part, json!({"accept_suggestions": [id]})));
    assert_eq!(declined["existing"], analysis["existing"]);
    assert!(
        rows(&declined)
            .iter()
            .any(|r| r["id"] == id && r["state"] == "decided_none")
    );

    let accepted = read(edit(&part, json!({"fill": {id: "R0.25"}})));
    let row = rows(&accepted).iter().find(|r| r["id"] == id).unwrap();
    assert_eq!(row["state"], "stated");
    assert_eq!(row["requirements"][0]["type"], "Size_Radius");
    assert_eq!(row["value"], "R0.25");
}

#[test]
fn grm05_keeps_its_questions_and_three_lengths() {
    let Some(part) = corpus_part("gramel/GRM-05_depth_lock_bolt.step") else {
        return;
    };
    let analysis = api::analyse(&part).unwrap();
    let questions = api::questions(&analysis, &Map::new()).unwrap();
    let expected = [
        ("part.material", Value::Null),
        ("part.general_tolerance", json!("ISO 2768-m")),
        ("part.surface_finish", json!("Ra 3.2")),
        ("part.coating", json!("None (as machined)")),
        ("part.heat_treatment", json!("None")),
        ("part.edges", json!("Break sharp edges 0.2 max")),
        ("part.notes", json!([])),
        ("diameter.fit:turned_steps:0", json!("general")),
        ("diameter.fit:turned_steps:4", json!("general")),
        ("diameter.runout:turned_steps:4", json!("none")),
        ("diameter.fit:turned_steps:6", json!("general")),
        ("diameter.runout:turned_steps:6", json!("none")),
        ("datum.A", json!([0])),
        ("datum.B", json!([2])),
        ("datum.C", json!([])),
        ("datum.B.control", json!("none")),
        ("part.process", json!("machined")),
        ("length:planes:2-3", json!("14 ±0.2 from B")),
        ("length:planes:2-5", json!("5 ±0.1 from B")),
        ("length:planes:2-8", json!("18 ±0.2 from B")),
    ];
    assert_eq!(
        questions
            .iter()
            .map(|q| (q.id.as_str(), q.default.clone().unwrap_or_default()))
            .collect::<Vec<_>>(),
        expected
    );
    for ((id, suggestion), face) in expected[17..].iter().zip([3, 5, 8]) {
        let row = rows(&analysis).iter().find(|r| r["id"] == *id).unwrap();
        assert_eq!(row["state"], "gap");
        assert_eq!(&row["suggestion"], suggestion);
        assert_eq!(row["faces"], json!([face]));
    }
}
