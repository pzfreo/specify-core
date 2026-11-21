//! The ids are a contract (docs/ids.md): the part-level ids, the requirement kinds and the
//! added requirement kinds the code has must be exactly the ones the doc lists.
//!
//! The code's ids come from the answer-id registry (`input::PART_NAMES`,
//! `input::FEATURE_KINDS`, `input::known_id`, `extras::KINDS`) and from the questions and
//! feature-table rows of real fixtures, so a new id the rules emit is noticed as well as one
//! added to the registry.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use specify_core::{api, extras, input, rules};

const PART_LEVEL: &str = "Part-level ids";
const KINDS: &str = "Requirement kinds";
const ADDED: &str = "Added requirement kinds";

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

/// Every question and feature-table row id of a set of fixtures, with every default taken.
fn emitted() -> BTreeSet<String> {
    let cases: [(&str, Option<usize>, Value); 8] = [
        ("parts/plate.step", None, json!({})),
        (
            "parts/plate.step",
            None,
            json!({"part.locating": rules::LOCATING[1]}),
        ),
        ("parts/GRM-02_shaft.step", None, json!({})),
        ("spindle.step", None, json!({})),
        ("parts/pin.step", None, json!({})),
        ("parts/assembly.step", Some(0), json!({})),
        ("parts/assembly.step", Some(1), json!({})),
        ("parts/electronics-enclosure-base.step.gz", None, json!({})),
    ];
    let mut out = BTreeSet::new();
    for (file, part, answers) in cases {
        let analysis = api::analyse_file(&common::fixtures().join(file), part).unwrap();
        let answers: Map<String, Value> = answers.as_object().unwrap().clone();
        let all = rules::with_all_defaults(&analysis, &answers).unwrap();
        for q in rules::questions(&analysis, &all).unwrap() {
            out.insert(q.id);
        }
        for row in analysis["table"]["rows"].as_array().into_iter().flatten() {
            out.insert(row["id"].as_str().unwrap().to_string());
        }
    }
    out
}

/// Whether `bom.<key>` is an answer the code takes, with some value of the right shape.
fn bom_accepted(key: &str) -> bool {
    ["made", "yes", "from stock", "text"].iter().any(|value| {
        let mut answers = Map::new();
        answers.insert(format!("bom.{key}"), json!(value));
        input::answers_shape(&answers).is_ok()
    })
}

/// The code's lists, by section, in the doc's spelling.
fn in_code(documented: &BTreeMap<String, BTreeSet<String>>) -> BTreeMap<String, BTreeSet<String>> {
    let emitted = emitted();
    for id in &emitted {
        assert!(
            input::known_id(id),
            "{id} is asked, but refused as an answer id"
        );
    }
    let mut part: BTreeSet<String> = input::PART_NAMES
        .iter()
        .map(|name| format!("part.{name}"))
        .collect();
    let mut kinds: BTreeSet<String> = input::FEATURE_KINDS.iter().map(|k| k.to_string()).collect();
    let letters: Vec<char> = ('A'..='Z').collect();
    let plain: Vec<char> = letters
        .iter()
        .copied()
        .filter(|l| input::known_id(&format!("datum.{l}")))
        .collect();
    let mut suffixes: BTreeSet<String> = ["flatness".to_string(), "control".to_string()].into();
    let mut bom_keys = BTreeSet::new();
    for id in emitted.iter().chain(documented.values().flatten()) {
        if let Some((_, suffix)) = id.strip_prefix("datum.").and_then(|r| r.split_once('.')) {
            suffixes.insert(suffix.to_string());
        }
        if let Some(key) = id.strip_prefix("bom.") {
            bom_keys.insert(key.to_string());
        }
    }
    if !plain.is_empty() {
        assert_eq!(
            plain,
            ('A'..='H').collect::<Vec<_>>(),
            "the datum letters an answer can have changed; docs/ids.md says `A` to `H`: \
             update it and the CHANGELOG with the reason"
        );
        part.insert("datum.<letter>".into());
    }
    for l in &letters {
        for suffix in &suffixes {
            let id = format!("datum.{l}.{suffix}");
            if input::known_id(&id) {
                part.insert(id);
            }
        }
    }
    part.extend(
        bom_keys
            .iter()
            .filter(|key| bom_accepted(key))
            .map(|key| format!("bom.{key}")),
    );
    if input::known_id(&format!("{}x", extras::PREFIX)) {
        part.insert(format!("{}<name>", extras::PREFIX));
    }
    for id in &emitted {
        if let Some((kind, _)) = id.split_once(':') {
            kinds.insert(kind.to_string());
        } else if id.strip_prefix("datum.").is_some_and(|l| l.len() == 1) {
            part.insert("datum.<letter>".into());
        } else {
            part.insert(id.clone());
        }
    }
    let added = extras::KINDS.iter().map(|k| k.to_string()).collect();
    BTreeMap::from([
        (PART_LEVEL.to_string(), part),
        (KINDS.to_string(), kinds),
        (ADDED.to_string(), added),
    ])
}

#[test]
fn the_ids_in_the_code_are_the_ones_docs_ids_md_lists() {
    let documented = documented();
    let code = in_code(&documented);
    let mut problems = Vec::new();
    for section in [PART_LEVEL, KINDS, ADDED] {
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
    assert!(problems.is_empty(), "{}", problems.join("\n"));
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
