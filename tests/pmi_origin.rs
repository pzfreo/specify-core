//! For #77: the file supplies intent and a stable measurement source, without recognition
//! at read time. Frozen old-kernel fixtures are deliberately never regenerated.
use haecceity::{
    p21::{Attribute, Document, RawEntity},
    pmi::{self, AttributeSet, AttributeValue, DimensionId, NoteOwner},
};
use serde_json::{Map, Value, json};
use specify_core::{
    api, characteristics, existing, part,
    pmi_origin::{self, Origin},
    resume, rules, writer,
};
use std::path::Path;

fn load(name: &str) -> part::LoadedPart {
    part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
        None,
    )
    .unwrap()
}
fn back(bytes: Vec<u8>) -> part::LoadedPart {
    part::load_bytes("origins.step", bytes).unwrap()
}
fn read(p: &part::LoadedPart) -> pmi::PmiRead {
    pmi::read(
        &Document::parse(p.bytes.to_vec()).unwrap(),
        std::slice::from_ref(&p.definition),
    )
    .unwrap()
}
fn write(p: &part::LoadedPart, given: &Map<String, Value>) -> part::LoadedPart {
    let intent = api::preview(&api::analyse(p).unwrap(), given).unwrap();
    back(writer::write_bytes(p, &intent, Some(given)).unwrap().0)
}
fn answers() -> Map<String, Value> {
    json!({"part.general_tolerance":"ISO 2768-m", "hole.function:holes:9":"fit:H7"})
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn every_dimension_has_origin_and_recognised_sources_round_trip_losslessly_and_repeat() {
    let plate = load("parts/plate.step");
    let given = answers();
    let written = write(&plate, &given);
    let doc = Document::parse(written.bytes.to_vec()).unwrap();
    let original = Document::parse(plate.bytes.to_vec()).unwrap();
    for id in original.ids() {
        assert_eq!(
            original.span(id).map(|s| &original.bytes()[s]),
            doc.span(id).map(|s| &doc.bytes()[s]),
            "#{id}"
        );
    }
    assert!(haecceity::express::validate_document(doc.graph()).is_empty());
    assert!(haecceity::express_rules::check_all(&doc).is_empty());
    let read = read(&written);
    let p = &read.parts[0];
    let mut general = 0;
    let mut requirement = 0;
    let mut sourced = 0;
    for (i, d) in p.dimensions.iter().enumerate() {
        let m = pmi_origin::read(p, NoteOwner::Dimension(DimensionId(i)));
        let expected = if matches!(d.tolerance, pmi::DimTolerance::None) {
            general += 1;
            Origin::GeneralTolerance
        } else {
            requirement += 1;
            Origin::Requirement
        };
        assert_eq!(m.origin, Some(expected));
        let set = p
            .attributes
            .iter()
            .position(|s| {
                s.name == pmi_origin::PROPERTY && s.on == Some(NoteOwner::Dimension(DimensionId(i)))
            })
            .unwrap();
        // The UDA points to the actual characteristic root, not its shared aspect or representation.
        let ids = &read.provenance.parts[0].attributes[set];
        assert!(ids.iter().any(|&id| matches!(doc.get(id), Some(RawEntity::Simple {name, attributes, ..}) if name == "PROPERTY_DEFINITION" && matches!(attributes.get(2), Some(Attribute::EntityRef(owner)) if read.provenance.parts[0].dimensions[i].contains(owner) && matches!(doc.get(*owner), Some(RawEntity::Simple{name,..}) if name == "DIMENSIONAL_SIZE" || name == "DIMENSIONAL_LOCATION")))));
        if let Some(source) = m.source {
            sourced += 1;
            assert_eq!(source.faces, resume::fingerprint(&plate).unwrap());
            assert_eq!(source.schema, 2);
            assert!(!source.support.is_empty());
            assert!(
                source.parameter == "bore.diameter"
                    || source.parameter.starts_with("location.length.")
            );
        }
    }
    assert!(general > 0 && requirement > 0 && sourced > 1);
    let numbers = characteristics::read(&written).unwrap();
    assert!(
        numbers
            .iter()
            .filter(|n| n.kind == "dimension")
            .all(|n| n.origin.is_some())
    );
    assert_eq!(
        api::analyse(&written).unwrap()["characteristics"],
        json!(numbers)
    );
    let fit = existing::read_existing(&written)
        .unwrap()
        .into_iter()
        .find(|n| n.kind == "dimension")
        .unwrap();
    assert_eq!(fit.origin, Some(Origin::Requirement));
    assert_eq!(fit.source.unwrap().parameter, "bore.diameter");
    assert_eq!(write(&plate, &given).bytes, written.bytes);
    assert_eq!(write(&written, &given).bytes, written.bytes);
}

#[test]
fn defaults_are_requirements_foreign_pmi_is_unmodified_and_old_files_stay_absent() {
    let p = load("parts/plate.step");
    let a = api::analyse(&p).unwrap();
    let given = rules::with_all_defaults(&a, &answers()).unwrap();
    let w = write(&p, &given);
    assert!(
        characteristics::read(&w)
            .unwrap()
            .iter()
            .any(|n| n.origin == Some(Origin::Requirement))
    );
    let foreign = load("resume-alpha6/plate.step.gz");
    let before = existing::read_existing(&foreign).unwrap();
    assert!(
        before
            .iter()
            .all(|n| n.origin.is_none() && n.source.is_none())
    );
    // No stored answers grants replacement permission; remove just that set to exercise Add.
    let mut i = rules::Intent::new(serde_json::to_value(&foreign.binding).unwrap());
    i.requirements.push(json!({"kind":"size", "faces":[9],"feature":"holes:9","nominal":10.,"upper":0.02,"lower":-0.02}).as_object().unwrap().clone());
    // Rename the saved-answer UDA in a test copy, retaining its complete valid graph.
    // It then has no Specify resume permission and must take the lossless Add path.
    let foreign = back(
        String::from_utf8(foreign.bytes.to_vec())
            .unwrap()
            .replace(resume::NAME, "foreign saved data")
            .into_bytes(),
    );
    assert!(resume::answers_for(&foreign).unwrap().is_none());
    i.binding = serde_json::to_value(&foreign.binding).unwrap();
    let appended = back(writer::write_bytes(&foreign, &i, None).unwrap().0);
    let before_doc = Document::parse(foreign.bytes.to_vec()).unwrap();
    let after_doc = Document::parse(appended.bytes.to_vec()).unwrap();
    for id in before_doc.ids() {
        assert_eq!(
            before_doc.span(id).map(|s| &before_doc.bytes()[s]),
            after_doc.span(id).map(|s| &after_doc.bytes()[s])
        );
    }
    let rows = existing::read_existing(&appended).unwrap();
    assert!(
        rows.iter()
            .any(|n| n.kind == "dimension" && n.origin.is_none())
    );
    assert!(rows.iter().any(|n| n.origin == Some(Origin::Requirement)));
}

#[test]
fn v024_support_sources_survive_corrected_measurements_and_renamed_features() {
    for name in ["chamfer", "round"] {
        let p = load(&format!("quiddity025/{name}.step.gz"));
        let base: Value = serde_json::from_slice(
            &std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("tests/fixtures/quiddity025/{name}.json")),
            )
            .unwrap(),
        )
        .unwrap();
        let written = write(&p, base["answers"].as_object().unwrap());
        let source = characteristics::read(&written)
            .unwrap()
            .into_iter()
            .find(|n| n.kind == "dimension" && json!(n.faces) == base["boss"]["faces"])
            .unwrap()
            .source
            .unwrap();
        // Independent v0.2.4 evidence: measurements change, this geometric namespace does not.
        assert_eq!(source.faces, base["evidence"]["faces"]);
        assert_eq!(json!(source.support), base["boss"]["faces"]);
        assert_eq!(source.parameter, "boss.diameter");
        let again = write(&written, base["answers"].as_object().unwrap());
        let sources = |p: &part::LoadedPart| {
            characteristics::read(p)
                .unwrap()
                .into_iter()
                .filter_map(|n| n.source)
                .collect::<Vec<_>>()
        };
        assert_eq!(sources(&again), sources(&written));
    }
    // The older upgrade changed recognition IDs and grew defining supports. Only the
    // measured cylinder belongs to the source key, as for characteristic-number identity.
    let old = load("resume-alpha6/pin.step.gz");
    let analysis = api::analyse(&old).unwrap();
    let given = analysis["resume"]["answers"].as_object().unwrap();
    let written = write(&old, given);
    assert!(
        characteristics::read(&written)
            .unwrap()
            .iter()
            .any(|n| n.source.is_some())
    );
    assert_eq!(
        characteristics::read(&write(&written, given))
            .unwrap()
            .into_iter()
            .filter_map(|n| n.source)
            .collect::<Vec<_>>(),
        characteristics::read(&written)
            .unwrap()
            .into_iter()
            .filter_map(|n| n.source)
            .collect::<Vec<_>>()
    );
}

#[test]
fn reserved_name_refuses_spoofing_and_schema_one_header_upgrades_losslessly() {
    let p = load("parts/plate.step");
    let intent = api::preview(&api::analyse(&p).unwrap(), &answers()).unwrap();
    let set = AttributeSet {
        name: pmi_origin::PROPERTY.into(),
        on: None,
        items: vec![("origin".into(), AttributeValue::Text("imported".into()))],
        description: None,
        general_property: None,
    };
    assert!(
        writer::write_bytes_with(&p, &intent, None, &[set])
            .unwrap_err()
            .to_string()
            .contains("reserved")
    );
    let doc = Document::parse(p.bytes.to_vec()).unwrap();
    let mut edit = haecceity::p21::Edit::new();
    edit.set_file_schema(vec![haecceity::express::EDITIONS[0].file_schema.into()]);
    let ed1 = back(doc.apply(&edit).unwrap().bytes);
    let written = write(&ed1, &answers());
    let after = Document::parse(written.bytes.to_vec()).unwrap();
    assert_eq!(
        after.file_schema().unwrap(),
        vec![haecceity::express::TARGET_SCHEMA]
    );
    assert!(haecceity::express::validate_document(after.graph()).is_empty());
    let before = Document::parse(ed1.bytes.to_vec()).unwrap();
    for id in before.ids() {
        assert_eq!(
            before.span(id).map(|s| &before.bytes()[s]),
            after.span(id).map(|s| &after.bytes()[s])
        );
    }
}

#[test]
fn unique_support_growth_carries_the_stored_source_even_if_recognition_renames_it() {
    let p = load("parts/plate.step");
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    i.requirements.push(json!({"kind":"size","feature":"holes:9","faces":[9],"nominal":10.,"upper":0.1,"lower":-0.1}).as_object().unwrap().clone());
    let first = back(writer::write_bytes(&p, &i, Some(&Map::new())).unwrap().0);
    let before = characteristics::read(&first).unwrap().remove(0);
    assert!(before.source.is_some());
    i.binding = serde_json::to_value(&first.binding).unwrap();
    i.requirements[0].insert("faces".into(), json!([9, 10]));
    i.requirements[0].insert("feature".into(), json!("renamed-by-new-kernel"));
    let grown = back(
        writer::write_bytes(&first, &i, Some(&Map::new()))
            .unwrap()
            .0,
    );
    let after = characteristics::read(&grown).unwrap().remove(0);
    assert_eq!(after.number, before.number);
    assert_eq!(after.faces, vec![9, 10]);
    assert_eq!(after.source, before.source);
}
