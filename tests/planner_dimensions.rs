//! #75: the shared planner is the selection authority; STEP keeps its complete quantities.
use haecceity::{p21::Document, pmi};
use quiddity::dimensioning::{Input, PatternRole, Required, Tolerance};
use serde_json::{Value, json};
use specify_core::{api, characteristics, part, rules, writer};
use std::{collections::BTreeSet, path::Path};

fn plate() -> part::LoadedPart {
    part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/plate.step"),
        None,
    )
    .unwrap()
}
fn required(i: &rules::Intent) -> Vec<Required> {
    let mut rs: Vec<Required> = i
        .requirements
        .iter()
        .chain(&i.general)
        .filter_map(|r| r.get("planner"))
        .map(|p| serde_json::from_value(p["required"].clone()).unwrap())
        .collect();
    rs.sort_by(|a, b| a.id.cmp(&b.id));
    rs
}
fn model(p: &part::LoadedPart) -> pmi::PmiRead {
    pmi::read(
        &Document::parse(p.bytes.to_vec()).unwrap(),
        std::slice::from_ref(&p.definition),
    )
    .unwrap()
}
#[test]
fn plate_writes_every_required_quantity_and_basics_use_the_pattern_origin() {
    let p = plate();
    let a = api::analyse(&p).unwrap();
    let given = json!({"part.material":"steel"});
    let i = rules::apply_with_defaults(&a, given.as_object().unwrap(), true).unwrap();
    let rs = required(&i);
    // 11 all-General quantities + two general offsets that cannot merge with Basic origins.
    assert_eq!(rs.len(), 13);
    let mut input: Input = serde_json::from_value(a["dimensioning"].clone()).unwrap();
    input.datums = i
        .datums
        .iter()
        .map(|d| quiddity::dimensioning::Datum {
            label: d["letter"].as_str().unwrap().into(),
            faces: serde_json::from_value(d["faces"].clone()).unwrap(),
        })
        .collect();
    assert_eq!(quiddity::dimensioning::plan(&input).0.len(), 11);
    input.specified = rs
        .iter()
        .filter(|r| r.id.starts_with("specified:") || r.id.starts_with("answered:"))
        .cloned()
        .collect();
    assert_eq!(quiddity::dimensioning::plan(&input).0, rs);
    let centres: Vec<_> = rs
        .iter()
        .filter(|r| r.tolerance == Tolerance::Basic && r.pattern == Some(PatternRole::Centre))
        .collect();
    assert!(centres.iter().any(|r| (r.nominal - 50.).abs() < 1e-6));
    // The shared 30 also covers the pattern centre; every pitch is basic for the FCF.
    assert!(rs.iter().any(|r| (r.nominal - 30.).abs() < 1e-6
        && r.tolerance == Tolerance::Basic
        && r.covers.len() >= 6));
    let pitches: Vec<_> = rs
        .iter()
        .filter(|r| matches!(r.pattern, Some(PatternRole::Pitch { .. })))
        .collect();
    assert_eq!(pitches.len(), 2);
    assert!(pitches.iter().all(|r| r.tolerance == Tolerance::Basic));
    let (bytes, _) = writer::write_bytes(&p, &i, Some(given.as_object().unwrap())).unwrap();
    let written = part::load_bytes("planned.step", bytes).unwrap();
    let read = model(&written);
    assert_eq!(read.parts[0].dimensions.len(), rs.len());
    let mut stored: Vec<Required> = read.parts[0]
        .attributes
        .iter()
        .filter(|a| a.name == "specify dimension plan")
        .map(|a| match &a.items[0].1 {
            pmi::AttributeValue::Text(s) => serde_json::from_value(
                serde_json::from_str::<Value>(s).unwrap()["required"].clone(),
            )
            .unwrap(),
            _ => panic!("typed requirement"),
        })
        .collect();
    stored.sort_by(|a, b| a.id.cmp(&b.id));
    // Persistent JSON text rounds frame noise to nine places for native/WASM parity.
    // Requirement identities, grouping and tolerance classes remain exact.
    let mut expected = serde_json::to_value(&rs).unwrap();
    fn canonical(v: &mut Value) {
        match v {
            Value::Number(n) if n.is_f64() => {
                *v = json!((n.as_f64().unwrap() * 1e9).round() / 1e9 + 0.0);
            }
            Value::Array(a) => a.iter_mut().for_each(canonical),
            Value::Object(m) => m.values_mut().for_each(canonical),
            _ => {}
        }
    }
    canonical(&mut expected);
    assert_eq!(serde_json::to_value(stored).unwrap(), expected);
    assert!(read.parts[0].features.iter().any(|f| matches!(
        f,
        pmi::Feature::Group {
            kind: pmi::GroupKind::PatternOfFeatures,
            ..
        }
    )));
    assert_eq!(characteristics::read(&written).unwrap().len(), 20);
}

#[test]
fn an_existing_written_plate_retires_member_numbers_and_never_reuses_them() {
    let old = part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/planner75/plate-v028.step.gz"),
        None,
    )
    .unwrap();
    let before = characteristics::read(&old).unwrap();
    assert_eq!(before.len(), 28);
    assert_eq!(before.iter().filter(|c| c.kind == "dimension").count(), 21);
    let a = api::analyse(&old).unwrap();
    let given = a["resume"]["answers"].as_object().unwrap();
    let i = rules::apply_with_defaults(&a, given, true).unwrap();
    let (bytes, report) = writer::write_bytes(&old, &i, Some(given)).unwrap();
    assert!(report.warnings.contains(
        &"retired characteristic numbers: 6, 7, 8, 9, 10, 11, 21, 22, 23, 24, 25, 26".into()
    ));
    let new = part::load_bytes("planned.step", bytes).unwrap();
    let after = characteristics::read(&new).unwrap();
    assert_eq!(after.len(), 20);
    let max = before.iter().map(|c| c.number).max().unwrap();
    for prior in &before {
        if let Some(now) = after.iter().find(|c| {
            c.kind == prior.kind
                && c.r#type == prior.r#type
                && c.faces == prior.faces
                && c.reference == prior.reference
        }) {
            assert_eq!(now.number, prior.number, "unchanged {prior:?}");
        }
    }
    let old_numbers: BTreeSet<_> = before.iter().map(|c| c.number).collect();
    let new_numbers: BTreeSet<_> = after.iter().map(|c| c.number).collect();
    let retired: BTreeSet<_> = old_numbers.difference(&new_numbers).copied().collect();
    assert_eq!(retired, (6..=11).chain(21..=26).collect());
    assert_eq!(
        new_numbers
            .difference(&old_numbers)
            .copied()
            .collect::<BTreeSet<_>>(),
        (29..=32).collect()
    );
    assert_eq!(
        after
            .iter()
            .find(|c| c.faces == [2] && c.reference == [4])
            .unwrap()
            .number,
        5,
        "the reversed 10 mm extent keeps its number"
    );
    assert!(
        after
            .iter()
            .filter(|c| !old_numbers.contains(&c.number))
            .all(|c| c.number > max)
    );
    assert_eq!(new_numbers.len(), after.len());
    let a = api::analyse(&new).unwrap();
    let given = a["resume"]["answers"].as_object().unwrap();
    let i = rules::apply_with_defaults(&a, given, true).unwrap();
    let (again, _) = writer::write_bytes(&new, &i, Some(given)).unwrap();
    assert!(again == *new.bytes, "unchanged resume bytes");
    // A new answered inspection requirement must allocate above the entire old history.
    let mut given = given.clone();
    given.insert(
        "extra.new".into(),
        json!({"kind":"flatness","faces":[5],"tolerance":0.02}),
    );
    let mut i = i;
    i.requirements.push(
        json!({"kind":"flatness","faces":[5],"tolerance":0.02})
            .as_object()
            .unwrap()
            .clone(),
    );
    let (bytes, _) = writer::write_bytes(&new, &i, Some(&given)).unwrap();
    let added = part::load_bytes("added.step", bytes).unwrap();
    let numbers = characteristics::read(&added).unwrap();
    assert!(numbers.iter().all(|c| !retired.contains(&c.number)));
    assert!(
        numbers
            .iter()
            .any(|c| c.number > new_numbers.iter().copied().max().unwrap())
    );
    eprintln!("retired plate numbers: {retired:?}");
}

#[test]
fn reference_flag_is_written_as_a_standard_auxiliary_dimension() {
    let p = plate();
    let a = api::analyse(&p).unwrap();
    let given = json!({"part.general_tolerance":"ISO 2768-m"});
    let mut i = api::preview(&a, given.as_object().unwrap()).unwrap();
    let r = i
        .general
        .iter_mut()
        .find(|r| r["kind"] == "location")
        .unwrap();
    r["planner"]["required"]["reference"] = json!(["determining-plane-path"]);
    r["planner"]["required"]["tolerance"] = json!("untoleranced");
    let (bytes, _) = writer::write_bytes(&p, &i, Some(given.as_object().unwrap())).unwrap();
    let p = part::load_bytes("reference.step", bytes).unwrap();
    let read = model(&p);
    assert_eq!(
        read.parts[0]
            .dimensions
            .iter()
            .filter(|d| d.modifiers.contains(&pmi::DimensionModifier::Reference))
            .count(),
        1
    );
}

#[test]
fn stored_dimensions_enter_the_planner_and_are_kept_once_in_add_mode() {
    let p = plate();
    let mut i = rules::Intent::new(json!(p.binding));
    i.requirements.push(json!({"kind":"size","faces":[9],"feature":"holes:9","nominal":8.,"upper":0.02,"lower":-0.02}).as_object().unwrap().clone());
    let (bytes, _) = writer::write_bytes(&p, &i, None).unwrap();
    let imported = part::load_bytes("imported.step", bytes).unwrap();
    let a = api::analyse(&imported).unwrap();
    assert_eq!(a["dimensioning_stated"].as_array().unwrap().len(), 1);
    let given = json!({"part.material":"steel"});
    let i = rules::apply_with_defaults(&a, given.as_object().unwrap(), true).unwrap();
    let rs = required(&i);
    assert!(
        rs.iter()
            .any(|r| r.id.starts_with("stored:") && r.nominal == 8.)
    );
    assert!(!rs.iter().any(|r| r.id == "dim:hole:9/bore.diameter"));
    let (bytes, _) = writer::write_bytes(&imported, &i, None).unwrap();
    let written = part::load_bytes("added.step", bytes).unwrap();
    let read = model(&written);
    assert_eq!(
        read.parts[0]
            .dimensions
            .iter()
            .filter(|d| d.nominal.as_ref().is_some_and(
                |v| matches!(&v.quantity,pmi::Quantity::Length(l) if l.value.to_f64()==8.)
            ))
            .count(),
        1
    );
}

#[test]
fn synthetic_supports_are_converted_to_the_source_inch_context() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/nist/nist_ctc_05_asme1_ap242.stp.gz");
    let p = part::load(&path, Some(0)).unwrap();
    let given = json!({"part.general_tolerance":"ISO 2768-m"});
    let i = api::preview(&api::analyse(&p).unwrap(), given.as_object().unwrap()).unwrap();
    let (bytes, _) = writer::write_bytes(&p, &i, None).unwrap();
    let p = part::File::of_bytes(Path::new("inch.step"), bytes)
        .unwrap()
        .load(Some(0))
        .unwrap();
    let read = model(&p);
    let points: Vec<_> = read.parts[0]
        .geometry
        .iter()
        .filter(|g| {
            g.item.leaves.iter().any(|(name, attrs)| {
                name == "CARTESIAN_POINT"
                    && attrs.first()
                        == Some(&pmi::GeometryValue::Text(writer::PMI_PROVENANCE.into()))
            })
        })
        .collect();
    assert!(!points.is_empty());
    assert!(
        points
            .iter()
            .all(|g| g.length_unit == Some(pmi::LengthUnit::Inch))
    );
}

#[test]
fn stored_pattern_origin_is_a_specified_authority_not_a_second_origin() {
    let p = plate();
    let mut i = rules::Intent::new(json!(p.binding));
    i.requirements.push(json!({"kind":"location", "faces":[6,7,8,10,11,13], "reference":[0], "distance":50., "upper":0.03, "lower":-0.03}).as_object().unwrap().clone());
    let (bytes, _) = writer::write_bytes(&p, &i, None).unwrap();
    let imported = part::load_bytes("origin.step", bytes).unwrap();
    let a = api::analyse(&imported).unwrap();
    let given = json!({"part.general_tolerance":"ISO 2768-m"});
    let i = api::preview(&a, given.as_object().unwrap()).unwrap();
    let rs = required(&i);
    let stored: Vec<_> = rs.iter().filter(|r| r.id.starts_with("stored:")).collect();
    assert_eq!(stored.len(), 1);
    assert!(
        matches!(&stored[0].kind, quiddity::dimensioning::Kind::Location { to, .. } if to.feature.starts_with("pattern["))
    );
    assert!(!rs.iter().any(|r| matches!(&r.kind, quiddity::dimensioning::Kind::Location { from, to, axis } if matches!(&stored[0].kind, quiddity::dimensioning::Kind::Location { from: a, to: b, axis: c } if from==a && to==b && axis==c)) && !r.id.starts_with("stored:")));
    let (bytes, _) = writer::write_bytes(&imported, &i, None).unwrap();
    let written = part::load_bytes("origin-added.step", bytes).unwrap();
    let read = model(&written);
    assert_eq!(
        read.parts[0].dimensions.len(),
        rs.len(),
        "stored origin is already present exactly once"
    );
}
