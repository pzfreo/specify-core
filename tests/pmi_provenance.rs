//! STEP-only authorship, including the two gramel regressions from #22.
mod common;

use std::path::PathBuf;

use haecceity::p21::{Attribute, Document, RawEntity};
use haecceity::pmi::{self, AttributeValue};
use haecceity::step::read_part_definitions;
use serde_json::{Map, Value, json};
use specify_core::{api, existing, part, rules, writer};

const MARKER: &str = "specify-core:written:v1";

fn has(doc: &Document, ids: &[u64], entity: &str, slot: usize) -> bool {
    ids.iter().any(|&id| {
        matches!(doc.get(id),
        Some(RawEntity::Simple { name, attributes, .. }) if name == entity
            && matches!(attributes.get(slot), Some(Attribute::String(s)) if s == MARKER))
    })
}

fn described(doc: &Document, ids: &[u64], entity: &str) -> bool {
    ids.iter().any(|&id| {
        matches!(doc.get(id), Some(RawEntity::Simple { name, .. }) if name == entity)
            && doc.referrers(id).iter().any(|&r| {
                matches!(doc.get(r),
            Some(RawEntity::Simple { name, attributes, .. }) if name == "DESCRIPTION_ATTRIBUTE"
                && attributes == &[Attribute::String(MARKER.into()), Attribute::EntityRef(id)])
            })
    })
}

/// Read through the pinned reader, checking every semantic root, independently of Specify's
/// recognition helper. Raw fields are reached through the reader's per-item provenance.
fn all_owned(bytes: &[u8]) -> pmi::PartPmi {
    let doc = Document::parse(bytes.to_vec()).unwrap();
    let defs = read_part_definitions(bytes).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    assert_eq!(read.parts.len(), 1);
    let p = &read.parts[0];
    let prov = &read.provenance.parts[0];
    for ids in &prov.dimensions {
        assert!(
            described(&doc, ids, "SHAPE_DIMENSION_REPRESENTATION"),
            "dimension lacks its representation description: {ids:?}"
        );
    }
    assert!(
        p.tolerances
            .iter()
            .all(|t| t.description.as_deref() == Some(MARKER))
    );
    for (items, entity, slot) in [
        (&prov.datums, "DATUM", 1),
        (&prov.threads, "THREAD", 1),
        (&prov.knurls, "TURNED_KNURL", 0),
    ] {
        for ids in items {
            assert!(
                has(&doc, ids, entity, slot),
                "missing marker on {entity}: {ids:?}"
            );
        }
    }
    for items in [&prov.general, &prov.surface_textures, &prov.attributes] {
        for ids in items {
            assert!(
                described(&doc, ids, "PROPERTY_DEFINITION_REPRESENTATION"),
                "{ids:?}"
            );
        }
    }
    if p.material.is_some() {
        assert!(described(
            &doc,
            &prov.material,
            "PROPERTY_DEFINITION_REPRESENTATION"
        ));
    }
    // Fixed UDA metadata and values stay exactly as alpha.8 writes/reads them. No additional
    // note or attribute is introduced by the description annotations.
    assert!(
        p.attributes
            .iter()
            .all(|a| a.description.as_deref() == Some("user defined attribute"))
    );
    let mut unmark = haecceity::p21::Edit::new();
    for (&id, e) in &doc.graph().entities {
        if matches!(e, RawEntity::Simple { name, .. } if name == "DESCRIPTION_ATTRIBUTE") {
            unmark.remove(id);
        }
        if let RawEntity::Simple {
            name, attributes, ..
        } = e
            && name == "PROPERTY_DEFINITION"
        {
            assert!(!matches!(attributes.get(1), Some(Attribute::String(s)) if s == MARKER));
            if matches!(attributes.first(), Some(Attribute::String(s)) if s.is_empty() || s == "surface_condition")
            {
                assert_eq!(
                    attributes[1],
                    Attribute::String(String::new()),
                    "helper/parameter #{id}"
                );
                for &link in doc.referrers(id) {
                    assert!(
                        !described(&doc, &[link], "PROPERTY_DEFINITION_REPRESENTATION"),
                        "helper #{id} marked: {e:?}, referrers: {:?}",
                        doc.referrers(id)
                            .iter()
                            .map(|r| doc.get(*r))
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }
    let plain = Document::parse(doc.apply(&unmark).unwrap().bytes).unwrap();
    let plain_read = pmi::read(&plain, &defs).unwrap();
    assert_eq!(p.attributes, plain_read.parts[0].attributes);
    assert_eq!(read.findings, plain_read.findings);
    assert!(haecceity::express::validate_document(doc.graph()).is_empty());
    assert!(haecceity::express_rules::check_all(&doc).is_empty());
    p.clone()
}

fn corpus(name: &str) -> Option<PathBuf> {
    let root =
        std::env::var_os("SPECIFY_CORPUS").or_else(|| std::env::var_os("QUIDDITY_CORPUS"))?;
    Some(PathBuf::from(root).join(name))
}

#[test]
fn gramel_threads_knurls_and_every_other_written_requirement_have_step_provenance() {
    assert_eq!(writer::PMI_PROVENANCE, MARKER);
    for stem in ["GRM-02_shaft", "GRM-03_thumbwheel_drive_screw"] {
        let path = corpus(&format!("gramel/{stem}.step"));
        let path = match path.filter(|p| p.is_file()) {
            Some(p) => p,
            None if stem == "GRM-02_shaft" => {
                eprintln!(
                    "gramel/GRM-02_shaft.step absent from corpus: using the committed shaft fixture"
                );
                common::fixtures().join("parts/GRM-02_shaft.step")
            }
            None => {
                assert!(
                    std::env::var_os("SPECIFY_CORPUS").is_none()
                        && std::env::var_os("QUIDDITY_CORPUS").is_none(),
                    "exported corpus is missing {stem}"
                );
                eprintln!("no corpus: GRM-03 provenance regression not run");
                continue;
            }
        };
        let loaded = part::load(&path, None).unwrap();
        let analysis = api::analyse(&loaded).unwrap();
        let mut given = json!({
            "part.material": "steel", "part.surface_finish": "Ra 3.2",
            "part.notes": ["Deburr before assembly"]
        })
        .as_object()
        .unwrap()
        .clone();
        if stem == "GRM-03_thumbwheel_drive_screw" {
            given.extend(
                json!({
                    "hole.function:holes:12": "tapped:M2",
                    "diameter.fit:turned_steps:6": "thread:M3",
                    "diameter.fit:turned_steps:3": "knurl:straight"
                })
                .as_object()
                .unwrap()
                .clone(),
            );
        }
        let answers = rules::with_all_defaults(&analysis, &given).unwrap();
        let intent = rules::apply_with_defaults(&analysis, &answers, false).unwrap();
        for store_answers in [false, true] {
            let (bytes, _) =
                writer::write_bytes(&loaded, &intent, store_answers.then_some(&answers)).unwrap();
            let pmi = all_owned(&bytes);
            assert!(!pmi.threads.is_empty(), "{stem} should write threads");
            if stem == "GRM-03_thumbwheel_drive_screw" {
                assert!(!pmi.knurls.is_empty());
                assert!(pmi.threads.iter().any(|t| {
                    t.qualifier
                        .as_deref()
                        .is_some_and(|s| s.starts_with("M2") && !s.starts_with("M20"))
                }));
            }
            assert!(!pmi.datums.is_empty());
            assert!(!pmi.general.is_empty());
            assert!(!pmi.surface_textures.is_empty());
            let back = part::load_bytes("written.step", bytes.clone()).unwrap();
            let settings_prov = existing::part_settings_provenance(&back).unwrap();
            for key in ["material", "general_tolerance", "surface_finish"] {
                assert_eq!(settings_prov[key], MARKER, "{stem}: {key}");
            }
            assert_eq!(settings_prov["notes"], json!([MARKER]));
            let items = existing::read_existing(&back).unwrap();
            assert!(!items.is_empty());
            assert!(
                items
                    .iter()
                    .all(|e| e.provenance.as_deref() == Some(MARKER))
            );
            assert!(
                serde_json::to_value(&items)
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|e| e["provenance"] == MARKER)
            );
            if !store_answers {
                // No stored answers retains the existing additive write contract. Authorship
                // is still recognised above, and an empty subsequent edit keeps every byte.
                let empty = rules::Intent::new(serde_json::to_value(&back.binding).unwrap());
                let (unchanged, _) = writer::write_bytes(&back, &empty, None).unwrap();
                assert_eq!(bytes, unchanged);
                continue;
            }
            let mut again = intent.clone();
            again.binding = serde_json::to_value(&back.binding).unwrap();
            let (twice, _) =
                writer::write_bytes(&back, &again, store_answers.then_some(&answers)).unwrap();
            let second = all_owned(&twice);
            assert_eq!(pmi.threads.len(), second.threads.len());
            assert_eq!(pmi.knurls.len(), second.knurls.len());
            assert_eq!(pmi.dimensions.len(), second.dimensions.len());
            assert_eq!(pmi.attributes.len(), second.attributes.len());
            if store_answers {
                assert_eq!(bytes, twice, "unchanged resumed writes retain bytes");
            }
        }
    }
}

#[test]
fn imported_ap242_pmi_is_never_marked_even_when_new_pmi_uses_the_same_face() {
    let path = corpus("nist/nist_ctc_01_asme1_ap242.stp")
        .filter(|p| p.is_file())
        .unwrap_or_else(|| {
            // The exported NIST *_rd corpus is geometry-only AP203. This repository's
            // committed AP242 NIST corpus carries the semantic input PMI under test.
            eprintln!("using committed NIST CTC-01 AP242 corpus (exported *_rd is geometry-only)");
            common::case("nist/nist_ctc_01_asme1_ap242")
                .input()
                .unwrap()
        });
    let loaded = part::load(&path, None).unwrap();
    let before = existing::read_existing(&loaded).unwrap();
    assert!(!before.is_empty());
    assert!(before.iter().all(|p| p.provenance.is_none()));
    let mut intent = rules::Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    let face = before
        .iter()
        .flat_map(|p| &p.faces)
        .next()
        .copied()
        .unwrap();
    intent.requirements.push(
        json!({"kind":"flatness", "faces":[face], "tolerance":0.0123})
            .as_object()
            .unwrap()
            .clone(),
    );
    intent
        .part
        .insert("notes".into(), json!(["Specify-added instruction"]));
    let (bytes, _) = writer::write_bytes(&loaded, &intent, None).unwrap();
    let written = part::load_bytes("mixed.step", bytes.clone()).unwrap();
    let after = existing::read_existing(&written).unwrap();
    assert_eq!(
        before,
        after
            .iter()
            .filter(|p| p.provenance.is_none())
            .cloned()
            .collect::<Vec<_>>()
    );
    assert!(
        after
            .iter()
            .any(|p| p.r#type == "Flatness" && p.provenance.as_deref() == Some(MARKER))
    );
    let old = Document::parse(loaded.bytes.to_vec()).unwrap();
    let new = Document::parse(bytes).unwrap();
    for &id in old.graph().entities.keys() {
        let text = |doc: &Document| doc.bytes()[doc.span(id).unwrap()].to_vec();
        assert_eq!(text(&old), text(&new), "input #{id} changed");
    }
}

#[test]
fn dimensions_tolerances_and_caller_attributes_are_marked() {
    let loaded = part::load(&common::fixtures().join("parts/plate.step"), None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let given: Map<String, Value> = json!({"part.material":"steel", "datum.A.flatness":"0.02"})
        .as_object()
        .unwrap()
        .clone();
    let intent = rules::apply_with_defaults(&analysis, &given, true).unwrap();
    let extra = pmi::AttributeSet {
        name: "editor metadata".into(),
        on: None,
        items: vec![("literal".into(), AttributeValue::Text("unchanged".into()))],
        description: None,
        general_property: None,
    };
    let (bytes, _) = writer::write_bytes_with(&loaded, &intent, None, &[extra]).unwrap();
    let pmi = all_owned(&bytes);
    assert!(!pmi.dimensions.is_empty());
    assert!(!pmi.tolerances.is_empty());
    assert!(pmi.attributes.iter().any(|a| a.name == "editor metadata"
        && a.description.as_deref() == Some("user defined attribute")
        && a.items == [("literal".into(), AttributeValue::Text("unchanged".into()))]));
}

#[test]
fn an_unmarked_legacy_resumed_file_gains_markers_before_the_noop_shortcut() {
    let loaded = part::load(&common::fixtures().join("parts/plate.step"), None).unwrap();
    let mut intent = rules::Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    intent.part.insert("notes".into(), json!(["Legacy note"]));
    let answers = json!({"part.notes":["Legacy note"]})
        .as_object()
        .unwrap()
        .clone();
    let (bytes, _) = writer::write_bytes(&loaded, &intent, Some(&answers)).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let mut edit = haecceity::p21::Edit::new();
    for (&id, entity) in &doc.graph().entities {
        let RawEntity::Simple {
            name, attributes, ..
        } = entity
        else {
            continue;
        };
        if name == "DESCRIPTION_ATTRIBUTE"
            && matches!(attributes.first(), Some(Attribute::String(s)) if s == MARKER)
        {
            edit.remove(id);
        }
    }
    let legacy_bytes = doc.apply(&edit).unwrap().bytes;
    let legacy = part::load_bytes("legacy.step", legacy_bytes.clone()).unwrap();
    assert!(
        specify_core::resume::answers_for(&legacy)
            .unwrap()
            .is_some()
    );
    intent.binding = serde_json::to_value(&legacy.binding).unwrap();
    let (upgraded, _) = writer::write_bytes(&legacy, &intent, Some(&answers)).unwrap();
    assert_ne!(legacy_bytes, upgraded);
    all_owned(&upgraded);
    let reopened = part::load_bytes("upgraded.step", upgraded.clone()).unwrap();
    intent.binding = serde_json::to_value(&reopened.binding).unwrap();
    let (again, _) = writer::write_bytes(&reopened, &intent, Some(&answers)).unwrap();
    assert_eq!(again, upgraded);
}

#[test]
fn writing_an_identical_imported_material_does_not_claim_its_authorship() {
    let loaded = part::load(&common::fixtures().join("parts/plate.step"), None).unwrap();
    let doc = Document::parse(loaded.bytes.to_vec()).unwrap();
    let defs = read_part_definitions(&loaded.bytes).unwrap();
    let foreign = pmi::PartPmi {
        material: Some(pmi::Material {
            id: "brass".into(),
            name: None,
            density: None,
        }),
        ..Default::default()
    };
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(pmi::PartId(0), foreign)],
        pmi::write::Mode::Add,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let imported = doc.apply(&edit).unwrap().bytes;
    let loaded = part::load_bytes("imported-material.step", imported).unwrap();
    let mut intent = rules::Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    intent.part.insert("material".into(), json!("brass"));
    intent
        .part
        .insert("notes".into(), json!(["New instruction"]));
    let (bytes, _) = writer::write_bytes(&loaded, &intent, None).unwrap();
    let defs = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    assert_eq!(read.parts[0].material.as_ref().unwrap().id, "brass");
    assert!(!described(
        &doc,
        &read.provenance.parts[0].material,
        "PROPERTY_DEFINITION_REPRESENTATION"
    ));
    assert!(
        !existing::part_settings_provenance(
            &part::load_bytes("mixed.step", doc.bytes().to_vec()).unwrap()
        )
        .unwrap()
        .contains_key("material")
    );
    assert!(
        read.parts[0]
            .attributes
            .iter()
            .any(|a| a.name == "semantic text"
                && a.description.as_deref() == Some("user defined attribute"))
    );
}

#[test]
fn two_sizes_on_one_aspect_do_not_use_the_identifier_slot() {
    let loaded = part::load(&common::fixtures().join("parts/pin.step"), None).unwrap();
    let mut intent = rules::Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    for nominal in [10, 11] {
        intent.requirements.push(
            json!({"kind":"size", "faces":[0], "feature":"same aspect",
            "nominal":nominal, "upper":0.1, "lower":-0.1})
            .as_object()
            .unwrap()
            .clone(),
        );
    }
    let (bytes, _) = writer::write_bytes(&loaded, &intent, None).unwrap();
    let p = all_owned(&bytes);
    assert_eq!(p.dimensions.len(), 2);
    assert!(matches!((&p.dimensions[0].kind, &p.dimensions[1].kind),
        (pmi::DimensionKind::Size { feature: a, .. }, pmi::DimensionKind::Size { feature: b, .. }) if a == b));
    let doc = Document::parse(bytes).unwrap();
    let roots: Vec<_> = doc
        .graph()
        .entities
        .iter()
        .filter_map(|(&id, e)| {
            matches!(e, RawEntity::Simple { name, .. } if name == "DIMENSIONAL_SIZE").then_some(id)
        })
        .collect();
    assert_eq!(roots.len(), 2);
    // Explicit UR1/WR2 regression: provenance occupies none of either size's identifier slots.
    for id in roots {
        assert!(!doc.referrers(id).iter().any(|&r|
            matches!(doc.get(r), Some(RawEntity::Simple { name, .. }) if name == "ID_ATTRIBUTE")));
    }
}

#[test]
fn settings_provenance_tracks_selected_values_and_each_ordered_note() {
    let loaded = part::load(&common::fixtures().join("parts/plate.step"), None).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let given = json!({"part.material":"steel", "part.surface_finish":"Ra 3.2",
        "part.coating":"black oxide", "part.heat_treatment":"anneal", "part.edges":"Deburr only",
        "part.notes":["First instruction", "Second instruction"]})
    .as_object()
    .unwrap()
    .clone();
    let mut intent = rules::apply_with_defaults(&analysis, &given, true).unwrap();
    intent.requirements.push(
        json!({"kind":"finish", "faces":[0], "value":"Ra 1.6"})
            .as_object()
            .unwrap()
            .clone(),
    );
    let (bytes, _) = writer::write_bytes(&loaded, &intent, None).unwrap();
    all_owned(&bytes);
    let back = part::load_bytes("settings.step", bytes.clone()).unwrap();
    assert!(
        existing::read_existing(&back)
            .unwrap()
            .iter()
            .any(|row| row.kind == "note"
                && row.r#type == "surface finish"
                && row.provenance.as_deref() == Some(MARKER))
    );
    let settings = existing::part_settings(&back).unwrap();
    let sources = existing::part_settings_provenance(&back).unwrap();
    for key in [
        "material",
        "general_tolerance",
        "surface_finish",
        "coating",
        "heat_treatment",
        "edges",
    ] {
        assert!(settings.contains_key(key), "{key}");
        assert_eq!(sources[key], MARKER, "{key}");
    }
    assert_eq!(sources["notes"], json!([MARKER, MARKER]));
    let doc = Document::parse(bytes).unwrap();
    let defs = read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let mut edit = haecceity::p21::Edit::new();
    // Simulate a mixed file: an unmarked first note and material beside marked settings.
    let first = read.parts[0]
        .attributes
        .iter()
        .position(|a| {
            a.items
                .iter()
                .any(|(_, v)| *v == AttributeValue::Text("First instruction".into()))
        })
        .unwrap();
    for ids in [
        &read.provenance.parts[0].material,
        &read.provenance.parts[0].attributes[first],
    ] {
        for id in ids {
            for &r in doc.referrers(*id) {
                if matches!(doc.get(r), Some(RawEntity::Simple { name, .. }) if name == "DESCRIPTION_ATTRIBUTE")
                {
                    edit.remove(r);
                }
            }
        }
    }
    let mixed = part::load_bytes("mixed.step", doc.apply(&edit).unwrap().bytes).unwrap();
    assert_eq!(existing::part_settings(&mixed).unwrap(), settings);
    let sources = existing::part_settings_provenance(&mixed).unwrap();
    assert!(!sources.contains_key("material"));
    assert_eq!(sources["notes"], json!([null, MARKER]));
    assert_eq!(sources["coating"], MARKER);
}
