//! Answers stored in a written file and resumed from it: specify-core's `tests/test_resume.py`,
//! the cross-engine forms (Python's stored answers read here, this port's read by Python's
//! pattern), the fingerprint's JSON text, and a Python-written file rewritten by replace.

mod common;

use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use specify_core::part::load;
use specify_core::pyjson;
use specify_core::resume::{
    self, answers_for, fingerprint, fingerprint_of, fingerprint_v1, stored,
};
use specify_core::rules::{Intent, apply_with_defaults};
use specify_core::writer::write;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-resume-tests")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture(name: &str) -> PathBuf {
    common::fixtures()
        .join("parts")
        .join(format!("{name}.step"))
}

fn analysis(case: &str) -> Value {
    common::case(case)
        .artifact("analysis")
        .expect("a captured analysis")
}

fn apply(analysis: &Value, given: Value) -> Intent {
    apply_with_defaults(analysis, given.as_object().unwrap(), true).expect("an intent")
}

#[test]
fn long_saved_answers_resume_from_ordered_text_chunks() {
    let loaded = load(&fixture("plate"), None).unwrap();
    let analysis = specify_core::api::analyse(&loaded).unwrap();
    let given = json!({"part.material":"steel", "part.notes":["Keep dry. ".repeat(1000)]});
    let intent = apply(&analysis, given.clone());
    let (bytes, _) =
        specify_core::writer::write_bytes(&loaded, &intent, Some(given.as_object().unwrap()))
            .unwrap();
    let back = specify_core::part::load_bytes("chunked.step", bytes.clone()).unwrap();
    // ADR P11: the long note is the file's PMI, not a stored answer.
    assert_eq!(
        answers_for(&back).unwrap(),
        Some(specify_core::feature_table::storable(
            given.as_object().unwrap()
        ))
    );
    assert!(String::from_utf8_lossy(&bytes).contains("haecceity text chunk/1"));
    let resumed = specify_core::api::analyse(&back).unwrap();
    let intent = apply(&resumed, given.clone());
    let (again, _) =
        specify_core::writer::write_bytes(&back, &intent, Some(given.as_object().unwrap()))
            .unwrap();
    assert_eq!(
        again, bytes,
        "unchanged chunked answers must resave without drift"
    );
}

// --- test_resume.py --------------------------------------------------------------------------

fn feature(analysis: &Value, family: &str, diameter: f64) -> String {
    analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["family"] == family && f["record"]["diameter"].as_f64() == Some(diameter))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn flat(path: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
    Regex::new(r"\s*\n\s*")
        .unwrap()
        .replace_all(&text, " ")
        .into_owned()
}

/// Counts of what a written file holds, as Python's `_counts` (`saved` counts the stored
/// answers' items).
fn counts(path: &Path) -> (usize, usize, usize) {
    let text = flat(path);
    let n = |pattern: &str| Regex::new(pattern).unwrap().find_iter(&text).count();
    (
        n(r"= ?DATUM\("),
        n(r"POSITION_TOLERANCE"),
        n(r"DESCRIPTIVE_REPRESENTATION_ITEM\('pmi-assist answers'"),
    )
}

// test_resume.py::test_a_written_file_resumes_its_answers_and_is_rewritten_not_added_to, to
// ADR P11: the stored answers are those that leave no PMI behind, and a save with the same
// answers adds nothing: equal counts, one answers payload, the same bytes.
#[test]
fn a_written_file_resumes_its_answers_and_is_rewritten_not_added_to() {
    let dir = temp("resume");
    let pin = load(&fixture("pin"), None).unwrap();
    let base = specify_core::api::analyse(&pin).unwrap();
    let knurl = feature(&base, "turned_steps", 10.0);
    let given = json!({
        "part.material": "brass",
        format!("diameter.fit:{knurl}"): "knurl:straight",
        "part.coating": "None (as machined)",
    });
    let first = dir.join("first.step");
    write(
        &pin,
        &apply(&base, given.clone()),
        &first,
        given.as_object(),
    )
    .unwrap();
    let loaded = load(&first, None).unwrap();
    let storable = specify_core::feature_table::storable(given.as_object().unwrap());
    assert_eq!(answers_for(&loaded).unwrap(), Some(storable.clone()));
    let again = specify_core::api::analyse(&loaded).unwrap();
    assert!(again["resume"].get("conflicts").is_none());
    assert_eq!(again["resume"]["answers"], json!(storable));

    let second = dir.join("second.step");
    write(
        &loaded,
        &apply(&again, given.clone()),
        &second,
        given.as_object(),
    )
    .unwrap();
    assert_eq!(counts(&first), counts(&second));
    assert_eq!(counts(&second).2, 1);
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap()
    );
}

// A file Python wrote (schema 1, Python's own PROPERTY_DEFINITION form) is written again:
// its payload is replaced by one schema-3 payload (Quiddity fingerprints), not kept beside it.
#[test]
fn a_python_written_files_schema_one_payload_is_replaced_not_duplicated() {
    let dir = temp("python-written");
    let written = common::case("parts/plate").written_step().unwrap();
    let text = std::fs::read_to_string(&written).unwrap();
    let ours = fingerprint_v1(&load(&fixture("plate"), None).unwrap()).unwrap();
    let mut payload = Value::Object(stored(text.as_bytes()).remove(&0).unwrap());
    assert_eq!(payload["schema"], json!(1));
    payload["faces"] = json!(ours);
    let pattern = Regex::new(r"('pmi-assist answers',')[A-Za-z0-9+/=]*'").unwrap();
    let patched = dir.join("python.step");
    std::fs::write(
        &patched,
        pattern
            .replace(
                &text,
                format!("${{1}}{}'", resume::encode(&payload)).as_str(),
            )
            .as_ref(),
    )
    .unwrap();
    let loaded = load(&patched, None).unwrap();
    let given = json!({"part.coating": "None (as machined)"});
    let base = specify_core::api::analyse(&loaded).unwrap();
    let out = dir.join("out.step");
    write(
        &loaded,
        &apply(&base, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    let after = flat(&out);
    assert!(!after.contains("PROPERTY_DEFINITION('pmi-assist answers','schema 1'"));
    let item_count = after
        .matches("DESCRIPTIVE_REPRESENTATION_ITEM('pmi-assist answers'")
        .count()
        + after
            .matches("COMPOUND_REPRESENTATION_ITEM('pmi-assist answers'")
            .count();
    assert_eq!(item_count, 1);
    let back = load(&out, None).unwrap();
    let rewritten = &stored(&back.bytes)[&0];
    assert_eq!(rewritten["schema"], json!(3));
    assert!(rewritten["geometry"].is_object());
    assert!(rewritten.get("faces").is_none());
    assert_eq!(
        answers_for(&back).unwrap(),
        Some(specify_core::feature_table::storable(
            given.as_object().unwrap()
        ))
    );
}

// test_resume.py::test_without_answers_nothing_is_stored
#[test]
fn without_answers_nothing_is_stored() {
    let dir = temp("noanswers");
    let analysis = analysis("parts/pin");
    let out = dir.join("plain.step");
    let pin = load(&fixture("pin"), None).unwrap();
    write(
        &pin,
        &apply(&analysis, json!({"part.material": "brass"})),
        &out,
        None,
    )
    .unwrap();
    assert!(stored(&std::fs::read(&out).unwrap()).is_empty());
    assert!(answers_for(&load(&out, None).unwrap()).unwrap().is_none());
}

// test_resume.py::test_answers_for_other_faces_are_not_resumed
#[test]
fn answers_for_other_faces_are_not_resumed() {
    let dir = temp("forged");
    let analysis = analysis("parts/pin");
    let given = json!({"part.material": "brass"});
    let out = dir.join("x.step");
    let pin = load(&fixture("pin"), None).unwrap();
    write(
        &pin,
        &apply(&analysis, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    let raw = std::fs::read(&out).unwrap();
    // Both a forged legacy schema 2 hash and a mutated schema 3 geometry fail to resume
    // and are reported (never silently dropped).
    for mutate in [
        Box::new(|p: &mut Value| {
            p["schema"] = json!(2);
            p.as_object_mut().unwrap().remove("geometry");
            p["faces"] = json!("0".repeat(64));
        }) as Box<dyn Fn(&mut Value)>,
        Box::new(|p: &mut Value| {
            p["geometry"]["faces"][0]["centroid"][0] = json!(9999.0);
        }),
    ] {
        let mut payload = Value::Object(stored(&raw).remove(&0).unwrap());
        mutate(&mut payload);
        let changed = with_payload_bytes(&raw, &payload);
        std::fs::write(&out, &changed).unwrap();
        assert!(stored(&changed).contains_key(&0));
        let back = load(&out, None).unwrap();
        assert!(answers_for(&back).unwrap().is_none());
        let (a, warnings) = specify_core::api::analyse_reporting(&back).unwrap();
        assert!(a.get("resume").is_none());
        assert_eq!(
            a["resume_warning"].as_str(),
            Some(resume::NOT_REUSED_MESSAGE)
        );
        assert!(warnings.iter().any(|w| w == resume::NOT_REUSED_MESSAGE));
    }
}

// A small part's stored item fits in a single DESCRIPTIVE_REPRESENTATION_ITEM matching
// Python's pattern; larger payloads use haecceity's ordered text chunks, read by `stored`.
#[test]
fn the_stored_item_is_the_one_pythons_pattern_reads() {
    let dir = temp("pattern");
    let analysis = analysis("parts/pin");
    let given = json!({"part.material": "steel"});
    let out = dir.join("x.step");
    let pin = load(&fixture("pin"), None).unwrap();
    write(
        &pin,
        &apply(&analysis, given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    // resume.py's _SAVED, over the text with line breaks removed.
    let text = Regex::new(r"\s*\n\s*")
        .unwrap()
        .replace_all(&std::fs::read_to_string(&out).unwrap(), "")
        .into_owned();
    let saved = Regex::new(
        r"DESCRIPTIVE_REPRESENTATION_ITEM\(\s*'pmi-assist answers'\s*,\s*'([A-Za-z0-9+/=]*)'\s*\)",
    )
    .unwrap();
    let found: Vec<_> = saved.captures_iter(&text).collect();
    assert_eq!(found.len(), 1);
    let payload = stored(text.as_bytes()).remove(&0).unwrap();
    assert_eq!(found[0][1], resume::encode(&Value::Object(payload)));
}

// --- the fingerprint, across engines ---------------------------------------------------------------

/// The plate's faces as Python's `describe_faces` gives them (kind, centroid rounded to 3
/// decimals: `round(c, 3)` of OpenCascade's centroids, signs of zero included), and the
/// fingerprint Python stored for them in the file it wrote (`parts/plate/written.step.gz`).
const PYTHON_PLATE: &str = r#"[["plane", [-50.0, -0.0, -0.0]], ["plane", [0.0, -30.0, -0.0]], ["plane", [-0.109, -0.0, 5.0]], ["plane", [0.0, 30.0, -0.0]], ["plane", [-0.109, -0.0, -5.0]], ["plane", [50.0, -0.0, -0.0]], ["cylinder", [-30.0, -15.0, -0.0]], ["cylinder", [0.0, -15.0, -0.0]], ["cylinder", [30.0, -15.0, -0.0]], ["cylinder", [0.0, 0.0, -0.0]], ["cylinder", [-30.0, 15.0, -0.0]], ["cylinder", [0.0, 15.0, -0.0]], ["cylinder", [45.0, -0.0, -0.0]], ["cylinder", [30.0, 15.0, -0.0]]]"#;

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// The fingerprint hashes Python's exact `json.dumps` text: given Python's faces, pyjson writes
// the text Python hashed.
#[test]
fn the_fingerprint_text_is_pythons() {
    let faces: Value = serde_json::from_str(PYTHON_PLATE).unwrap();
    assert_eq!(pyjson::dumps(&faces), PYTHON_PLATE);
    let written = common::case("parts/plate").written_step().unwrap();
    let payload = stored(&std::fs::read(&written).unwrap())
        .remove(&0)
        .unwrap();
    assert_eq!(payload["faces"], json!(sha(PYTHON_PLATE)));
}

// Python's stored answers are read in Python's form, in every file Python wrote.
#[test]
fn pythons_stored_answers_are_read() {
    for case in common::cases() {
        if case.part().is_some() {
            continue;
        }
        let (Some(written), Some(Value::Array(back))) =
            (case.written_step(), case.artifact("readback"))
        else {
            continue;
        };
        let found = stored(&std::fs::read(&written).unwrap());
        for (k, python) in back.iter().enumerate() {
            if python["resume"].is_object() {
                assert_eq!(
                    found.get(&k).map(|p| &p["answers"]),
                    Some(&python["resume"]),
                    "{} part {k}",
                    case.name
                );
            }
        }
    }
}

// Why a schema 1 file Python wrote does not resume here (known difference, Q14): its payload is
// read and its face count matches, but its fingerprint is of OpenCascade's centroids, whose
// zero components carry OpenCascade's signs; the Rust engine's centroids round to the same
// numbers with other signs of zero.
#[test]
fn a_python_fingerprint_differs_only_in_signs_of_zero() {
    let loaded = load(&fixture("plate"), None).unwrap();
    let ours = fingerprint_v1(&loaded).unwrap();
    assert_ne!(ours, sha(PYTHON_PLATE));
    let python: Value = serde_json::from_str(PYTHON_PLATE).unwrap();
    let rust: Vec<Value> = specify_core::features::describe_faces(&loaded)
        .unwrap()
        .iter()
        .map(|f| {
            json!([
                f.kind,
                f.centroid
                    .unwrap()
                    .iter()
                    .map(|c| specify_core::pyfmt::round_half_even(*c, 3))
                    .collect::<Vec<_>>()
            ])
        })
        .collect();
    let python = python.as_array().unwrap();
    assert_eq!(python.len(), rust.len());
    for (p, r) in python.iter().zip(&rust) {
        assert_eq!(p[0], r[0]);
        for (a, b) in p[1]
            .as_array()
            .unwrap()
            .iter()
            .zip(r[1].as_array().unwrap())
        {
            assert_eq!(a.as_f64(), b.as_f64(), "{p} {r}"); // -0.0 == 0.0
        }
    }
    let written = common::case("parts/plate").written_step().unwrap();
    let back = load(&written, None).unwrap();
    let payload = stored(&back.bytes).remove(&0).unwrap();
    assert_eq!(payload["face_count"], json!(back.binding.face_count));
    assert!(answers_for(&back).unwrap().is_none());
}

// --- schema 2 (Q14) --------------------------------------------------------------------------

/// A file the writer wrote for the plate with `given` stored, and its text.
fn written_plate(dir: &Path, given: &Value) -> (PathBuf, String) {
    let out = dir.join("plate.step");
    let plate = load(&fixture("plate"), None).unwrap();
    write(
        &plate,
        &apply(&analysis("parts/plate"), given.clone()),
        &out,
        given.as_object(),
    )
    .unwrap();
    let text = std::fs::read_to_string(&out).unwrap();
    (out, text)
}

/// Replace the stored `pmi-assist answers` item in STEP bytes (single item or chunked compound)
/// with `payload`.
fn with_payload_bytes(bytes: &[u8], payload: &Value) -> Vec<u8> {
    let doc = haecceity::p21::Document::parse(bytes.to_vec()).unwrap();
    let mut edit = haecceity::p21::Edit::new();
    let mut replaced = 0;
    for id in doc.ids() {
        let Some(haecceity::p21::RawEntity::Simple {
            name, attributes, ..
        }) = doc.get(id)
        else {
            continue;
        };
        if (name == "DESCRIPTIVE_REPRESENTATION_ITEM" || name == "COMPOUND_REPRESENTATION_ITEM")
            && matches!(attributes.first(), Some(haecceity::p21::Attribute::String(s)) if s == resume::NAME)
        {
            edit.replace(
                id,
                haecceity::p21::simple(
                    "DESCRIPTIVE_REPRESENTATION_ITEM",
                    vec![
                        haecceity::p21::Attribute::String(resume::NAME.into()),
                        haecceity::p21::Attribute::String(resume::encode(payload)),
                    ],
                ),
            );
            replaced += 1;
        }
    }
    assert_eq!(replaced, 1, "expected one stored-answers item");
    doc.apply(&edit).unwrap().bytes
}

// The writer stores schema 3, carrying Quiddity's own versioned fingerprints (faces + features)
// instead of Specify's own SHA-256 hash.
#[test]
fn the_writer_stores_schema_three_with_quiddity_fingerprints() {
    let dir = temp("schema3");
    let given = json!({"part.material": "steel", "part.coating": "None (as machined)"});
    let (out, _) = written_plate(&dir, &given);
    let loaded = load(&out, None).unwrap();
    let payload = stored(&loaded.bytes).remove(&0).unwrap();
    assert_eq!(payload["schema"], json!(3));
    assert!(payload.get("faces").is_none());
    assert!(payload.get("face_names").is_none());
    let geom = &payload["geometry"];
    assert_eq!(
        geom["fingerprint_version"],
        json!(quiddity::correspondence::FINGERPRINT_VERSION)
    );
    assert_eq!(
        geom["faces"].as_array().unwrap().len(),
        loaded.binding.face_count
    );
    assert!(!geom["features"].as_array().unwrap().is_empty());
    // ADR P11: the material writes PMI, so only storable answers are stored.
    assert_eq!(
        answers_for(&loaded).unwrap(),
        Some(specify_core::feature_table::storable(
            given.as_object().unwrap()
        ))
    );
}

// Each legacy schema is checked against its own fingerprint: a schema 1 payload is verified on
// the schema 1 fingerprint and not on schema 2's, and the reverse. A schema 1 payload in this
// port's attribute form whose hash is not reproduced (another kernel's signs of zero) still
// resumes, without face evidence (#35): `unverified` says so, and its answers are matched by
// ID (here a datum on a face the part has, applied either way).
#[test]
fn each_schema_is_checked_against_its_own_fingerprint() {
    let dir = temp("schemas");
    let given = json!({"part.material": "steel"});
    let (out, _) = written_plate(&dir, &given);
    let raw = std::fs::read(&out).unwrap();
    let loaded = load(&out, None).unwrap();
    let (v1, v2) = (
        fingerprint_v1(&loaded).unwrap(),
        fingerprint(&loaded).unwrap(),
    );
    let mut payload = Value::Object(stored(&raw).remove(&0).unwrap());
    payload.as_object_mut().unwrap().remove("geometry");
    // The datum the file's PMI states (its default, written): ADR P9 resumes an answer only
    // where the PMI agrees, and a datum on another face would be a conflict.
    payload["answers"]["datum.A"] = json!([2]);
    for (schema, faces, resumes, verified) in [
        (1, &v1, true, true),
        (1, &v2, true, false),
        (2, &v1, false, false),
        (2, &v2, true, true),
    ] {
        payload["schema"] = json!(schema);
        payload["faces"] = json!(faces);
        let path = dir.join(format!("s{schema}-{resumes}.step"));
        std::fs::write(&path, with_payload_bytes(&raw, &payload)).unwrap();
        let back = load(&path, None).unwrap();
        let (analysis, warnings) = specify_core::api::analyse_reporting(&back).unwrap();
        let resume = &analysis["resume"];
        assert_eq!(
            resume.is_object() && resume["unverified"].is_null(),
            verified,
            "schema {schema}: {resume}"
        );
        assert_eq!(resume.is_object(), resumes, "schema {schema}: {resume}");
        if resumes {
            // Verified or matched by ID, the stored datum is read; ADR P11: the file states
            // it (and the material), so the stored copies are not resumed.
            assert!(resume["unresolved"].is_null(), "schema {schema}: {resume}");
            assert!(resume["answers"].get("datum.A").is_none(), "{resume}");
            assert!(resume["answers"].get("part.material").is_none(), "{resume}");
            let stated = |id: &str| {
                analysis["table"]["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["id"] == id && r["state"] == "stated")
            };
            assert!(stated("datum.A") && stated("part.material"));
        } else {
            // Mismatched legacy schema 2 is never silently dropped: reported in warning + review.
            assert_eq!(
                analysis["resume_warning"].as_str(),
                Some(resume::NOT_REUSED_MESSAGE)
            );
            assert!(warnings.iter().any(|w| w == resume::NOT_REUSED_MESSAGE));
        }
        assert_eq!(
            answers_for(&back).unwrap().is_some(),
            resumes,
            "schema {schema}, {}",
            if faces == &v1 {
                "schema 1 fingerprint"
            } else {
                "schema 2 fingerprint"
            }
        );
    }
    // A future unsupported schema is passed over.
    payload["schema"] = json!(4);
    payload["faces"] = json!(v2);
    let path = dir.join("s4.step");
    std::fs::write(&path, with_payload_bytes(&raw, &payload)).unwrap();
    assert!(stored(&std::fs::read(&path).unwrap()).is_empty());
}

// The schema 2 fingerprint of the faces Python's engine gives the plate (OpenCascade's signs of
// zero) is the Rust engine's: the documented canonical form (docs/design.md) makes the two
// engines' fingerprints of the same faces equal, where schema 1's differ.
#[test]
fn schema_two_fingerprints_pythons_faces_as_the_rust_engines() {
    let loaded = load(&fixture("plate"), None).unwrap();
    let python: Value = serde_json::from_str(PYTHON_PLATE).unwrap();
    assert_eq!(fingerprint_of(&python, 2), fingerprint(&loaded).unwrap());
    assert_eq!(fingerprint_of(&python, 1), sha(PYTHON_PLATE));
    assert_ne!(fingerprint_of(&python, 1), fingerprint_v1(&loaded).unwrap());
    // The text schema 2 hashes, exactly.
    assert_eq!(
        fingerprint(&loaded).unwrap(),
        sha(&PYTHON_PLATE.replace("-0.0", "0.0"))
    );
}

/// These files were committed on the old dependency before upgrading. In particular CTC-02's
/// ambiguous rows change under quiddity #82 and cgb217's frame/fillets change under #81.
#[test]
fn alpha7_saved_answers_survive_the_schema6_upgrade() {
    for (name, count) in [
        ("golden_oriented_slots", 13),
        ("cgb217", 37),
        ("ctc02_ap242", 34),
    ] {
        let dir = common::fixtures().join("resume_alpha7");
        let meta: Value =
            serde_json::from_slice(&std::fs::read(dir.join(format!("{name}.json"))).unwrap())
                .unwrap();
        if name == "golden_oriented_slots" {
            // These are the actual 0.1.1 answers: all 13 are part/datum settings,
            // not feature-answer IDs. Do not invent an oriented_slots answer migration.
            assert!(
                meta["answers"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .all(|id| !id.contains(':'))
            );
            assert_eq!(
                meta["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|f| f["family"] == "oriented_slots")
                    .count(),
                3
            );
        }
        let path = dir.join(format!("{name}.step.gz"));
        let loaded = load(&path, None).unwrap();
        let saved = stored(&loaded.bytes).remove(&0).unwrap();
        assert_eq!(saved["answers"], meta["answers"]);
        let (analysis, warnings) = specify_core::api::analyse_reporting(&loaded).unwrap();
        if name == "golden_oriented_slots" {
            for old in meta["features"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["family"] == "oriented_slots")
            {
                // #91 restores the plate frame and #100 claims both end walls: the same
                // four physical walls as alpha.7, with the #92 family rename.
                // schema6 checks their actual end planes and rigid motions.
                let matches: Vec<_> = analysis["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|f| {
                        f["family"] == "slots"
                            && f["faces"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .all(|face| old["faces"].as_array().unwrap().contains(face))
                    })
                    .collect();
                assert_eq!(matches.len(), 1, "old support {}", old["id"]);
                assert_eq!(matches[0]["faces"], old["faces"]);
            }
        }
        let resumed = &analysis["resume"];
        assert!(resumed.is_object(), "{name}: saved answers disappeared");
        let applied = resumed["answers"].as_object().unwrap();
        // ADR P11: stored answers that write PMI are the file's to state, not resumed.
        let mut repeats = 0;
        let unresolved = resumed["unresolved"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let migrated = resumed["migrated"].as_array().cloned().unwrap_or_default();
        for (old, value) in meta["answers"].as_object().unwrap() {
            let new = migrated
                .iter()
                .find(|pair| pair[0] == *old)
                .and_then(|pair| pair[1].as_str())
                .unwrap_or(old);
            if let Some(answer) = applied.get(new) {
                assert_eq!(answer, value, "{name}: {old}");
                if let Some((_, old_feature)) = old.split_once(':') {
                    let old_support = meta["features"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|f| f["id"] == old_feature)
                        .unwrap();
                    let new_feature = new.split_once(':').unwrap().1;
                    let new_support = analysis["features"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|f| f["id"] == new_feature)
                        .unwrap();
                    assert_eq!(old_support["family"], new_support["family"]);
                    // This is the geometric-support rule, never matching by dimension/order.
                    for face in old_support["faces"].as_array().unwrap() {
                        assert!(
                            new_support["faces"].as_array().unwrap().contains(face),
                            "{name}: {old} attached to wrong support {new}"
                        );
                    }
                }
            } else if !unresolved.iter().any(|u| u["id"] == *old)
                && specify_core::feature_table::storable(&Map::from_iter([(
                    old.clone(),
                    value.clone(),
                )]))
                .is_empty()
            {
                repeats += 1;
            } else {
                let report = unresolved
                    .iter()
                    .find(|u| u["id"] == *old)
                    .unwrap_or_else(|| panic!("{name}: silently dropped {old}"));
                assert_eq!(report["answer"], *value);
                assert!(!report["reason"].as_str().unwrap().is_empty());
                assert!(
                    warnings
                        .iter()
                        .any(|w| w.contains(old) && w.contains("left open"))
                );
                assert!(!applied.contains_key(old));
            }
        }
        assert_eq!(applied.len() + repeats, count, "{name}");
        assert_eq!(
            applied.len() + repeats + unresolved.len(),
            meta["answers"].as_object().unwrap().len()
        );
        // An unresolved old feature has no accepted answer in the current interview.
        let qs = specify_core::api::questions(&analysis, applied).unwrap();
        let open = specify_core::rules::open_questions(&qs, applied);
        for u in &unresolved {
            if open.iter().any(|q| q.id == u["id"].as_str().unwrap()) {
                assert!(!applied.contains_key(u["id"].as_str().unwrap()));
            }
        }
        // Saving again uses current recognition, and the exact old answers remain readable.
        let intent = specify_core::api::apply(&analysis, applied, true).unwrap();
        let (bytes, _) =
            specify_core::writer::write_bytes(&loaded, &intent, Some(applied)).unwrap();
        let rewritten = specify_core::part::load_bytes(name, bytes).unwrap();
        assert_eq!(answers_for(&rewritten).unwrap().as_ref(), Some(applied));
        eprintln!(
            "{name}: {} reattached, {} unresolved with old ID, {} migrated",
            applied.len(),
            unresolved.len(),
            migrated.len()
        );
    }
}

#[test]
fn schema_three_recovers_within_centroid_tolerance_and_across_version_bump() {
    let dir = temp("schema3-tolerance");
    let given = json!({
        "part.material": "steel",
        "part.coating": "None (as machined)",
        "part.heat_treatment": "None",
    });
    let (out, _) = written_plate(&dir, &given);
    let raw = std::fs::read(&out).unwrap();
    let expected = specify_core::feature_table::storable(given.as_object().unwrap());
    let base_payload = Value::Object(stored(&raw).remove(&0).unwrap());
    assert_eq!(base_payload["schema"], json!(3));

    for version in [
        quiddity::correspondence::FINGERPRINT_VERSION,
        "quiddity-rust/fingerprint/4",
    ] {
        let mut payload = base_payload.clone();
        payload["geometry"]["fingerprint_version"] = json!(version);
        let x = payload["geometry"]["faces"][0]["centroid"][0]
            .as_f64()
            .unwrap();
        // Shift centroid by 0.005 mm (within 0.01 mm recovery tolerance).
        payload["geometry"]["faces"][0]["centroid"][0] = json!(x + 0.005);
        let path = dir.join(format!("tol-{}.step", version.replace('/', "_")));
        std::fs::write(&path, with_payload_bytes(&raw, &payload)).unwrap();
        let loaded = load(&path, None).unwrap();
        assert_eq!(answers_for(&loaded).unwrap().as_ref(), Some(&expected));
        let (a, warnings) = specify_core::api::analyse_reporting(&loaded).unwrap();
        assert_eq!(a["resume"]["answers"], json!(expected));
        assert!(a.get("resume_warning").is_none());
        assert!(warnings.is_empty(), "{warnings:?}");
        let review = specify_core::api::review(&a, &expected).unwrap();
        assert!(
            !review["problems"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["code"] == "saved_answers_not_reused")
        );
    }
}

#[test]
fn schema_three_rebinds_unchanged_part_via_quiddity_correspond() {
    let dir = temp("schema3-correspond");
    let given = json!({
        "part.material": "steel",
        "part.coating": "None (as machined)",
    });
    let (out, _) = written_plate(&dir, &given);
    let raw = std::fs::read(&out).unwrap();
    let expected = specify_core::feature_table::storable(given.as_object().unwrap());
    let mut payload = Value::Object(stored(&raw).remove(&0).unwrap());
    // Translate the saved Quiddity fingerprints by +100 mm along X: far outside the 0.01 mm
    // tolerant check, so only Quiddity's `correspond` (rigid alignment) can recover it.
    let shift = |v: &mut Value| {
        if let Some(arr) = v.as_array_mut()
            && let Some(x) = arr.first().and_then(Value::as_f64)
        {
            arr[0] = json!(x + 100.0);
        }
    };
    for face in payload["geometry"]["faces"].as_array_mut().unwrap() {
        shift(&mut face["centroid"]);
        shift(&mut face["support"]);
    }
    for feat in payload["geometry"]["features"].as_array_mut().unwrap() {
        shift(&mut feat["position"]);
    }
    let path = dir.join("translated.step");
    std::fs::write(&path, with_payload_bytes(&raw, &payload)).unwrap();
    let loaded = load(&path, None).unwrap();
    assert_eq!(answers_for(&loaded).unwrap().as_ref(), Some(&expected));
    let (a, warnings) = specify_core::api::analyse_reporting(&loaded).unwrap();
    assert_eq!(a["resume"]["answers"], json!(expected));
    assert!(a.get("resume_warning").is_none());
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn real_part_change_is_rejected_and_reported_across_api_cli_and_mcp() {
    let dir = temp("real-change-reported");
    let given = json!({
        "part.material": "steel",
        "part.coating": "None (as machined)",
    });
    let (out, _) = written_plate(&dir, &given);
    let raw = std::fs::read(&out).unwrap();
    let base_payload = Value::Object(stored(&raw).remove(&0).unwrap());

    // 1. Centroid moved beyond 0.01 mm tolerance (0.05 mm) -> rejected and reported.
    let mut moved = base_payload.clone();
    let x = moved["geometry"]["faces"][0]["centroid"][0]
        .as_f64()
        .unwrap();
    moved["geometry"]["faces"][0]["centroid"][0] = json!(x + 0.05);
    moved["geometry"]["fingerprint_version"] = json!("quiddity-rust/fingerprint/4");

    // 2. Hole radius changed with identical centroid -> rejected and reported.
    let mut resized = base_payload.clone();
    let cyl = resized["geometry"]["faces"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["surface"] == "cylinder")
        .unwrap();
    let r = cyl["parameters"]["radius"].as_f64().unwrap();
    cyl["parameters"]["radius"] = json!(r + 0.5);

    // 3. Real edited revision: plate_thickened.old answers embedded onto plate_thickened.new.
    let old_thick = load(
        &common::fixtures().join("revisions/plate_thickened.old.step.gz"),
        None,
    )
    .unwrap();
    let new_thick = load(
        &common::fixtures().join("revisions/plate_thickened.new.step.gz"),
        None,
    )
    .unwrap();
    let old_payload = resume::payload(
        &old_thick,
        &specify_core::feature_table::storable(given.as_object().unwrap()),
    )
    .unwrap();
    let new_written = {
        let a = specify_core::api::analyse(&new_thick).unwrap();
        let intent = apply(&a, given.clone());
        let (bytes, _) =
            specify_core::writer::write_bytes(&new_thick, &intent, given.as_object()).unwrap();
        with_payload_bytes(&bytes, &old_payload)
    };

    let mut cases = vec![
        ("centroid-moved.step", with_payload_bytes(&raw, &moved)),
        ("radius-resized.step", with_payload_bytes(&raw, &resized)),
        ("plate-thickened.step", new_written),
    ];

    for (name, bytes) in cases.drain(..) {
        let path = dir.join(name);
        std::fs::write(&path, &bytes).unwrap();
        let loaded = load(&path, None).unwrap();
        assert!(answers_for(&loaded).unwrap().is_none(), "{name}");
        let (a, warnings) = specify_core::api::analyse_reporting(&loaded).unwrap();
        assert!(a.get("resume").is_none(), "{name}");
        assert_eq!(
            a["resume_warning"].as_str(),
            Some(resume::NOT_REUSED_MESSAGE),
            "{name}"
        );
        assert!(
            warnings.iter().any(|w| w == resume::NOT_REUSED_MESSAGE),
            "{name}: {warnings:?}"
        );
        let review = specify_core::api::review(&a, &Map::new()).unwrap();
        assert!(
            review["problems"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["code"] == "saved_answers_not_reused"
                    && p["severity"] == "warning"
                    && p["message"] == resume::NOT_REUSED_MESSAGE),
            "{name}: {}",
            review["problems"]
        );

        // CLI reports it on stderr (never silent).
        let cli = std::process::Command::new(env!("CARGO_BIN_EXE_specify-core"))
            .arg("analyse")
            .arg(&path)
            .arg("-o")
            .arg(dir.join(format!("{name}.analysis.json")))
            .output()
            .unwrap();
        assert!(cli.status.success(), "{name}");
        let stderr = String::from_utf8_lossy(&cli.stderr);
        assert!(
            stderr.contains(resume::NOT_REUSED_MESSAGE),
            "{name} CLI stderr: {stderr}"
        );

        // MCP open reports it in warnings, and MCP review reports it in problems.
        let mut server = specify_core::mcp::Server::new(&[dir.clone()]).unwrap();
        let call = |server: &mut specify_core::mcp::Server, id: u64, tool: &str, args: Value| {
            let req = json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {
                    "_meta": {
                        "io.modelcontextprotocol/protocolVersion": specify_core::mcp::MODERN,
                        "io.modelcontextprotocol/clientCapabilities": {}
                    },
                    "name": tool,
                    "arguments": args,
                }
            });
            server.handle(&req, None).body.unwrap()["result"]["structuredContent"].clone()
        };
        let opened = call(&mut server, 1, "open", json!({"path": path}));
        assert_eq!(opened["resumed"], json!(false), "{name}: {opened}");
        assert!(
            opened["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w == resume::NOT_REUSED_MESSAGE),
            "{name} MCP open: {opened}"
        );
        let reviewed = call(&mut server, 2, "review", json!({"doc": opened["doc"]}));
        assert!(
            reviewed["problems"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["code"] == "saved_answers_not_reused"
                    && p["message"] == resume::NOT_REUSED_MESSAGE),
            "{name} MCP review: {reviewed}"
        );
    }
}

#[test]
fn old_schemas_still_read_and_next_save_rewrites_as_schema_three() {
    let dir = temp("old-schemas-upgrade");
    let given = json!({
        "part.material": "steel",
        "part.coating": "None (as machined)",
    });
    let (out, _) = written_plate(&dir, &given);
    let raw = std::fs::read(&out).unwrap();
    let loaded = load(&out, None).unwrap();
    let expected = specify_core::feature_table::storable(given.as_object().unwrap());

    for (schema, hash) in [
        (1i64, fingerprint_v1(&loaded).unwrap()),
        (2i64, fingerprint(&loaded).unwrap()),
    ] {
        let old_payload = json!({
            "schema": schema,
            "answers": expected,
            "part": 0,
            "face_count": loaded.binding.face_count,
            "faces": hash,
        });
        let legacy_path = dir.join(format!("legacy-s{schema}.step"));
        std::fs::write(&legacy_path, with_payload_bytes(&raw, &old_payload)).unwrap();
        let legacy = load(&legacy_path, None).unwrap();
        assert_eq!(stored(&legacy.bytes)[&0]["schema"], json!(schema));
        assert_eq!(answers_for(&legacy).unwrap().as_ref(), Some(&expected));
        let (a, warnings) = specify_core::api::analyse_reporting(&legacy).unwrap();
        assert_eq!(a["resume"]["answers"], json!(expected));
        assert!(warnings.is_empty(), "schema {schema}: {warnings:?}");

        // Next save rewrites the legacy schema 1/2 payload as schema 3 with Quiddity fingerprints.
        let upgraded_path = dir.join(format!("upgraded-s{schema}.step"));
        write(&legacy, &apply(&a, given.clone()), &upgraded_path, given.as_object()).unwrap();
        let upgraded = load(&upgraded_path, None).unwrap();
        let rewritten = &stored(&upgraded.bytes)[&0];
        assert_eq!(rewritten["schema"], json!(3));
        assert!(rewritten.get("faces").is_none());
        assert_eq!(
            rewritten["geometry"]["fingerprint_version"],
            json!(quiddity::correspondence::FINGERPRINT_VERSION)
        );
        assert_eq!(answers_for(&upgraded).unwrap().as_ref(), Some(&expected));
    }
}
