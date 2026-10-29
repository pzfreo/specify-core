//! For #65: the STEP is the inspection number authority; no feature-ID or drawing match.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use specify_core::{api, characteristics, existing, part, resume, revision, rules, writer};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}
fn load(name: &str) -> part::LoadedPart {
    part::load(&fixture(name), None).unwrap()
}
fn back(bytes: Vec<u8>) -> part::LoadedPart {
    part::load_bytes("numbered.step", bytes).unwrap()
}
fn given() -> Map<String, Value> {
    Map::new()
}
fn intent(p: &part::LoadedPart) -> rules::Intent {
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    i.datums.push(
        json!({"letter":"A", "faces":[4]})
            .as_object()
            .unwrap()
            .clone(),
    );
    for r in [
        json!({"kind":"size", "faces":[9], "feature":"holes:9", "nominal":10., "upper":0.1,"lower":-0.1}),
        json!({"kind":"location", "faces":[9], "reference":[4], "distance":20., "basic":true}),
        json!({"kind":"flatness", "faces":[4], "tolerance":0.05}),
        json!({"kind":"finish", "faces":[9], "value":"Ra 3.2"}),
    ] {
        i.requirements.push(r.as_object().unwrap().clone());
    }
    i.part.insert("surface_finish".into(), json!("Ra 6.3"));
    i
}
fn write(p: &part::LoadedPart, i: &rules::Intent) -> part::LoadedPart {
    back(writer::write_bytes(p, i, Some(&given())).unwrap().0)
}
fn numbers(p: &part::LoadedPart) -> Vec<characteristics::Characteristic> {
    characteristics::read(p).unwrap()
}
fn rebind(i: &rules::Intent, p: &part::LoadedPart) -> rules::Intent {
    let mut i = i.clone();
    i.binding = serde_json::to_value(&p.binding).unwrap();
    i
}

#[test]
fn every_kind_round_trips_standard_ids_additively_and_is_exposed_in_analysis_and_pmi() {
    let p = load("parts/plate.step");
    let i = intent(&p);
    let written = write(&p, &i);
    let ns = numbers(&written);
    assert_eq!(ns.len(), 6); // datum, size, location, FCF, face and part textures
    assert_eq!(
        ns.iter().map(|c| c.number).collect::<BTreeSet<_>>(),
        (1..=6).collect()
    );
    let doc = Document::parse(written.bytes.to_vec()).unwrap();
    assert!(haecceity::express::validate_document(doc.graph()).is_empty());
    assert!(haecceity::express_rules::check_all(&doc).is_empty());
    for n in &ns {
        assert!(doc.referrers(n.entity).iter().any(|id| matches!(doc.get(*id),
            Some(RawEntity::Simple {name,attributes,..}) if name == "ID_ATTRIBUTE"
            && attributes == &vec![Attribute::String(n.number.to_string()), Attribute::EntityRef(n.entity)])));
    }
    // haecceity's append preserves every source record exactly (the AP214 schema header
    // alone may become AP242); numbers never cause geometry or original PMI rewrites.
    let original = Document::parse(p.bytes.to_vec()).unwrap();
    for id in original.ids() {
        assert_eq!(
            original.span(id).map(|s| &original.bytes()[s]),
            doc.span(id).map(|s| &doc.bytes()[s]),
            "#{id}"
        );
    }
    let analysis = api::analyse(&written).unwrap();
    assert_eq!(
        analysis["characteristics"],
        serde_json::to_value(&ns).unwrap()
    );
    assert!(
        analysis["existing"].as_array().unwrap().is_empty(),
        "resumed interview stays geometry-only"
    );
    let existing = existing::read_existing(&written).unwrap();
    assert!(existing.iter().all(|e| e.number.is_some()));
    assert_eq!(
        existing.len(),
        5,
        "part finish is in the authoritative characteristics list"
    );
    // Independent fresh runs and a resumed run allocate identical numbers; an unchanged
    // resumed run also retains every byte, including original entity IDs and presentation.
    assert_eq!(written.bytes, write(&p, &i).bytes);
    assert_eq!(written.bytes, write(&written, &rebind(&i, &written)).bytes);
}

#[test]
fn additions_do_not_renumber_removals_retire_even_after_an_empty_write_and_values_can_change() {
    let p = load("parts/plate.step");
    let mut i = intent(&p);
    let first = write(&p, &i);
    let old = numbers(&first);
    i = rebind(&i, &first);
    i.requirements.insert(
        0,
        json!({"kind":"flatness", "faces":[5], "tolerance":0.02})
            .as_object()
            .unwrap()
            .clone(),
    );
    let added = write(&first, &i);
    let ns = numbers(&added);
    for n in &old {
        let now = ns
            .iter()
            .find(|c| {
                c.kind == n.kind
                    && c.r#type == n.r#type
                    && c.faces == n.faces
                    && c.reference == n.reference
            })
            .unwrap();
        assert_eq!(now.number, n.number);
    }
    assert_eq!(ns.iter().find(|c| c.faces == [5]).unwrap().number, 7);
    i = rebind(&i, &added);
    i.requirements.remove(0);
    i.requirements[0].insert("upper".into(), json!(0.2));
    let removed = write(&added, &i);
    assert_eq!(
        numbers(&removed)
            .iter()
            .map(|c| c.number)
            .collect::<Vec<_>>(),
        old.iter().map(|c| c.number).collect::<Vec<_>>()
    );
    i = rebind(&i, &removed);
    i.requirements.insert(
        0,
        json!({"kind":"flatness", "faces":[5], "tolerance":0.02})
            .as_object()
            .unwrap()
            .clone(),
    );
    let readded = write(&removed, &i);
    assert_eq!(
        numbers(&readded)
            .iter()
            .find(|c| c.faces == [5])
            .unwrap()
            .number,
        8
    );
    let empty = write(
        &readded,
        &rules::Intent::new(serde_json::to_value(&readded.binding).unwrap()),
    );
    assert!(numbers(&empty).is_empty());
    let mut i = rules::Intent::new(serde_json::to_value(&empty.binding).unwrap());
    i.requirements.push(
        json!({"kind":"flatness", "faces":[5], "tolerance":0.02})
            .as_object()
            .unwrap()
            .clone(),
    );
    assert_eq!(numbers(&write(&empty, &i))[0].number, 9);
}

#[test]
fn general_iso2768_rows_have_no_entities_and_allocate_no_numbers() {
    let p = load("parts/plate.step");
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    i.part
        .insert("general_tolerance".into(), json!("ISO 2768-mK"));
    i.general.push(
        json!({"kind":"size","faces":[9],"nominal":10.})
            .as_object()
            .unwrap()
            .clone(),
    );
    let result = write(&p, &i);
    assert!(numbers(&result).is_empty());
    assert!(
        api::analyse(&result)
            .unwrap()
            .get("characteristics")
            .is_none()
    );
    assert!(!String::from_utf8_lossy(&result.bytes).contains(characteristics::PROPERTY));
}

#[test]
fn legacy_numberless_files_have_byte_identical_analysis_and_pmi_serialization() {
    // Captured by the unmodified parent f3f4410e070b345b388a2fd9369a9f9decf0e252
    // in a separate checkout: byte comparisons against this change cannot bless themselves.
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/characteristic-numbers-legacy.json")).unwrap();
    let hash = |value: &Value| {
        Sha256::digest(serde_json::to_vec(value).unwrap())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    for (name, baseline) in expected.as_object().unwrap() {
        let p = load(name);
        assert!(numbers(&p).is_empty());
        let a = api::analyse(&p).unwrap();
        assert!(a.get("characteristics").is_none());
        let pmi = serde_json::to_value(existing::read_existing(&p).unwrap()).unwrap();
        assert_eq!(
            hash(&a),
            baseline["analysis_sha256"],
            "{name}: analysis bytes"
        );
        assert_eq!(hash(&pmi), baseline["pmi_sha256"], "{name}: PMI read bytes");
        let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
        i.part.insert("material".into(), json!("steel"));
        let answers = json!({"part.material":"steel"})
            .as_object()
            .unwrap()
            .clone();
        let output = writer::write_bytes(&p, &i, Some(&answers)).unwrap().0;
        let actual = Sha256::digest(output)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(
            actual, baseline["settings_only_writer_sha256"],
            "{name}: unchanged settings-only writer"
        );
    }
}

#[test]
fn revised_part_correspondence_carries_numbers_and_retirement_with_the_answers() {
    let old = load("revisions/hole_removed.old.step.gz");
    let new = load("revisions/hole_removed.new.step.gz");
    let a = api::analyse(&old).unwrap();
    let answers = json!({"hole.function:holes:6":"fit:H7", "hole.function:holes:7":"fit:H7", "hole.function:holes:8":"fit:H7", "hole.function:holes:9":"fit:H7"}).as_object().unwrap().clone();
    let i = api::preview(&a, &answers).unwrap();
    let old = back(writer::write_bytes(&old, &i, Some(&answers)).unwrap().0);
    let ns = numbers(&old);
    assert_eq!(ns.len(), 4);
    let revised = revision::diff(&old, &new, &Map::new()).unwrap();
    assert!(revised.answers.contains_key(characteristics::CARRY));
    let i = api::preview(&api::analyse(&new).unwrap(), &revised.answers).unwrap();
    let written = back(
        writer::write_bytes(&new, &i, Some(&revised.answers))
            .unwrap()
            .0,
    );
    let now = numbers(&written);
    assert_eq!(now.len(), 3);
    for n in ns.iter().filter(|n| n.faces != [7]) {
        let mapped = if n.faces[0] > 7 {
            n.faces[0] - 1
        } else {
            n.faces[0]
        };
        assert_eq!(
            now.iter().find(|c| c.faces == [mapped]).unwrap().number,
            n.number
        );
    }
    assert!(
        !resume::answers_for(&written)
            .unwrap()
            .unwrap()
            .contains_key(characteristics::CARRY),
        "transport is consumed, never reapplied on resume"
    );
    let mut i = rebind(&i, &written);
    i.requirements.push(
        json!({"kind":"flatness", "faces":[4],"tolerance":0.02})
            .as_object()
            .unwrap()
            .clone(),
    );
    let final_part = write(&written, &i);
    assert_eq!(
        numbers(&final_part)
            .iter()
            .find(|c| c.kind == "geometric_tolerance")
            .unwrap()
            .number,
        ns.iter().map(|c| c.number).max().unwrap() + 1
    );
}

#[test]
fn older_quiddity_written_pmi_keeps_numbers_on_upgrade_to_023() {
    let p = load("resume-alpha6/ftc10-reid.step.gz");
    // Add standard numeric ids to the committed OLD writer's PMI without regenerating any
    // old semantic records. This simulates numbering in a producer predating this kernel.
    let doc = Document::parse(p.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&p.bytes).unwrap();
    let read = haecceity::pmi::read(&doc, &defs).unwrap();
    let mut edit = Edit::new();
    let mut count = 0;
    for ids in &read.provenance.parts[0].dimensions {
        for &id in ids {
            if matches!(doc.get(id), Some(RawEntity::Simple {name,..}) if name == "DIMENSIONAL_SIZE" || name == "DIMENSIONAL_LOCATION")
            {
                count += 1;
                edit.add(haecceity::p21::simple(
                    "ID_ATTRIBUTE",
                    vec![
                        Attribute::String((100 + count).to_string()),
                        Attribute::EntityRef(id),
                    ],
                ));
            }
        }
    }
    assert!(count > 0);
    let annotated = back(doc.apply(&edit).unwrap().bytes);
    let old_numbers = numbers(&annotated);
    let a = api::analyse(&annotated).unwrap();
    assert!(
        !a["resume"]["migrated"].as_array().unwrap().is_empty(),
        "fixture actually crosses changed recognition IDs"
    );
    let answers = a["resume"]["answers"].as_object().unwrap();
    let i = api::preview(&a, answers).unwrap();
    let written = back(
        writer::write_bytes(&annotated, &i, Some(answers))
            .unwrap()
            .0,
    );
    let now = numbers(&written);
    let mut retained = 0;
    for n in old_numbers {
        let candidates: Vec<_> = now
            .iter()
            .filter(|c| {
                c.kind == n.kind
                    && c.r#type == n.r#type
                    && n.faces.iter().all(|f| c.faces.contains(f))
                    && n.reference.iter().all(|f| c.reference.contains(f))
            })
            .collect();
        // Current rules deliberately no longer write some old default-only rows.
        if candidates.is_empty() {
            continue;
        }
        retained += 1;
        assert!(
            candidates.iter().any(|c| c.number == n.number),
            "{n:?}: {candidates:?}"
        );
    }
    assert!(
        retained > 20,
        "a real old-engine PMI population survives: {retained}"
    );
}

#[test]
fn an_added_control_on_the_same_face_cannot_take_an_existing_controls_number() {
    let p = load("parts/plate.step");
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    i.requirements.push(
        json!({"kind":"flatness","faces":[4],"tolerance":0.05})
            .as_object()
            .unwrap()
            .clone(),
    );
    let first = write(&p, &i);
    i = rebind(&i, &first);
    i.requirements.insert(
        0,
        json!({"kind":"flatness","faces":[4],"tolerance":0.02})
            .as_object()
            .unwrap()
            .clone(),
    );
    let added = write(&first, &i);
    assert_eq!(
        numbers(&added).iter().map(|c| c.number).collect::<Vec<_>>(),
        [2, 1]
    );
    i = rebind(&i, &added);
    i.requirements.remove(1);
    let removed = write(&added, &i);
    assert_eq!(numbers(&removed)[0].number, 2);
}

#[test]
fn foreign_pmi_append_keeps_original_bytes_and_numbers_even_for_equal_requirements() {
    let p = load("parts/plate.step");
    let i = intent(&p);
    // Without stored answers the second write uses lossless Add, including a shared datum.
    let first = back(writer::write_bytes(&p, &i, None).unwrap().0);
    let original = numbers(&first);
    let added = back(
        writer::write_bytes(&first, &rebind(&i, &first), None)
            .unwrap()
            .0,
    );
    let after = numbers(&added);
    assert_eq!(after.len(), 11); // the existing datum is reused, five new requirements
    assert_eq!(
        after.iter().map(|c| c.number).collect::<BTreeSet<_>>(),
        (1..=11).collect()
    );
    for n in original {
        assert!(after.contains(&n));
    }
    let doc = Document::parse(first.bytes.to_vec()).unwrap();
    let new = Document::parse(added.bytes.to_vec()).unwrap();
    for id in doc.ids() {
        assert_eq!(
            doc.span(id).map(|s| &doc.bytes()[s]),
            new.span(id).map(|s| &new.bytes()[s])
        );
    }
}

#[test]
fn stale_revision_transport_cannot_reuse_numbers_retired_in_the_destination_file() {
    let old = load("revisions/hole_removed.old.step.gz");
    let new = load("revisions/hole_removed.new.step.gz");
    let answers = json!({"hole.function:holes:6":"fit:H7"})
        .as_object()
        .unwrap()
        .clone();
    let i = api::preview(&api::analyse(&old).unwrap(), &answers).unwrap();
    let old = back(writer::write_bytes(&old, &i, Some(&answers)).unwrap().0);
    let r = revision::diff(&old, &new, &Map::new()).unwrap();
    let mut i = api::preview(&api::analyse(&new).unwrap(), &r.answers).unwrap();
    let new = back(writer::write_bytes(&new, &i, Some(&r.answers)).unwrap().0);
    i = rebind(&i, &new);
    let extra = json!({"kind":"flatness","faces":[4],"tolerance":0.02})
        .as_object()
        .unwrap()
        .clone();
    i.requirements.push(extra.clone());
    let added = back(writer::write_bytes(&new, &i, Some(&r.answers)).unwrap().0);
    let retired = numbers(&added)
        .into_iter()
        .find(|c| c.kind == "geometric_tolerance")
        .unwrap()
        .number;
    i = rebind(&i, &added);
    i.requirements.pop();
    let removed = back(writer::write_bytes(&added, &i, Some(&r.answers)).unwrap().0);
    i = rebind(&i, &removed);
    i.requirements.push(extra);
    let readded = back(
        writer::write_bytes(&removed, &i, Some(&r.answers))
            .unwrap()
            .0,
    );
    assert!(
        numbers(&readded)
            .iter()
            .find(|c| c.kind == "geometric_tolerance")
            .unwrap()
            .number
            > retired
    );
    // The transport is also bound to its intended target geometry, even on a direct writer
    // call that does not go through the browser's answer-shape checks.
    let unrelated = load("parts/pin.step");
    let empty = rules::Intent::new(serde_json::to_value(&unrelated.binding).unwrap());
    assert!(
        writer::write_bytes(&unrelated, &empty, Some(&r.answers))
            .unwrap_err()
            .to_string()
            .contains("different geometry")
    );
}

#[test]
fn imported_shared_features_numeric_ids_are_reserved_without_numbering_old_requirements() {
    let p = load("parts/plate.step");
    let mut i = rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    i.requirements.push(
        json!({"kind":"size","faces":[9],"nominal":10.,"upper":0.1,"lower":-0.1})
            .as_object()
            .unwrap()
            .clone(),
    );
    let numbered = write(&p, &i);
    let d = Document::parse(numbered.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&numbered.bytes).unwrap();
    let mut model = haecceity::pmi::read(&d, &defs).unwrap().parts.remove(0);
    model.attributes.clear();
    // Import the semantic requirement without inspection identity, using haecceity alone.
    let original = Document::parse(p.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(&p.bytes).unwrap();
    let (edit, _) = haecceity::pmi::write(
        &original,
        &defs,
        &[(haecceity::pmi::PartId(0), model)],
        haecceity::pmi::write::Mode::Add,
        haecceity::pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    let imported = Document::parse(original.apply(&edit).unwrap().bytes).unwrap();
    let read = haecceity::pmi::read(&imported, &defs).unwrap();
    let feature = read.provenance.parts[0].features[0].iter().copied().find(|id|
        matches!(imported.get(*id), Some(RawEntity::Simple {name,..}) if name == "SHAPE_ASPECT")).unwrap();
    let mut edit = Edit::new();
    edit.add(haecceity::p21::simple(
        "ID_ATTRIBUTE",
        vec![
            Attribute::String("40".into()),
            Attribute::EntityRef(feature),
        ],
    ));
    let imported = back(imported.apply(&edit).unwrap().bytes);
    assert!(
        numbers(&imported).is_empty(),
        "a geometric feature id is not a requirement number"
    );
    let added = back(
        writer::write_bytes(&imported, &rebind(&i, &imported), None)
            .unwrap()
            .0,
    );
    assert_eq!(numbers(&added)[0].number, 41);
    assert!(
        existing::read_existing(&added).unwrap()[0].number.is_none(),
        "the imported requirement remains unnumbered"
    );
}

#[test]
fn a_datum_requirement_number_is_not_overridden_by_its_geometric_features_id() {
    let p = load("parts/plate.step");
    let p = write(&p, &intent(&p));
    let required = numbers(&p)
        .iter()
        .find(|c| c.kind == "datum")
        .unwrap()
        .number;
    let doc = Document::parse(p.bytes.to_vec()).unwrap();
    let feature = doc
        .ids()
        .find(|id| {
            matches!(doc.get(*id),
        Some(RawEntity::Simple {name,..}) if name == "DATUM_FEATURE")
        })
        .unwrap();
    let mut edit = Edit::new();
    edit.add(haecceity::p21::simple(
        "ID_ATTRIBUTE",
        vec![
            Attribute::String("40".into()),
            Attribute::EntityRef(feature),
        ],
    ));
    let p = back(doc.apply(&edit).unwrap().bytes);
    assert_eq!(
        numbers(&p)
            .iter()
            .find(|c| c.kind == "datum")
            .unwrap()
            .number,
        required
    );
    assert_eq!(
        existing::read_existing(&p)
            .unwrap()
            .iter()
            .find(|c| c.kind == "datum")
            .unwrap()
            .number,
        Some(required)
    );
}

#[test]
fn surface_texture_pmi_reads_the_number_from_its_root_even_when_parameters_have_lower_ids() {
    let p = load("parts/plate.step");
    let p = write(&p, &intent(&p));
    let expected = numbers(&p)
        .into_iter()
        .find(|c| c.kind == "surface_texture" && !c.faces.is_empty())
        .unwrap();
    let doc = Document::parse(p.bytes.to_vec()).unwrap();
    let parameter = doc.ids().find(|id| matches!(doc.get(*id), Some(RawEntity::Simple {name,attributes,..})
        if name == "PROPERTY_DEFINITION" && matches!(attributes.first(), Some(Attribute::String(s)) if s == "surface_condition"))).unwrap();
    assert!(parameter > expected.entity);
    let swap = |id| {
        if id == parameter {
            expected.entity
        } else if id == expected.entity {
            parameter
        } else {
            id
        }
    };
    fn refs(a: &mut Attribute, swap: &impl Fn(u64) -> u64) {
        match a {
            Attribute::EntityRef(id) => *id = swap(*id),
            Attribute::List(items) => {
                for a in items {
                    refs(a, swap);
                }
            }
            Attribute::Typed { value, .. } => refs(value, swap),
            _ => {}
        }
    }
    let mut edit = Edit::new();
    for id in doc.ids() {
        let mut e = doc.get(id).unwrap().clone();
        match &mut e {
            RawEntity::Simple { id, attributes, .. } => {
                *id = swap(*id);
                for a in attributes {
                    refs(a, &swap);
                }
            }
            RawEntity::Complex { id, parts, .. } => {
                *id = swap(*id);
                for part in parts {
                    for a in &mut part.attributes {
                        refs(a, &swap);
                    }
                }
            }
        }
        edit.replace(swap(id), e);
    }
    let p = back(doc.apply(&edit).unwrap().bytes);
    let public = existing::read_existing(&p).unwrap();
    assert_eq!(
        public
            .iter()
            .find(|c| c.kind == "note" && c.r#type == "surface finish")
            .unwrap()
            .number,
        Some(expected.number)
    );
    assert_eq!(
        numbers(&p)
            .iter()
            .find(|c| c.entity == parameter)
            .unwrap()
            .number,
        expected.number
    );
}
