//! The material, coating and heat-treatment suggestions: specify-core's
//! `tests/test_materials.py`, and the tables compared with Python's
//! (`tests/fixtures/choices_oracle.json.gz`; see `tests/choices.rs` for how it is generated).
//!
//! `test_the_material_question_suggests_common_materials` and
//! `test_a_long_material_name_is_written_and_verified` analyse and write a part (`api`, `rules`,
//! `writer`) and are ported with them; the suggestions they read are checked here.

use std::collections::HashSet;
use std::io::Read;

use serde_json::Value;
use specify_core::materials::{COATINGS, HEAT_TREATMENTS, MATERIALS, suggested, suggestions};

// test_materials.py::test_the_material_question_suggests_common_materials (the suggestions)
#[test]
fn the_material_question_suggests_common_materials() {
    let all = suggestions();
    let found = |name: &str| all.iter().find(|s| s["value"] == name).unwrap().clone();
    assert_eq!(found("Brass CZ121 (CW614N)")["group"], "Brass and copper");
    assert!(
        found("POM-C acetal copolymer")["also"]
            .as_array()
            .unwrap()
            .contains(&Value::from("Delrin"))
    );
}

// test_materials.py::test_each_material_is_named_once
#[test]
fn each_material_is_named_once() {
    let names: HashSet<&str> = MATERIALS.iter().map(|&(name, _, _)| name).collect();
    assert_eq!(names.len(), MATERIALS.len());
    assert!(
        MATERIALS
            .iter()
            .all(|&(name, family, _)| !name.is_empty() && !family.is_empty())
    );
}

// test_materials.py::test_printed_materials_name_their_process_and_metals_carry_uns_numbers
#[test]
fn printed_materials_name_their_process_and_metals_carry_uns_numbers() {
    let printed: Vec<&str> = MATERIALS
        .iter()
        .filter(|(_, family, _)| family.starts_with("3D printing"))
        .map(|&(name, _, _)| name)
        .collect();
    for name in [
        "PA12 (MJF)",
        "PLA (FDM)",
        "Standard resin (SLA)",
        "AlSi10Mg (DMLS)",
    ] {
        assert!(printed.contains(&name), "{name}");
    }
    assert!(printed.iter().all(|name| name.ends_with(')'))); // the process, in brackets
    let also = |name: &str| MATERIALS.iter().find(|m| m.0 == name).unwrap().2;
    assert!(also("Stainless steel 304 (1.4301)").contains(&"S30400"));
    assert!(also("Brass CZ121 (CW614N)").contains(&"C38500"));
}

/// Python's `suggestions()` and `suggested()` of each table, exactly, keys and order included.
#[test]
fn suggestions_equal_pythons() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/choices_oracle.json.gz"
    );
    let file = std::fs::File::open(path).expect("tests/fixtures/choices_oracle.json.gz");
    let mut text = String::new();
    flate2::read::GzDecoder::new(file)
        .read_to_string(&mut text)
        .expect("gzip");
    let data: Value = serde_json::from_str(&text).expect("valid JSON");
    // serde_json keeps key order (preserve_order), so the text compares order too.
    let same = |rust: Vec<Value>, python: &Value| {
        assert_eq!(Value::from(rust).to_string(), python.to_string());
    };
    same(suggestions(), &data["materials"]);
    same(suggested(COATINGS), &data["coatings"]);
    same(suggested(HEAT_TREATMENTS), &data["heat_treatments"]);
}
