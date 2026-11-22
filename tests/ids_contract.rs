//! The ids are a contract (docs/ids.md): the part-level ids, the requirement kinds and the
//! added requirement kinds the code has must be exactly the ones the doc lists.
//!
//! The code's ids come from the answer-id registry (`input::PART_NAMES`,
//! `input::FEATURE_KINDS`, `input::DATUM_LETTERS`, `input::DATUM_CONTROLS`, `bom::KEYS`,
//! `extras::KINDS`) and from the questions and
//! feature-table rows of real fixtures, so a new id the rules emit is noticed as well as one
//! added to the registry.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use specify_core::{api, bom, extras, input, rules};

const PART_LEVEL: &str = "Part-level ids";
const KINDS: &str = "Requirement kinds";
const ADDED: &str = "Added requirement kinds";
const GENERAL: &str = "General dimension ids";

/// The doc's lists, by section: the first cell of each table row between the markers.
fn documented() -> BTreeMap<String, BTreeSet<String>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/ids.md");
    let text = std::fs::read_to_string(&path).unwrap();
    let start = text
        .find("<!-- ids:begin -->")
        .expect("docs/ids.md: no ids:begin marker");
    let end = text
        .find("<!-- ids:end -->")
        .expect("docs/ids.md: no ids:end marker");
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut section = String::new();
    for line in text[start..end].lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            section = heading.trim().to_string();
        } else if let Some(cell) = line.strip_prefix("| `") {
            let id = cell.split('`').next().unwrap();
            out.entry(section.clone())
                .or_default()
                .insert(id.to_string());
        }
    }
    out
}

/// Every question and feature-table row id of a set of fixtures, with every default taken, and
/// what went wrong getting them (the rules refuse an id they ask, after a rename in one place).
fn emitted() -> (BTreeSet<String>, Vec<String>) {
    let cases: [(&str, Option<usize>, Value); 9] = [
        ("parts/plate.step", None, json!({})),
        (
            "parts/plate.step",
            None,
            json!({"part.locating": rules::LOCATING[1]}),
        ),
        ("parts/GRM-02_shaft.step", None, json!({})),
        ("spindle.step", None, json!({})),
        ("revisions/complex_pattern.old.step.gz", None, json!({})),
        ("parts/pin.step", None, json!({})),
        ("parts/assembly.step", Some(0), json!({})),
        ("parts/assembly.step", Some(1), json!({})),
        ("parts/electronics-enclosure-base.step.gz", None, json!({})),
    ];
    let mut out = BTreeSet::new();
    let mut errors = Vec::new();
    for (file, part, answers) in cases {
        let analysis = api::analyse_file(&common::fixtures().join(file), part).unwrap();
        let answers: Map<String, Value> = answers.as_object().unwrap().clone();
        let questions = rules::with_all_defaults(&analysis, &answers)
            .and_then(|all| rules::questions(&analysis, &all));
        match questions {
            Ok(questions) => out.extend(questions.into_iter().map(|q| q.id)),
            Err(e) => errors.push(format!("{file}: the rules refuse their own ids: {e}")),
        }
        for row in analysis["table"]["rows"].as_array().into_iter().flatten() {
            out.insert(row["id"].as_str().unwrap().to_string());
        }
    }
    for id in &out {
        if !input::known_id(id) {
            errors.push(format!("{id} is asked, but refused as an answer id"));
        }
    }
    (out, errors)
}

/// The code's lists, by section, in the doc's spelling, and what went wrong reading them.
fn in_code() -> (BTreeMap<String, BTreeSet<String>>, Vec<String>) {
    let (emitted, mut errors) = emitted();
    let mut part: BTreeSet<String> = input::PART_NAMES
        .iter()
        .map(|name| format!("part.{name}"))
        .collect();
    let mut kinds: BTreeSet<String> = input::FEATURE_KINDS.iter().map(|k| k.to_string()).collect();
    if input::DATUM_LETTERS != ["A", "B", "C", "D", "E", "F", "G", "H"] {
        errors.push(format!(
            "the datum letters are now {:?}; docs/ids.md says `A` to `H`",
            input::DATUM_LETTERS
        ));
    }
    part.insert("datum.<letter>".into());
    part.extend(input::DATUM_CONTROLS.iter().map(|c| format!("datum.{c}")));
    part.extend(bom::KEYS.iter().map(|key| format!("bom.{key}")));
    part.insert(format!("{}<name>", extras::PREFIX));
    let mut measurements = BTreeSet::new();
    for id in &emitted {
        if let Some((prefix, support)) = id.rsplit_once(':')
            && let Some((reference, located)) = support.split_once('-')
        {
            let (located, axis) = located
                .split_once('-')
                .map_or((located, None), |(side, axis)| (side, Some(axis)));
            let face_group = |side: &str| side.split('.').all(|n| n.parse::<usize>().is_ok());
            let plane = prefix.ends_with(":planes");
            if !(face_group(reference)
                && face_group(located)
                && axis.is_none_or(|axis| !plane && matches!(axis, "0" | "1" | "2")))
            {
                errors.push(format!("invalid measurement supports on {id}"));
            }
            let shape = if plane {
                "<side>-<side>"
            } else {
                "<reference>-<located>"
            };
            measurements.insert(format!("{prefix}:{shape}"));
        } else if let Some((kind, _)) = id.split_once(':') {
            kinds.insert(kind.to_string());
        } else if id.strip_prefix("datum.").is_some_and(|l| l.len() == 1) {
            part.insert("datum.<letter>".into());
        } else {
            part.insert(id.clone());
        }
    }
    let added = extras::KINDS.iter().map(|k| k.to_string()).collect();
    let lists = BTreeMap::from([
        (PART_LEVEL.to_string(), part),
        (KINDS.to_string(), kinds),
        (ADDED.to_string(), added),
        (GENERAL.to_string(), measurements),
    ]);
    (lists, errors)
}

#[test]
fn the_ids_in_the_code_are_the_ones_docs_ids_md_lists() {
    let documented = documented();
    let (code, mut problems) = in_code();
    for section in [PART_LEVEL, KINDS, ADDED, GENERAL] {
        let doc = documented.get(section).cloned().unwrap_or_default();
        let have = &code[section];
        let gone: Vec<&String> = doc.difference(have).collect();
        let new: Vec<&String> = have.difference(&doc).collect();
        if !gone.is_empty() {
            problems.push(format!(
                "{section}: {gone:?} are in docs/ids.md but no longer in the code. The ids are a \
                 contract other tools build on: if the rename or removal is intended, update \
                 docs/ids.md and add a CHANGELOG entry giving the reason."
            ));
        }
        if !new.is_empty() {
            problems.push(format!(
                "{section}: {new:?} are in the code but not in docs/ids.md. Document each new id \
                 there (and in the CHANGELOG) before it ships."
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "{}\nThe ids are a contract other tools build on: a rename or removal needs docs/ids.md \
         and a CHANGELOG entry giving the reason.",
        problems.join("\n")
    );
}

/// The doc's worked examples are real rows.
#[test]
fn the_docs_examples_are_real_rows() {
    for (file, id, faces) in [
        ("parts/plate.step", "hole.position:holes:12", vec![12]),
        (
            "parts/GRM-02_shaft.step",
            "hole.full_thread:holes:15.16",
            vec![15],
        ),
    ] {
        let analysis = api::analyse_file(&common::fixtures().join(file), None).unwrap();
        let row = analysis["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap_or_else(|| panic!("{file}: no row {id}; docs/ids.md uses it as an example"));
        assert_eq!(row["faces"], json!(faces), "{file}: {id}");
    }
}

/// The file's other PMI is named `file_pmi:#<entity>` (docs/ids.md): the doc gives the prefix
/// the table uses, and every id the table gives an item of NIST CTC-01's other PMI has it.
#[test]
fn file_pmi_ids_have_the_documented_form() {
    let doc = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/ids.md"),
    )
    .unwrap();
    let form = format!("`{}#<entity>`", specify_core::edits::FILE_PMI);
    assert!(doc.contains(&form), "docs/ids.md does not give {form}");
    let part = specify_core::part::load(
        &common::fixtures().join("nist/nist_ctc_01_asme1_ap242.stp.gz"),
        None,
    )
    .unwrap();
    let table = api::analyse(&part).unwrap()["table"].clone();
    let ids: Vec<&str> = table["file_pmi"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert!(!ids.is_empty());
    for id in ids {
        let entity = id
            .strip_prefix(specify_core::edits::FILE_PMI)
            .and_then(|e| e.strip_prefix('#'));
        assert!(
            entity.is_some_and(|e| e.parse::<u64>().is_ok()),
            "{id} is not file_pmi:#<entity>"
        );
    }
}
