//! A general tolerance given as a block (#228; proposal specify-0.3.0, "A general tolerance
//! given as a block"): the four worked examples, a table another tool wrote, a change that
//! replaces the table, and the other two points of #228.

mod common;

use haecceity::p21::Document;
use haecceity::pmi::{self, CellValue, GeneralTolerance, LengthUnit, PartPmi, Quantity};
use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::part::{LoadedPart, load, load_bytes};
use specify_core::writer::WriteReport;

fn map(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn plate() -> LoadedPart {
    load(&common::fixtures().join("parts/plate.step"), None).unwrap()
}

/// The #228 half-shell (from the issue's gist): an inch shop drawing's part, its STEP in
/// millimetres.
fn half_shell() -> LoadedPart {
    load(
        &common::fixtures().join("issue228-half-shell.step.gz"),
        None,
    )
    .unwrap()
}

/// Saves `answers` as the editor does, open questions and all (`session::intent_for`).
fn save(loaded: &LoadedPart, answers: &Map<String, Value>) -> (LoadedPart, WriteReport) {
    let analysis = api::analyse(loaded).unwrap();
    let (intent, _open) = specify_core::session::intent_for(loaded, &analysis, answers).unwrap();
    let (bytes, report) =
        specify_core::writer::write_bytes(loaded, &intent, Some(answers)).unwrap();
    (load_bytes("saved.step", bytes).unwrap(), report)
}

fn pmi_of(loaded: &LoadedPart) -> PartPmi {
    specify_core::existing::read(loaded)
        .unwrap()
        .model()
        .clone()
}

/// The general tolerance row of the feature table: (state, value).
fn general_row(loaded: &LoadedPart) -> (String, Value) {
    let analysis = api::analyse(loaded).unwrap();
    let row = analysis["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "part.general_tolerance")
        .unwrap_or_else(|| panic!("no general tolerance row"))
        .clone();
    (
        row["state"].as_str().unwrap().to_string(),
        row["value"].clone(),
    )
}

/// The general tolerance note's text, if the part has one.
fn note(p: &PartPmi) -> Option<String> {
    p.attributes.iter().find_map(|set| {
        set.items.iter().find_map(|(name, value)| match value {
            pmi::AttributeValue::Text(t) if name == "general tolerance" && set.on.is_none() => {
                Some(t.clone())
            }
            _ => None,
        })
    })
}

/// Each table cell's (digits, tolerance text, unit).
fn cells(p: &PartPmi) -> Vec<(String, String, LengthUnit)> {
    let tables: Vec<_> = p
        .general
        .iter()
        .filter_map(|g| match g {
            GeneralTolerance::Table { cells, .. } => Some(cells),
            GeneralTolerance::Class { .. } => None,
        })
        .collect();
    assert_eq!(tables.len(), 1, "one table: {:?}", p.general);
    tables[0]
        .iter()
        .map(|c| {
            let digits = c
                .items
                .iter()
                .find_map(|(n, v)| match v {
                    CellValue::Count(d) if n == "significant number of digits" => {
                        Some(d.as_str().to_string())
                    }
                    _ => None,
                })
                .unwrap();
            let (value, unit) = c
                .items
                .iter()
                .find_map(|(n, v)| match v {
                    CellValue::Value(v) if n == "plus minus tolerance value" => match &v.quantity {
                        Quantity::Length(l) => Some((l.value.as_str().to_string(), l.unit.clone())),
                        Quantity::Angle(_) => None,
                    },
                    _ => None,
                })
                .unwrap();
            (digits, value, unit)
        })
        .collect()
}

fn classes(p: &PartPmi) -> usize {
    p.general
        .iter()
        .filter(|g| matches!(g, GeneralTolerance::Class { .. }))
        .count()
}

// --- Example 1: the #228 half-shell ---------------------------------------------------------

#[test]
fn example_1_the_228_half_shell_gets_an_inch_table_and_a_note_as_stated() {
    let part = half_shell();
    let before = pmi_of(&part);
    let answers = map(json!({
        "part.general_tolerance": ".XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°",
    }));
    let (saved, report) = save(&part, &answers);
    let p = pmi_of(&saved);
    assert_eq!(
        cells(&p),
        vec![
            ("2".into(), "0.01".into(), LengthUnit::Inch),
            ("3".into(), "0.005".into(), LengthUnit::Inch),
        ],
        "{:?}",
        report.written
    );
    assert_eq!(note(&p).as_deref(), Some("±1/64; angles ±1°"));
    // The general tolerance writes only itself: no ISO class, no dimension.
    assert_eq!(classes(&p), 0);
    assert_eq!(p.dimensions.len(), before.dimensions.len());
    assert_eq!(p.tolerances.len(), before.tolerances.len());
    let (state, value) = general_row(&saved);
    assert_eq!(state, "stated");
    assert_eq!(value, ".XX ±.01 · .XXX ±.005 · ±1/64 · angles ±1°");
}

// --- Example 2: ISO 2768-m, as today ---------------------------------------------------------

#[test]
fn example_2_iso_2768_m_is_a_class_as_today() {
    let (saved, _) = save(
        &plate(),
        &map(json!({"part.general_tolerance": "ISO 2768-m"})),
    );
    let p = pmi_of(&saved);
    assert_eq!(classes(&p), 1);
    assert!(
        !p.general
            .iter()
            .any(|g| matches!(g, GeneralTolerance::Table { .. }))
    );
    assert_eq!(note(&p), None);
    assert_eq!(general_row(&saved), ("stated".into(), json!("ISO 2768-m")));
}

// --- Example 3: none stated ------------------------------------------------------------------

#[test]
fn example_3_none_needed_writes_nothing_and_is_remembered() {
    let part = plate();
    let (saved, _) = save(&part, &map(json!({"part.general_tolerance": "none"})));
    let p = pmi_of(&saved);
    assert!(p.general.is_empty(), "{:?}", p.general);
    assert_eq!(note(&p), None);
    let (state, value) = general_row(&saved);
    assert_eq!(state, "decided_none");
    assert_eq!(value, "none");
}

// --- Example 4: a metric block ---------------------------------------------------------------

#[test]
fn example_4_a_metric_block_is_a_millimetre_table() {
    let answers = map(json!({
        "part.general_tolerance": "X ±0.5, X.X ±0.2, X.XX ±0.05, angles ±0°30'",
    }));
    let (saved, report) = save(&plate(), &answers);
    let p = pmi_of(&saved);
    assert_eq!(
        cells(&p),
        vec![
            ("0".into(), "0.5".into(), LengthUnit::Millimetre),
            ("1".into(), "0.2".into(), LengthUnit::Millimetre),
            ("2".into(), "0.05".into(), LengthUnit::Millimetre),
        ],
        "{:?}",
        report.written
    );
    assert_eq!(note(&p).as_deref(), Some("angles ±0°30'"));
    assert_eq!(classes(&p), 0);
    assert_eq!(
        general_row(&saved),
        (
            "stated".into(),
            json!("X ±0.5 · X.X ±0.2 · X.XX ±0.05 · angles ±0°30′")
        )
    );
}

// --- A table another tool wrote ---------------------------------------------------------------

/// `bytes` with a hand-written default tolerance table appended (Part 21 text written here, not
/// by haecceity's writer: no independent writer of the entity exists in reach, OCCT's XCAF has
/// none). An ISO 2768-style range table with an asymmetric decimal-place row, in millimetres,
/// related to a 'default tolerances' property as AP242 and the PMI practice (§4.1) describe.
fn with_foreign_table(bytes: &[u8]) -> Vec<u8> {
    let doc = Document::parse(bytes.to_vec()).unwrap();
    let pds = doc
        .ids()
        .find(|&id| {
            matches!(doc.get(id), Some(haecceity::p21::RawEntity::Simple { name, .. })
                if name == "PRODUCT_DEFINITION_SHAPE")
        })
        .unwrap();
    let mm = doc
        .ids()
        .find(|&id| {
            let span = doc.span(id).unwrap();
            let text = String::from_utf8_lossy(&doc.bytes()[span]);
            text.contains("LENGTH_UNIT") && text.contains(".MILLI.") && text.contains(".METRE.")
        })
        .unwrap();
    let n = doc.max_id() + 1;
    let id = |k: u64| format!("#{}", n + k);
    let length = |k: u64, name: &str, value: &str| {
        format!(
            "{}=(LENGTH_MEASURE_WITH_UNIT()MEASURE_REPRESENTATION_ITEM()MEASURE_WITH_UNIT(\
             LENGTH_MEASURE({value}),#{mm})REPRESENTATION_ITEM('{name}'));\n",
            id(k)
        )
    };
    let mut text = String::new();
    text += &format!("{}=REPRESENTATION_CONTEXT('','default setting');\n", id(0));
    text += &format!("{}=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);\n", id(1));
    text += &format!("{}=CONTEXT_DEPENDENT_UNIT({},'count');\n", id(2), id(1));
    // Row 1: over 0.5 up to 6, ±0.1.
    text += &length(3, "lower limit", "0.5");
    text += &length(4, "upper limit", "6.");
    text += &length(5, "plus minus tolerance value", "0.1");
    text += &format!(
        "{}=DEFAULT_TOLERANCE_TABLE_CELL('',SET_REPRESENTATION_ITEM(({},{},{})));\n",
        id(6),
        id(3),
        id(4),
        id(5)
    );
    // Row 2: over 6 up to 30, ±0.2.
    text += &length(7, "lower limit", "6.");
    text += &length(8, "upper limit", "30.");
    text += &length(9, "plus minus tolerance value", "0.2");
    text += &format!(
        "{}=DEFAULT_TOLERANCE_TABLE_CELL('',SET_REPRESENTATION_ITEM(({},{},{})));\n",
        id(10),
        id(7),
        id(8),
        id(9)
    );
    // Row 3: one decimal place, +0.3/-0.1.
    text += &format!(
        "{}=MEASURE_REPRESENTATION_ITEM('significant number of digits',COUNT_MEASURE(1.),{});\n",
        id(11),
        id(2)
    );
    text += &length(12, "upper tolerance value", "0.3");
    text += &length(13, "lower tolerance value", "-0.1");
    text += &format!(
        "{}=DEFAULT_TOLERANCE_TABLE_CELL('',SET_REPRESENTATION_ITEM(({},{},{})));\n",
        id(14),
        id(11),
        id(12),
        id(13)
    );
    text += &format!(
        "{}=DEFAULT_TOLERANCE_TABLE('',({},{},{}),{});\n",
        id(15),
        id(6),
        id(10),
        id(14),
        id(0)
    );
    text += &format!(
        "{}=DESCRIPTIVE_REPRESENTATION_ITEM('tolerance class','');\n",
        id(16)
    );
    text += &format!(
        "{}=REPRESENTATION('default tolerances',({}),{});\n",
        id(17),
        id(16),
        id(0)
    );
    text += &format!(
        "{}=PROPERTY_DEFINITION('default tolerances','',#{pds});\n",
        id(18)
    );
    text += &format!(
        "{}=PROPERTY_DEFINITION_REPRESENTATION({},{});\n",
        id(19),
        id(18),
        id(17)
    );
    text += &format!(
        "{}=REPRESENTATION_RELATIONSHIP('general tolerance definition','',{},{});\n",
        id(20),
        id(15),
        id(17)
    );
    let source = String::from_utf8(bytes.to_vec()).unwrap();
    let at = source.rfind("ENDSEC;").unwrap();
    let mut out = source[..at].to_string();
    out += &text;
    out += &source[at..];
    let ap242 =
        "FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 7 1 4 }'));";
    let start = out.find("FILE_SCHEMA(").unwrap();
    let end = start + out[start..].find(';').unwrap() + 1;
    out.replace_range(start..end, ap242);
    out.into_bytes()
}

/// `bytes` with every requirement copied by haecceity's writer onto the bare plate, unmarked:
/// the same PMI, written by another tool.
fn as_another_tool(bytes: &[u8]) -> Vec<u8> {
    let defs = haecceity::step::read_part_definitions(bytes).unwrap();
    let read = pmi::read(&Document::parse(bytes.to_vec()).unwrap(), &defs).unwrap();
    let mut copied = read.parts[0].clone();
    for t in &mut copied.tolerances {
        t.description = None;
    }
    copied.attributes.retain(|set| {
        !specify_core::resume::is_answers(set)
            && set.name != specify_core::characteristics::PROPERTY
    });
    let doc = Document::parse(plate().bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(pmi::PartId(0), copied)],
        pmi::write::Mode::Replace,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    doc.apply(&edit).unwrap().bytes
}

/// The plate with another tool's datum A and its flatness, and another tool's default tolerance
/// table. (No material: haecceity's writer, standing in for the other tool here, would draw it
/// in a Notes view of its own, which haecceity's add mode updates with each new note.)
fn foreign_plate() -> LoadedPart {
    let answers = map(json!({"datum.A.flatness": "0.05"}));
    let part = plate();
    let analysis = api::analyse(&part).unwrap();
    let intent = api::preview(&analysis, &answers).unwrap();
    let (bytes, _) = specify_core::writer::write_bytes(&part, &intent, None).unwrap();
    let other = as_another_tool(&bytes);
    load_bytes("foreign.step", with_foreign_table(&other)).unwrap()
}

const FOREIGN: &str = "0.5 to 6 ±0.1 · 6 to 30 ±0.2 · X.X +0.3/-0.1";

#[test]
fn a_table_another_tool_wrote_reads_as_stated() {
    let part = foreign_plate();
    assert_eq!(general_row(&part), ("stated".into(), json!(FOREIGN)));
}

#[test]
fn none_needed_is_only_for_a_file_that_states_none() {
    let part = foreign_plate();
    let before = part.bytes.to_vec();
    let (saved, report) = save(&part, &map(json!({"part.general_tolerance": "none"})));
    assert_eq!(general_row(&saved), ("stated".into(), json!(FOREIGN)));
    assert!(
        report
            .not_written
            .iter()
            .any(|l| l.contains("part.general_tolerance")),
        "{:?}",
        report.not_written
    );
    let p = pmi_of(&saved);
    assert_eq!(p.general.len(), 1);
    // Nothing of the table moved.
    let (a, b) = (
        Document::parse(before).unwrap(),
        Document::parse(saved.bytes.to_vec()).unwrap(),
    );
    for id in a.ids() {
        assert_eq!(
            &a.bytes()[a.span(id).unwrap()],
            &b.bytes()[b.span(id).unwrap()],
            "#{id}"
        );
    }
}

#[test]
fn a_change_replaces_the_table_and_keeps_every_other_entity() {
    let part = foreign_plate();
    let mut intent = specify_core::rules::Intent::new(serde_json::to_value(&part.binding).unwrap());
    intent
        .part
        .insert("general_tolerance".into(), json!(".XX ±.01, angles ±1°"));
    // A raw intent never replaces what the file states (ADR P11): it is refused.
    let refused = specify_core::writer::write_bytes(&part, &intent, None).unwrap_err();
    assert!(
        refused
            .to_string()
            .contains("already states general tolerance"),
        "{refused}"
    );
    // The edit list's explicit change does.
    let (bytes, report) =
        specify_core::writer::write_bytes_changing_general_tolerance(&part, &intent, None).unwrap();
    let saved = load_bytes("changed.step", bytes).unwrap();
    assert_eq!(
        general_row(&saved),
        ("stated".into(), json!(".XX ±.01 · angles ±1°")),
        "{:?}",
        report.written
    );
    let p = pmi_of(&saved);
    assert_eq!(
        cells(&p),
        vec![("2".into(), "0.01".into(), LengthUnit::Inch)]
    );
    // Another tool's PMI is still read, and every entity but the old table's is byte-identical.
    let old = pmi_of(&part);
    assert_eq!(p.datums.len(), old.datums.len());
    assert_eq!(p.tolerances.len(), old.tolerances.len());
    assert_eq!(p.material, old.material);
    let (a, b) = (
        Document::parse(part.bytes.to_vec()).unwrap(),
        Document::parse(saved.bytes.to_vec()).unwrap(),
    );
    // The old table is the 21 entities `with_foreign_table` appended; the file gives way only
    // those (their ids may be reused by what is written now). Every other entity is unchanged.
    let table = a.max_id() - 20;
    for id in a.ids().filter(|&id| id < table) {
        assert_eq!(
            String::from_utf8_lossy(&a.bytes()[a.span(id).unwrap()]),
            String::from_utf8_lossy(&b.bytes()[b.span(id).unwrap()]),
            "#{id} changed"
        );
    }
}

// --- The other two points of #228 -------------------------------------------------------------

/// #228: a save writes what is given while gaps remain (the editor's save, ADR P11's model):
/// the general tolerance and the material, every other question still open.
#[test]
fn issue_228_a_save_does_not_refuse_while_gaps_remain() {
    let part = plate();
    let answers = map(json!({
        "part.material": "6061 Al",
        "part.general_tolerance": ".XXX ±.005, angles ±1°",
    }));
    let analysis = api::analyse(&part).unwrap();
    let (intent, open) = specify_core::session::intent_for(&part, &analysis, &answers).unwrap();
    assert!(!open.is_empty(), "gaps remain");
    let (bytes, _) = specify_core::writer::write_bytes(&part, &intent, Some(&answers)).unwrap();
    let saved = load_bytes("saved.step", bytes).unwrap();
    let p = pmi_of(&saved);
    assert!(p.material.is_some());
    assert_eq!(
        general_row(&saved),
        ("stated".into(), json!(".XXX ±.005 · angles ±1°"))
    );
}

/// The surface finish row: (state, value).
fn finish_row(loaded: &LoadedPart) -> (String, Value) {
    let analysis = api::analyse(loaded).unwrap();
    let row = analysis["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "part.surface_finish")
        .unwrap()
        .clone();
    (
        row["state"].as_str().unwrap().to_string(),
        row["value"].clone(),
    )
}

/// #228: a drawing that gives a coating and no roughness. Surface finish can be decided none
/// (nothing written, the decision remembered) or left a gap (nothing written, still asked).
#[test]
fn issue_228_surface_finish_can_be_none_needed_or_a_gap() {
    let part = plate();
    let analysis = api::analyse(&part).unwrap();
    let none = map(json!({"part.coating": "Anodize blue", "part.surface_finish": "none"}));
    api::questions(&analysis, &none).expect("none is an answer for the surface finish");
    let (saved, _) = save(&part, &none);
    assert!(pmi_of(&saved).surface_textures.is_empty());
    assert_eq!(finish_row(&saved).0, "decided_none");

    let gap = map(json!({"part.coating": "Anodize blue"}));
    let (saved, _) = save(&part, &gap);
    assert!(pmi_of(&saved).surface_textures.is_empty());
    assert_eq!(finish_row(&saved).0, "gap");
}

/// The block is taken as one line of text and shown back in its canonical spelling.
#[test]
fn a_block_typed_in_one_line_is_shown_back_for_checking() {
    let options = ["ISO 2768-m"];
    let shown = specify_core::choices::interpret(
        "part.general_tolerance",
        ".XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°",
        &options,
        None,
        None,
    )
    .unwrap();
    assert_eq!(shown, ".XX ±.01 · .XXX ±.005 · ±1/64 · angles ±1°");
    // The issue's own spelling.
    let shown = specify_core::choices::interpret(
        "part.general_tolerance",
        ".XXX ±.005 in, angular ±1°",
        &options,
        None,
        None,
    )
    .unwrap();
    assert_eq!(shown, ".XXX ±.005 · angles ±1°");
    // A line it cannot read is refused, named.
    let e = specify_core::choices::interpret(
        "part.general_tolerance",
        ".XX ±.01, three places ±.005",
        &options,
        None,
        None,
    )
    .unwrap_err();
    assert!(e.0.contains("three places ±.005"), "{e}");
    // Text that looks like a block but does not read says why, never the class list.
    for (typed, why) in [
        (".XX ±.01 .XXX ±.005", "two tolerances on one line"),
        ("XX ±.01, XXX ±.005", "written with a point"),
        (".XX ±0", "a tolerance of zero"),
        ("fractions ±1/0", "a fraction with a zero"),
        (
            ".XX ±.01, .XX ±.02",
            "the same number of decimal places is stated twice",
        ),
    ] {
        let e =
            specify_core::choices::interpret("part.general_tolerance", typed, &options, None, None)
                .unwrap_err();
        assert!(
            e.0.contains(typed.split(',').next_back().unwrap().trim()) && e.0.contains(why),
            "{typed}: {e}"
        );
        assert!(!e.0.contains("Choose one of"), "{typed}: {e}");
    }
    // An answers file may hold it in the spelling typed.
    specify_core::choices::check(
        "part.general_tolerance",
        &json!(".XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°"),
        &options,
        None,
        None,
    )
    .unwrap();
}

/// A file stating ISO 2768-m changed to a block by an explicit change: the class gives way to
/// the table. The same block again changes nothing.
#[test]
fn a_class_changed_to_a_block_is_replaced_and_the_same_block_is_not_written_twice() {
    let (classed, _) = save(
        &plate(),
        &map(json!({"part.general_tolerance": "ISO 2768-m"})),
    );
    let mut intent =
        specify_core::rules::Intent::new(serde_json::to_value(&classed.binding).unwrap());
    intent
        .part
        .insert("general_tolerance".into(), json!("X.X ±0.2, X.XX ±0.05"));
    let (bytes, _) =
        specify_core::writer::write_bytes_changing_general_tolerance(&classed, &intent, None)
            .unwrap();
    let changed = load_bytes("changed.step", bytes).unwrap();
    let p = pmi_of(&changed);
    assert_eq!(classes(&p), 0);
    assert_eq!(cells(&p).len(), 2);
    assert_eq!(
        general_row(&changed),
        ("stated".into(), json!("X.X ±0.2 · X.XX ±0.05"))
    );
    let mut intent =
        specify_core::rules::Intent::new(serde_json::to_value(&changed.binding).unwrap());
    intent
        .part
        .insert("general_tolerance".into(), json!("X.X ±0.2, X.XX ±0.05"));
    let (again, report) = specify_core::writer::write_bytes(&changed, &intent, None).unwrap();
    assert!(
        report.warnings.iter().any(|w| w.contains("already stated")),
        "{:?}",
        report.warnings
    );
    assert_eq!(
        cells(&pmi_of(&load_bytes("again.step", again).unwrap())).len(),
        2
    );
}

/// ADR P11: a raw intent (CLI, serve, Python `write`) never replaces a general tolerance the
/// file states, in any form: a different class, block or ± figure is refused, and the file is
/// left as it was. Only `write_bytes_changing_general_tolerance` (the edit list's change)
/// replaces it.
#[test]
fn a_raw_intent_never_replaces_a_stated_general_tolerance() {
    for stated in ["ISO 2768-f", ".XX ±.01, angles ±1°"] {
        let (part, _) = save(&plate(), &map(json!({"part.general_tolerance": stated})));
        for other in ["ISO 2768-c", "X.X ±0.2", "±0.3 mm"] {
            let mut intent =
                specify_core::rules::Intent::new(serde_json::to_value(&part.binding).unwrap());
            intent.part.insert("general_tolerance".into(), json!(other));
            let e = specify_core::writer::write_bytes(&part, &intent, None).unwrap_err();
            assert!(
                e.to_string().contains("already states general tolerance"),
                "{stated} then {other}: {e}"
            );
        }
    }
}

/// The explicit change of Specify's own block: only its table and its note give way (with
/// what they alone reference, and the entities whose ids the new ones reuse); the material
/// and every other entity are byte for byte as they were.
#[test]
fn a_change_replaces_only_the_table_and_its_note() {
    // Written without stored answers, so the save has no bookkeeping of its own to replace.
    let bare = plate();
    let answers =
        map(json!({"part.material": "steel", "part.general_tolerance": ".XX ±.01, angles ±1°"}));
    let first = api::preview(&api::analyse(&bare).unwrap(), &answers).unwrap();
    let (bytes, _) = specify_core::writer::write_bytes(&bare, &first, None).unwrap();
    let part = load_bytes("block.step", bytes).unwrap();
    let doc = Document::parse(part.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let (p, prov) = (&read.parts[0], &read.provenance.parts[0]);
    let mut seeds: Vec<u64> = prov.general.iter().flatten().copied().collect();
    for (i, set) in p.attributes.iter().enumerate() {
        if set.items.iter().any(|(n, _)| n == "general tolerance") {
            seeds.extend(&prov.attributes[i]);
        }
    }
    // Everything the old general tolerance reaches, what describes it, and its links to its
    // presentation.
    let mut reach: std::collections::BTreeSet<u64> = seeds.iter().copied().collect();
    let mut todo = seeds.clone();
    while let Some(id) = todo.pop() {
        if let Some(e) = doc.get(id) {
            haecceity::p21::visit_attributes(e, &mut |a| {
                if let haecceity::p21::Attribute::EntityRef(t) = a
                    && reach.insert(*t)
                {
                    todo.push(*t);
                }
            });
        }
        for &r in doc.referrers(id) {
            if matches!(doc.get(r), Some(haecceity::p21::RawEntity::Simple { name, .. })
                if name == "DESCRIPTION_ATTRIBUTE" || name == "DRAUGHTING_MODEL_ITEM_ASSOCIATION")
            {
                reach.insert(r);
            }
        }
    }
    // The Notes view draws the part's notes (the material and the old note): it is redrawn
    // without the old note, and is presentation, not a requirement. Its own entities may
    // change; every semantic entity outside the old table and note may not.
    let mut drawn: std::collections::BTreeSet<u64> = doc
        .ids()
        .filter(|&id| {
            matches!(doc.get(id), Some(haecceity::p21::RawEntity::Simple { name, attributes, .. })
                if (name == "DRAUGHTING_MODEL" || name == "DEFAULT_MODEL_GEOMETRIC_VIEW")
                    && matches!(attributes.first(), Some(haecceity::p21::Attribute::String(s)) if s == "Notes"))
        })
        .collect();
    let semantic: std::collections::BTreeSet<u64> = prov.all().into_iter().collect();
    let mut todo: Vec<u64> = drawn.iter().copied().collect();
    while let Some(id) = todo.pop() {
        if let Some(e) = doc.get(id) {
            haecceity::p21::visit_attributes(e, &mut |a| {
                if let haecceity::p21::Attribute::EntityRef(t) = a
                    && !semantic.contains(t)
                    && drawn.insert(*t)
                {
                    todo.push(*t);
                }
            });
        }
    }
    let mut intent = specify_core::rules::Intent::new(serde_json::to_value(&part.binding).unwrap());
    intent
        .part
        .insert("general_tolerance".into(), json!("X.X ±0.2"));
    let (bytes, _) =
        specify_core::writer::write_bytes_changing_general_tolerance(&part, &intent, None).unwrap();
    let after = Document::parse(bytes.clone()).unwrap();
    let saved = load_bytes("changed.step", bytes).unwrap();
    let q = pmi_of(&saved);
    assert_eq!(note(&q), None);
    assert_eq!(cells(&q).len(), 1);
    assert_eq!(q.material, p.material);
    let mut gone = 0;
    for id in doc.ids() {
        let before = &doc.bytes()[doc.span(id).unwrap()];
        let now = after.span(id).map(|s| &after.bytes()[s]);
        if now != Some(before) {
            if drawn.contains(&id) && !semantic.contains(&id) {
                continue;
            }
            assert!(
                reach.contains(&id),
                "#{id} changed, not the old table's or note's: {}",
                String::from_utf8_lossy(before)
            );
            gone += 1;
        }
    }
    assert!(gone > 0);
}
