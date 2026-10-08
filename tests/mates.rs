//! Mates and flat contacts between the parts of an assembly (`src/mates.rs`, port of
//! `mates.py`), checked against specify-core's own outputs.
//!
//! Ports `tests/test_mates.py`'s assertions on what `mates` and `contacts` find (the questions,
//! defaults and intents built from them are the rules' and are checked there), and sweeps every
//! captured part of a file of several parts against Python's `analysis["mates"]` and the faces'
//! `contact`. The sweep's differences are in `tests/fixtures/known_differences/mates.json`.

mod common;

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use common::{Difference, Tolerances, cases, check_known, compare};
use haecceity::geom::Surface;
use serde_json::{Map, Value, json};
use specify_core::mates::{contacts, contacts_with_refusals, mates};
use specify_core::part::{LoadedPart, load_all};

const BASE: usize = 0;
const COVER: usize = 1;
const PIN: usize = 2;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parts")
        .join(name)
}

fn stack() -> Vec<LoadedPart> {
    load_all(&fixture("stack.step")).expect("stack.step loads")
}

/// The diameter of cylindrical face `face` of `part`.
fn diameter(part: &LoadedPart, face: usize) -> Option<f64> {
    match part.part.faces[face].surface {
        Surface::Cylinder { radius, .. } => Some(2.0 * radius),
        _ => None,
    }
}

/// The suggestions of `found` by the diameter of their face: diameter (to 3 decimals) ->
/// (value, reason) of every face of that size, which must agree.
fn by_diameter(
    part: &LoadedPart,
    found: &Map<String, Value>,
) -> BTreeMap<String, (String, String)> {
    let mut out: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (face, s) in found {
        let d = diameter(part, face.parse().unwrap()).expect("a mate is on a cylinder");
        let entry = (
            s["value"].as_str().unwrap().to_owned(),
            s["reason"].as_str().unwrap().to_owned(),
        );
        let previous = out.insert(format!("{d:.3}"), entry.clone());
        assert!(
            previous.is_none_or(|p| p == entry),
            "faces of Ø{d} disagree"
        );
    }
    out
}

/// test_mates.py `test_a_screw_through_a_clearance_hole_into_a_tap_drill` (what reaches the
/// question's default and basis).
#[test]
fn a_screw_through_a_clearance_hole_into_a_tap_drill() {
    let parts = stack();
    let base = by_diameter(&parts[BASE], &mates(&parts, BASE));
    assert_eq!(
        base["4.200"],
        (
            "tapped:M5".to_owned(),
            "it lines up with the ⌀5.5 M5 clearance hole in cover: \
             a screw through that threads in here"
                .to_owned()
        )
    );
    let cover = by_diameter(&parts[COVER], &mates(&parts, COVER));
    assert_eq!(cover["5.500"].0, "clearance:M5");
    assert!(cover["5.500"].1.contains("M5 tap drill in base"));
}

/// test_mates.py `test_a_pin_in_its_bores_is_a_fit_on_both`.
#[test]
fn a_pin_in_its_bores_is_a_fit_on_both() {
    let parts = stack();
    for part in [BASE, COVER] {
        let found = by_diameter(&parts[part], &mates(&parts, part));
        assert_eq!(found["8.000"].0, "fit:H7");
        assert!(
            found["8.000"]
                .1
                .starts_with("the ⌀8 shaft of pin fits in it")
        );
    }
    let pin = by_diameter(&parts[PIN], &mates(&parts, PIN));
    assert_eq!(pin["8.000"].0, "h6");
    // The head touches nothing coaxially: no suggestion.
    assert!(!pin.contains_key("10.000"));
}

/// test_mates.py `test_mates_are_by_face_and_disagreement_suggests_nothing` (its `mates` half;
/// `suggestion` is the rules').
#[test]
fn mates_are_by_face() {
    let parts = stack();
    let found = mates(&parts, PIN);
    let values: Vec<&str> = found
        .values()
        .map(|s| s["value"].as_str().unwrap())
        .collect();
    assert_eq!(values, ["h6"]);
}

/// test_mates.py `test_a_part_alone_has_no_mates` and `test_a_part_alone_has_no_contacts`.
#[test]
fn a_part_alone_has_no_mates_or_contacts() {
    let parts = load_all(&fixture("plate.step")).unwrap();
    assert_eq!(parts.len(), 1);
    assert!(mates(&parts, 0).is_empty());
    assert!(contacts(&parts, 0).is_empty());
}

/// test_mates.py `test_datum_a_is_the_face_where_the_part_meets_another` (test_datums.py's
/// datum A in an assembly is this test): the contact the rules take datum A from. The rules
/// choose the plane with the most contact; here, that the cover's underside meets the base
/// over more area than its top meets the pin's head, and the base's top meets the cover.
#[test]
fn datum_a_is_the_face_where_the_part_meets_another() {
    let parts = stack();
    for (part, other) in [(COVER, "base"), (BASE, "cover")] {
        let found = contacts(&parts, part);
        let (face, contact) = found
            .iter()
            .max_by(|a, b| {
                a.1["area"]
                    .as_f64()
                    .unwrap()
                    .total_cmp(&b.1["area"].as_f64().unwrap())
            })
            .expect("a contact");
        assert_eq!(contact["part"], other, "part {part} face {face}");
    }
    let cover = contacts(&parts, COVER);
    let (face, _) = cover
        .iter()
        .max_by(|a, b| {
            a.1["area"]
                .as_f64()
                .unwrap()
                .total_cmp(&b.1["area"].as_f64().unwrap())
        })
        .unwrap();
    let face = &parts[COVER].part.faces[face.parse::<usize>().unwrap()];
    let Surface::Plane { frame } = face.surface else {
        panic!("a contact is flat")
    };
    let normal_z = if face.reversed {
        -frame.z[2]
    } else {
        frame.z[2]
    };
    assert!(normal_z < 0.0, "the cover's datum A faces down");
}

/// The stack's contacts, by hand: the base's top and the cover's underside are each 60 x 40
/// with four holes; their common face keeps the larger of each pair (the cover's Ø5.5
/// clearance holes over the Ø4.2 tap drills, and the two Ø8 bores): 2400 - 4·π·2.75² - π·4².
/// The pin's Ø10 head on the cover's top, round its Ø8 hole: π·(5² - 4²).
#[test]
fn the_stacks_contact_areas_are_the_exact_areas() {
    let parts = stack();
    let plate = 2400.0 - 4.0 * PI * 2.75f64.powi(2) - PI * 16.0;
    let annulus = PI * (25.0 - 16.0);
    let areas = |part: usize| -> Vec<(String, f64)> {
        let (found, refused) = contacts_with_refusals(&parts, part);
        assert!(refused.is_empty(), "{refused:?}");
        found
            .values()
            .map(|c| {
                (
                    c["part"].as_str().unwrap().to_owned(),
                    c["area"].as_f64().unwrap(),
                )
            })
            .collect()
    };
    // Within the rounding to 3 decimals.
    let close = |got: f64, exact: f64| (got - exact).abs() <= 5e-4;
    let base = areas(BASE);
    assert_eq!(base.len(), 1);
    assert!(
        base[0].0 == "cover" && close(base[0].1, plate),
        "{base:?} against {plate}"
    );
    let mut cover = areas(COVER);
    cover.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(cover.len(), 2);
    assert!(
        cover[0].0 == "base" && close(cover[0].1, plate),
        "{cover:?}"
    );
    assert!(
        cover[1].0 == "pin" && close(cover[1].1, annulus),
        "{cover:?} against {annulus}"
    );
    let pin = areas(PIN);
    assert!(
        pin.len() == 1 && pin[0].0 == "cover" && close(pin[0].1, annulus),
        "{pin:?}"
    );
    println!("stack: plate {plate:.6} annulus {annulus:.6}; rust {base:?} {cover:?} {pin:?}");
}

/// A part placed twice meets the other part at each instance; the plate of `assembly.step`
/// has its pins' ends on it (Ø6 discs, π·3²).
#[test]
fn each_instance_of_a_part_is_placed() {
    let parts = load_all(&fixture("assembly.step")).unwrap();
    assert_eq!(parts[1].placements.len(), 2);
    for (part, other) in [(0, "pin"), (1, "plate")] {
        let found = contacts(&parts, part);
        assert_eq!(found.len(), 1, "{found:?}");
        let contact = found.values().next().unwrap();
        assert_eq!(contact["part"], other);
        assert!((contact["area"].as_f64().unwrap() - 9.0 * PI).abs() <= 1.5e-3);
    }
}

/// Rust's mates and contacts of part `k` as one value, in the shape the sweep compares.
fn rust_value(parts: &[LoadedPart], k: usize) -> Value {
    json!({"mates": mates(parts, k), "contacts": contacts(parts, k)})
}

/// Python's from its analysis: `analysis["mates"]` and each face's `contact` by face id.
fn python_value(analysis: &Value) -> Value {
    let mut found = Map::new();
    for face in analysis["faces"].as_array().expect("faces") {
        if let Some(contact) = face.get("contact") {
            found.insert(face["id"].to_string(), contact.clone());
        }
    }
    json!({"mates": analysis.get("mates").cloned().unwrap_or(json!({})), "contacts": found})
}

/// The sweep over every captured part of a file of several parts.
#[test]
fn mates_and_contacts_match_the_oracle() {
    // Areas are rounded to 3 decimals by both engines; a difference in that last digit is
    // recorded (straight boundaries give the exact area in both, so any difference there is
    // a finding).
    let tolerances = Tolerances::default();
    let mut differences: Vec<Difference> = Vec::new();
    let mut loaded: BTreeMap<PathBuf, Vec<LoadedPart>> = BTreeMap::new();
    let mut checked = 0;
    for case in cases() {
        let Some(k) = case.part() else { continue };
        let Some(analysis) = case.artifact("analysis") else {
            continue;
        };
        if common::is_error(&analysis) {
            continue;
        }
        let Some(input) = case.input() else { continue };
        let parts = loaded
            .entry(input.clone())
            .or_insert_with(|| load_all(&input).expect("loads"));
        for part in parts.iter() {
            if let Some(why) = part.degraded() {
                println!("{}: degraded: {why}", case.name);
            }
        }
        let rust = if k < parts.len() {
            let (_, refused) = contacts_with_refusals(parts, k);
            for why in refused {
                println!("{}: contact refused: {why}", case.name);
            }
            // Determinism: the same parts give the same output twice.
            let value = rust_value(parts, k);
            assert_eq!(value, rust_value(parts, k), "{}", case.name);
            value
        } else {
            println!(
                "{}: Rust reads {} part(s); there is no part {k}",
                case.name,
                parts.len()
            );
            Value::Null
        };
        differences.extend(compare(
            &case.name,
            &python_value(&analysis),
            &rust,
            &tolerances,
        ));
        checked += 1;
    }
    println!(
        "mates sweep: {checked} parts compared, {} differences",
        differences.len()
    );
    for d in &differences {
        println!("  {d:?}");
    }
    check_known("mates", &differences);
}

/// Determinism across loads: loading the file again gives the same mates and contacts.
#[test]
fn mates_and_contacts_are_deterministic() {
    for file in ["stack.step", "assembly.step"] {
        let once = load_all(&fixture(file)).unwrap();
        let twice = load_all(&fixture(file)).unwrap();
        for k in 0..once.len() {
            let a = serde_json::to_string(&rust_value(&once, k)).unwrap();
            let b = serde_json::to_string(&rust_value(&twice, k)).unwrap();
            assert_eq!(a, b, "{file} part {k}");
        }
    }
}
