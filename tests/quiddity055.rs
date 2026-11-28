//! User-visible boundaries of the 0.5.5 recognition upgrade.
mod common;

use serde_json::{Map, Value, json};
use specify_pmi::{api, edits, part, rules, writer};

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
fn mfcad_10127_keeps_the_length_on_published_shallow_levels() {
    let Some(part) = corpus_part("mfcadpp/10127.step") else {
        return;
    };
    let analysis = api::analyse(&part).unwrap();
    for face in [20, 25] {
        assert!(
            analysis["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["family"] == "step_levels"
                    && r["faces"].as_array().unwrap().contains(&json!(face)))
        );
    }
    let row = rows(&analysis)
        .iter()
        .find(|r| r["id"] == "length:planes:20-25")
        .expect("10127: preserve the independent length on faces 20 and 25");
    assert_eq!(row["faces"], json!([25]));
    assert_eq!(row["suggestion"], "4.23656 ±0.1 from face 20");
}

#[test]
fn ftc07_keeps_stated_location_between_retired_levels_as_other_pmi() {
    let Some(part) = corpus_part("nist/nist_ftc_07_asme1_rd.stp") else {
        return;
    };
    // Quiddity 0.5.4 included 84 and 173 in its planner ladder. 0.5.5 drops them
    // from its ladder. An independently stated location still belongs to the file.
    let mut intent = rules::Intent::new(serde_json::to_value(&part.binding).unwrap());
    intent.requirements.push(
        json!({
            "kind": "location", "faces": [173], "reference": [84],
            "distance": 9.517274, "upper": 0.1, "lower": -0.1
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    let (bytes, _) = common::whole_intent::write_bytes(part.bytes.to_vec(), &[(intent, None)]).unwrap();
    let annotated = part::load_bytes("ftc07.step", bytes).unwrap();
    let analysis = api::analyse(&annotated).unwrap();
    let input: quiddity::dimensioning::Input =
        serde_json::from_value(analysis["dimensioning"].clone()).unwrap();
    assert!(
        input
            .steps
            .heights
            .iter()
            .all(|step| !step.faces.iter().any(|face| [84, 173].contains(face))),
        "FTC-07: faces 84 and 173 have left the kernel height ladder"
    );
    assert!(analysis["features"].as_array().unwrap().iter().all(|r| {
        r["family"] != "step_levels"
            || [84, 173]
                .into_iter()
                .all(|face| !r["faces"].as_array().unwrap().contains(&json!(face)))
    }));
    assert!(
        analysis["table"]["file_pmi"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["type"] == "Location_LinearDistance" && r["faces"] == json!([84, 173])),
        "FTC-07: location between the retired levels must appear under Other PMI"
    );
    assert!(
        !rows(&analysis)
            .iter()
            .any(|r| r["id"] == "length:planes:84-173")
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
fn string_post_fillets_are_recognised_without_rows() {
    let Some(part) = corpus_part("gramel/string_post.step") else {
        return;
    };
    // Fillet rows wait for quiddity 0.5.6 radius groups (Paul, 2026-11-22).
    let analysis = api::analyse(&part).unwrap();
    let fillets: Vec<_> = analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|feature| feature["family"] == "fillets")
        .collect();
    assert!(
        fillets
            .iter()
            .any(|feature| feature["faces"] == json!([7]) && feature["record"]["radius"] == 0.25)
    );
    assert!(
        rows(&analysis)
            .iter()
            .all(|row| fillets.iter().all(|fillet| row["feature"] != fillet["id"])),
        "string_post: recognised fillets must have no rows"
    );
    let questions = api::questions(&analysis, &Map::new()).unwrap();
    assert!(questions.iter().all(|q| {
        fillets
            .iter()
            .all(|fillet| q.feature.as_deref() != fillet["id"].as_str())
    }));
    let edited = edit(
        &part,
        json!({"fill": {"part.general_tolerance": "ISO 2768-f"}}),
    );
    let saved = part::load_bytes("string_post.step", edited.bytes).unwrap();
    let analysis = api::analyse(&saved).unwrap();
    assert_eq!(analysis["part"]["general_tolerance"], "ISO 2768-f");
    assert!(
        analysis["existing"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["type"] != "Size_Radius")
    );
}

#[test]
fn mfcad_10092_keeps_the_datum_reference_of_an_existing_length() {
    let Some(part) = corpus_part("mfcadpp/10092.step") else {
        return;
    };
    let analysis = api::analyse(&part).unwrap();
    assert!(
        analysis["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["id"] == "step_levels:19.35" && f["record"]["height_axis"] == "y")
    );
    let row = rows(&analysis)
        .iter()
        .find(|r| r["id"] == "length:planes:17-19")
        .expect("10092: the 2 mm length must keep datum B's face 17");
    assert_eq!(row["faces"], json!([19]));
    assert_eq!(row["suggestion"], "2 ±0.1 from B");
    assert!(
        !rows(&analysis)
            .iter()
            .any(|r| r["id"] == "length:planes:5.17-19")
    );
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

#[test]
fn ftc07_tilted_length_is_stated_changeable_and_removable() {
    let Some(part) = corpus_part("nist/nist_ftc_07_asme1_rd.stp") else {
        return;
    };
    let id = "length:planes:83-255.263";
    let before = api::analyse(&part).unwrap();
    let normal = &before["faces"][83]["direction"];
    assert!(
        normal
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v.as_f64().unwrap().abs() < 1.0),
        "face 83 must be tilted"
    );
    assert!(!before["features"].as_array().unwrap().iter().any(|f| {
        f["family"] == "step_levels" && f["faces"].as_array().unwrap().contains(&json!(83))
    }));
    let row = |analysis: &Value| {
        rows(analysis)
            .iter()
            .find(|row| row["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(row(&before)["state"], "gap");
    let mut intent = rules::Intent::new(serde_json::to_value(&part.binding).unwrap());
    for (from, to, distance) in [
        (json!([255, 263]), json!([83]), 258.205),
        (json!([84]), json!([173]), 9.517274),
    ] {
        intent.requirements.push(
            json!({"kind": "location", "faces": to, "reference": from, "distance": distance,
                "upper": 0.5, "lower": -0.5})
            .as_object()
            .unwrap()
            .clone(),
        );
    }
    let (bytes, _) = common::whole_intent::write_bytes(part.bytes.to_vec(), &[(intent, None)]).unwrap();
    let annotated = part::load_bytes("ftc07.step", bytes).unwrap();
    let before = api::analyse(&annotated).unwrap();
    let stated = row(&before);
    assert_eq!(stated["state"], "stated");
    assert_eq!(stated["value"], "258.205 ±0.5");
    let other = &before["table"]["file_pmi"];
    assert_eq!(other.as_array().unwrap().len(), 1);
    assert_eq!(other[0]["faces"], json!([84, 173]));

    let changed = edit(
        &annotated,
        json!({"change": {(id): {"expect": stated["value"], "to": "±0.2"}}}),
    );
    let reopened = part::load_bytes("ftc07.step", changed.bytes).unwrap();
    let after = api::analyse(&reopened).unwrap();
    let updated = row(&after);
    assert_eq!(updated["state"], "stated");
    assert_eq!(updated["value"], "258.205 ±0.2");
    assert_eq!(
        updated["requirements"][0]["faces"],
        stated["requirements"][0]["faces"]
    );
    assert_eq!(after["table"]["file_pmi"], *other);

    let removed = edit(&reopened, json!({"items": {"remove": [{"id": id}]}}));
    let after = api::analyse(&part::load_bytes("ftc07.step", removed.bytes).unwrap()).unwrap();
    assert_eq!(row(&after)["state"], "gap");
    assert_eq!(after["table"]["file_pmi"], *other);
}

#[test]
fn ftc07_has_measured_step_rows_on_all_three_axes() {
    let Some(part) = corpus_part("nist/nist_ftc_07_asme1_rd.stp") else {
        return;
    };
    let analysis = api::analyse(&part).unwrap();
    for (axis, level, id, suggestion) in [
        (
            "x",
            "step_levels:201",
            "length:planes:42-201",
            "297.253 ±0.5 from face 42",
        ),
        (
            "y",
            "step_levels:174.176.178.179",
            "length:planes:86-174.176.178.179",
            "16.51 ±0.2 from A",
        ),
        (
            "z",
            "step_levels:166.170",
            "length:planes:166.170-254.257",
            "138.121 ±0.5 from faces 254, 257",
        ),
    ] {
        assert!(
            analysis["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|feature| {
                    feature["id"] == level && feature["record"]["height_axis"] == axis
                }),
            "FTC-07: missing {axis} level {level}"
        );
        assert!(
            rows(&analysis).iter().any(|row| {
                row["id"] == id && row["state"] == "gap" && row["suggestion"] == suggestion
            }),
            "FTC-07: missing measured {axis} length {id}"
        );
    }
}
