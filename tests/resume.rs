//! Answers stored in a written file and resumed from it: specify-core's `tests/test_resume.py`,
//! the cross-engine forms (Python's stored answers read here, this port's read by Python's
//! pattern), the fingerprint's JSON text, and a Python-written file rewritten by replace.

mod common;

use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use specify_pmi::part::load;
use specify_pmi::pyjson;
use specify_pmi::resume::{self, answers_for, fingerprint, fingerprint_of, fingerprint_v1, stored};
use specify_pmi::rules::{Intent, apply_with_defaults};
use specify_pmi::writer::write;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-pmi-resume-tests")
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
    let analysis = specify_pmi::api::analyse(&loaded).unwrap();
    let given = json!({"part.material":"steel", "part.notes":["Keep dry. ".repeat(1000)]});
    let intent = apply(&analysis, given.clone());
    let (bytes, _) =
        specify_pmi::writer::write_bytes(&loaded, &intent, Some(given.as_object().unwrap()))
            .unwrap();
    let back = specify_pmi::part::load_bytes("chunked.step", bytes.clone()).unwrap();
    // ADR P11: the long note is the file's PMI, not a stored answer.
    assert_eq!(
        answers_for(&back).unwrap(),
        Some(specify_pmi::feature_table::storable(
            given.as_object().unwrap()
        ))
    );
    assert!(String::from_utf8_lossy(&bytes).contains("haecceity text chunk/1"));
    let resumed = specify_pmi::api::analyse(&back).unwrap();
    let intent = apply(&resumed, given.clone());
    let (again, _) =
        specify_pmi::writer::write_bytes(&back, &intent, Some(given.as_object().unwrap())).unwrap();
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
    let base = specify_pmi::api::analyse(&pin).unwrap();
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
    let storable = specify_pmi::feature_table::storable(given.as_object().unwrap());
    assert_eq!(answers_for(&loaded).unwrap(), Some(storable.clone()));
    let again = specify_pmi::api::analyse(&loaded).unwrap();
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
    let base = specify_pmi::api::analyse(&loaded).unwrap();
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
        Some(specify_pmi::feature_table::storable(
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
        let (a, warnings) = specify_pmi::api::analyse_reporting(&back).unwrap();
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
    let rust: Vec<Value> = specify_pmi::features::describe_faces(&loaded)
        .unwrap()
        .iter()
        .map(|f| {
            json!([
                f.kind,
                f.centroid
                    .unwrap()
                    .iter()
                    .map(|c| specify_pmi::pyfmt::round_half_even(*c, 3))
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
        Some(specify_pmi::feature_table::storable(
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
        let (analysis, warnings) = specify_pmi::api::analyse_reporting(&back).unwrap();
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
        let (analysis, warnings) = specify_pmi::api::analyse_reporting(&loaded).unwrap();
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
                && specify_pmi::feature_table::storable(&Map::from_iter([(
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
        let qs = specify_pmi::api::questions(&analysis, applied).unwrap();
        let open = specify_pmi::rules::open_questions(&qs, applied);
        for u in &unresolved {
            if open.iter().any(|q| q.id == u["id"].as_str().unwrap()) {
                assert!(!applied.contains_key(u["id"].as_str().unwrap()));
            }
        }
        // Saving again uses current recognition, and the exact old answers remain readable.
        let intent = specify_pmi::api::apply(&analysis, applied, true).unwrap();
        let (bytes, _) = specify_pmi::writer::write_bytes(&loaded, &intent, Some(applied)).unwrap();
        let rewritten = specify_pmi::part::load_bytes(name, bytes).unwrap();
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
fn schema_three_recovers_across_version_bump_by_quiddity_verdict() {
    let dir = temp("schema3-tolerance");
    let given = json!({
        "part.material": "steel",
        "part.coating": "None (as machined)",
        "part.heat_treatment": "None",
    });
    let (out, _) = written_plate(&dir, &given);
    let raw = std::fs::read(&out).unwrap();
    let expected = specify_pmi::feature_table::storable(given.as_object().unwrap());
    let base_payload = Value::Object(stored(&raw).remove(&0).unwrap());
    assert_eq!(base_payload["schema"], json!(3));

    // The same part, its fingerprints labelled as an earlier format's: quiddity reads them and
    // rules the part unchanged (a face's centroid moving a hair between versions is no change).
    let mut payload = base_payload.clone();
    payload["geometry"]["fingerprint_version"] = json!("quiddity-rust/fingerprint/4");
    let x = payload["geometry"]["faces"][0]["centroid"][0]
        .as_f64()
        .unwrap();
    payload["geometry"]["faces"][0]["centroid"][0] = json!(x + 0.005);
    let path = dir.join("tol-fingerprint_4.step");
    std::fs::write(&path, with_payload_bytes(&raw, &payload)).unwrap();
    let loaded = load(&path, None).unwrap();
    assert_eq!(answers_for(&loaded).unwrap().as_ref(), Some(&expected));
    let (a, warnings) = specify_pmi::api::analyse_reporting(&loaded).unwrap();
    assert_eq!(a["resume"]["answers"], json!(expected));
    assert!(a.get("resume_warning").is_none());
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// Specify #271: cgb217's fingerprints as quiddity 0.5.5 wrote them (/4), against this
/// quiddity's (/5) of the same file, are an unchanged part: its saved answers rebind.
#[test]
fn cgb217_fingerprint_4_is_unchanged_against_the_current_fingerprints() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let path = corpus.join("cadgenbench_inputs/cgb217.step.gz");
    if !path.is_file() {
        return;
    }
    let loaded = load(&path, None).unwrap();
    let old = common::read_json_gz(&common::fixtures().join("resume_fp4/cgb217.fingerprint4.json.gz"));
    let corr = quiddity::correspondence::correspond(
        &serde_json::from_value(old.clone()).unwrap(),
        &loaded.recognition().fingerprints,
    )
    .unwrap();
    assert_eq!(corr.verdict, quiddity::correspondence::Verdict::Unchanged);
    let mut evidence = resume::evidence(&loaded).unwrap();
    evidence["geometry"] = old;
    assert!(resume::evidence_matches(&loaded, &evidence).unwrap());
}

#[test]
fn schema_three_rebinds_unchanged_part_via_quiddity_correspond() {
    let dir = temp("schema3-correspond");
    let given = json!({
        "part.material": "steel",
        "part.coating": "None (as machined)",
    });
    // Use an asymmetric part (four distinct holes + off-centre boss): symmetric parts
    // have an ambiguous rigid alignment and must not be rebound silently.
    let asymmetric = load(
        &common::fixtures().join("revisions/hole_moved.old.step.gz"),
        None,
    )
    .unwrap();
    let base = specify_pmi::api::analyse(&asymmetric).unwrap();
    let (raw, _) = specify_pmi::writer::write_bytes(
        &asymmetric,
        &apply(&base, given.clone()),
        given.as_object(),
    )
    .unwrap();
    let expected = specify_pmi::feature_table::storable(given.as_object().unwrap());
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
    let (a, warnings) = specify_pmi::api::analyse_reporting(&loaded).unwrap();
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
        &specify_pmi::feature_table::storable(given.as_object().unwrap()),
    )
    .unwrap();
    let new_written = {
        let a = specify_pmi::api::analyse(&new_thick).unwrap();
        let intent = apply(&a, given.clone());
        let (bytes, _) =
            specify_pmi::writer::write_bytes(&new_thick, &intent, given.as_object()).unwrap();
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
        let (a, warnings) = specify_pmi::api::analyse_reporting(&loaded).unwrap();
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
        let review = specify_pmi::api::review(&a, &Map::new()).unwrap();
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
        let cli = std::process::Command::new(env!("CARGO_BIN_EXE_specify-pmi"))
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
        let mut server = specify_pmi::mcp::Server::new(std::slice::from_ref(&dir)).unwrap();
        let call = |server: &mut specify_pmi::mcp::Server, id: u64, tool: &str, args: Value| {
            let req = json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": {
                    "_meta": {
                        "io.modelcontextprotocol/protocolVersion": specify_pmi::mcp::MODERN,
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
    let expected = specify_pmi::feature_table::storable(given.as_object().unwrap());

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
        let (a, warnings) = specify_pmi::api::analyse_reporting(&legacy).unwrap();
        assert_eq!(a["resume"]["answers"], json!(expected));
        assert!(warnings.is_empty(), "schema {schema}: {warnings:?}");

        // Next save rewrites the legacy schema 1/2 payload as schema 3 with Quiddity fingerprints.
        let upgraded_path = dir.join(format!("upgraded-s{schema}.step"));
        write(
            &legacy,
            &apply(&a, given.clone()),
            &upgraded_path,
            given.as_object(),
        )
        .unwrap();
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

/// Swap two face entries in the part's first `CLOSED_SHELL`, renumbering those two faces
/// without changing any geometry.
fn swap_closed_shell_faces(bytes: &[u8], a: usize, b: usize) -> Vec<u8> {
    use haecceity::p21::{Attribute, Document, Edit, RawEntity};
    let doc = Document::parse(bytes.to_vec()).unwrap();
    let shell_id = doc
        .ids()
        .find(|&id| {
            matches!(
                doc.get(id),
                Some(RawEntity::Simple { name, .. }) if name == "CLOSED_SHELL"
            )
        })
        .expect("a CLOSED_SHELL");
    let RawEntity::Simple {
        name, attributes, ..
    } = doc.get(shell_id).unwrap()
    else {
        unreachable!();
    };
    let mut attributes = attributes.clone();
    let Attribute::List(faces) = &mut attributes[1] else {
        panic!("expected face list");
    };
    faces.swap(a, b);
    let mut edit = Edit::new();
    edit.replace(shell_id, haecceity::p21::simple(name, attributes));
    doc.apply(&edit).unwrap().bytes
}

/// Turn `face_grooves.step` end for end (180° rotation about X around z=3.5:
/// `(x, y, z) -> (x, -y, 7 - z)`, `(dx, dy, dz) -> (dx, -dy, -dz)`) without renumbering faces.
fn flip_face_grooves_end_for_end(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    let num = r"([-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?)";
    let pt_re = Regex::new(&format!(
        r"(CARTESIAN_POINT\('[^']*',)\({num},{num},{num}\)"
    ))
    .unwrap();
    let dir_re = Regex::new(&format!(r"(DIRECTION\('[^']*',)\({num},{num},{num}\)")).unwrap();
    let fmt = |v: f64| {
        let v = if v == 0.0 { 0.0 } else { v };
        let s = format!("{v}");
        if s.contains('.') || s.contains('e') || s.contains('E') {
            s
        } else {
            format!("{s}.")
        }
    };
    let text = pt_re
        .replace_all(&text, |caps: &regex::Captures<'_>| {
            let x: f64 = caps[2].parse().unwrap();
            let y: f64 = caps[3].parse().unwrap();
            let z: f64 = caps[4].parse().unwrap();
            format!("{}({},{},{})", &caps[1], fmt(x), fmt(-y), fmt(7.0 - z))
        })
        .into_owned();
    let text = dir_re
        .replace_all(&text, |caps: &regex::Captures<'_>| {
            let x: f64 = caps[2].parse().unwrap();
            let y: f64 = caps[3].parse().unwrap();
            let z: f64 = caps[4].parse().unwrap();
            format!("{}({},{},{})", &caps[1], fmt(x), fmt(-y), fmt(-z))
        })
        .into_owned();
    text.into_bytes()
}

/// Embed a stored answers payload directly onto `loaded` as a Specify user-defined attribute.
fn embed_payload_bytes(loaded: &specify_pmi::part::LoadedPart, payload: &Value) -> Vec<u8> {
    use haecceity::p21::Document;
    let doc = Document::parse(loaded.bytes.to_vec()).unwrap();
    let model = haecceity::pmi::PartPmi {
        attributes: vec![resume::attribute_set(payload)],
        ..Default::default()
    };
    let (edit, _) = haecceity::pmi::write(
        &doc,
        std::slice::from_ref(&loaded.definition),
        &[(haecceity::pmi::PartId(0), model)],
        haecceity::pmi::Mode::Add,
        haecceity::pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    doc.apply(&edit).unwrap().bytes
}

#[test]
fn swapping_spindle_faces_4_and_8_rebinds_general_plane_length_none() {
    let spindle = load(&common::fixtures().join("spindle.step"), None).unwrap();
    // Dismiss the 12 mm plane length (faces 8-10), leaving the 34 mm plane length (faces 4-10) open.
    let applied = common::apply_edits(&spindle, json!({"none": ["length:planes:8-10"]}));
    let before = specify_pmi::part::load_bytes("spindle-none.step", applied.bytes.clone()).unwrap();
    let a_before = specify_pmi::api::analyse(&before).unwrap();
    assert_eq!(
        a_before["resume"]["answers"]["length:planes:8-10"],
        json!("none")
    );

    // Swap faces 4 and 8 in CLOSED_SHELL: the 12 mm plane moves from face 8 to face 4,
    // and the 34 mm plane moves from face 4 to face 8.
    let swapped_bytes = swap_closed_shell_faces(&applied.bytes, 4, 8);
    let swapped = specify_pmi::part::load_bytes("spindle-swapped.step", swapped_bytes).unwrap();
    let (analysis, warnings) = specify_pmi::api::analyse_reporting(&swapped).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(analysis.get("resume_warning").is_none());

    let resumed_answers = analysis["resume"]["answers"].as_object().unwrap();
    assert_eq!(
        resumed_answers.get("length:planes:4-10"),
        Some(&json!("none"))
    );
    assert!(
        !resumed_answers.contains_key("length:planes:8-10"),
        "saved none must not stay on length:planes:8-10: {resumed_answers:?}"
    );
    assert_eq!(
        answers_for(&swapped)
            .unwrap()
            .unwrap()
            .get("length:planes:4-10"),
        Some(&json!("none"))
    );
    assert!(
        !answers_for(&swapped)
            .unwrap()
            .unwrap()
            .contains_key("length:planes:8-10")
    );

    // In the swapped part, 4-10 (12 mm) stays decided_none and 8-10 (34 mm) stays open (gap).
    let rows = analysis["table"]["rows"].as_array().unwrap();
    let row_12 = rows
        .iter()
        .find(|r| r["id"] == "length:planes:4-10")
        .unwrap();
    let row_34 = rows
        .iter()
        .find(|r| r["id"] == "length:planes:8-10")
        .unwrap();
    assert_eq!(row_12["state"], json!("decided_none"), "{row_12}");
    assert_eq!(row_34["state"], json!("gap"), "{row_34}");
    assert!(
        row_34["suggestion"].as_str().unwrap().starts_with("34 "),
        "{row_34}"
    );
    let review = specify_pmi::api::review(&analysis, resumed_answers).unwrap();
    let open = review["open"].as_array().unwrap();
    assert!(open.contains(&json!("length:planes:8-10")), "{open:?}");
    assert!(!open.contains(&json!("length:planes:4-10")), "{open:?}");
}

#[test]
fn hole_removed_rematch_leaves_deleted_bore_finish_open_and_refuses_write() {
    let old = load(
        &common::fixtures().join("revisions/hole_removed.old.step.gz"),
        None,
    )
    .unwrap();
    let new = load(
        &common::fixtures().join("revisions/hole_removed.new.step.gz"),
        None,
    )
    .unwrap();
    // Face 7 in old is the deleted Ø7 bore; face 7 in new is a different Ø8 bore (was face 8).
    let saved = json!({
        "part.material": "steel",
        "extra.bore_finish": {"kind": "finish", "faces": [7], "value": "Ra 0.8"},
        "_specify": {"evidence": resume::evidence(&old).unwrap(), "unresolved": {}},
    })
    .as_object()
    .unwrap()
    .clone();

    let new_analysis = specify_pmi::api::analyse(&new).unwrap();
    let new_features: Vec<specify_pmi::features::Feature> =
        serde_json::from_value(new_analysis["features"].clone()).unwrap();
    let rematched = resume::rematch_answers(&new, &new_features, &saved, None, None).unwrap();

    assert!(
        !rematched.answers.contains_key("extra.bore_finish"),
        "deleted bore finish must not be kept on new face 7: {:?}",
        rematched.answers
    );
    assert!(
        rematched
            .unresolved
            .iter()
            .any(|u| u.id == "extra.bore_finish" && u.answer == saved["extra.bore_finish"]),
        "{:?}",
        rematched.unresolved
    );
    assert!(
        rematched.answers["_specify"]["unresolved"]
            .get("extra.bore_finish")
            .is_some(),
        "unresolved carry block must be attached for extra.bore_finish: {:?}",
        rematched.answers["_specify"]
    );
    // Preview produces no finish requirement on face 7, and apply refuses the unresolved extra block.
    let preview = specify_pmi::api::preview(&new_analysis, &rematched.answers).unwrap();
    assert!(
        preview.requirements.is_empty(),
        "must not plan a finish onto new face 7: {:?}",
        preview.requirements
    );
    let err = specify_pmi::api::apply(&new_analysis, &rematched.answers, true).unwrap_err();
    assert!(
        err.to_string().contains("extra.bore_finish"),
        "expected unresolved block error, got: {err}"
    );
}

#[test]
fn swapping_coplanar_face_grooves_lands_fails_tolerant_write_check() {
    let fg = load(
        &common::fixtures().join("quiddity023/face_grooves.step"),
        None,
    )
    .unwrap();
    let evidence = resume::evidence(&fg).unwrap();
    let saved = json!({
        "extra.land_flat": {"kind": "flatness", "faces": [0], "value": "0.02"},
        "_specify": {"evidence": evidence, "unresolved": {}},
    })
    .as_object()
    .unwrap()
    .clone();

    // Faces 0 and 4 are coplanar concentric annular lands sharing centroid [0, 0, 0]
    // with different areas (82.47 vs 111.33 mm²) and neighbours.
    let swapped_bytes = swap_closed_shell_faces(&fg.bytes, 0, 4);
    let swapped =
        specify_pmi::part::load_bytes("face_grooves_swapped.step", swapped_bytes).unwrap();

    assert!(
        !resume::evidence_matches(&swapped, &evidence).unwrap(),
        "swapping coplanar concentric annular lands must not pass evidence_matches"
    );
    let analysis = specify_pmi::api::analyse(&swapped).unwrap();
    let plain = json!({
        "extra.land_flat": {"kind": "flatness", "faces": [0], "value": "0.02"},
    })
    .as_object()
    .unwrap()
    .clone();
    let intent = specify_pmi::api::preview(&analysis, &plain).unwrap();
    let err = specify_pmi::writer::write_bytes(&swapped, &intent, Some(&saved)).unwrap_err();
    assert!(
        err.to_string().contains("stale answers"),
        "writer must refuse stale evidence on swapped coplanar lands: {err}"
    );
}

#[test]
fn turning_face_grooves_end_for_end_does_not_move_flatness_to_opposite_end() {
    let fg = load(
        &common::fixtures().join("quiddity023/face_grooves.step"),
        None,
    )
    .unwrap();
    let answers = json!({
        "extra.end_flat": {"kind": "flatness", "faces": [0], "value": "0.02"},
        "datum.A": [0],
        "datum.A.flatness": "0.02",
    })
    .as_object()
    .unwrap()
    .clone();
    let payload = resume::payload(&fg, &answers).unwrap();
    let evidence = resume::evidence(&fg).unwrap();

    // Turn face_grooves end for end without renumbering its faces: face 0 moves from z=0
    // to z=7 and face 10 moves from z=7 to z=0. Because the part is end-for-end symmetric,
    // correspondence cannot distinguish the two ends and must report the saved answers as
    // not reused rather than moving face 0's flatness onto opposite-end face 10.
    let flipped_bytes = flip_face_grooves_end_for_end(&fg.bytes);
    let flipped_bare =
        specify_pmi::part::load_bytes("face_grooves_flipped.step", flipped_bytes).unwrap();
    let flipped_with_payload = specify_pmi::part::load_bytes(
        "face_grooves_flipped_saved.step",
        embed_payload_bytes(&flipped_bare, &payload),
    )
    .unwrap();

    assert!(answers_for(&flipped_with_payload).unwrap().is_none());
    let (analysis, warnings) = specify_pmi::api::analyse_reporting(&flipped_with_payload).unwrap();
    assert!(analysis.get("resume").is_none(), "{analysis}");
    assert_eq!(
        analysis["resume_warning"].as_str(),
        Some(resume::NOT_REUSED_MESSAGE)
    );
    assert!(
        warnings.iter().any(|w| w == resume::NOT_REUSED_MESSAGE),
        "{warnings:?}"
    );

    // External rematch_answers also leaves face-0 answers open rather than moving them to face 10.
    let features: Vec<specify_pmi::features::Feature> =
        serde_json::from_value(analysis["features"].clone()).unwrap();
    let rematched =
        resume::rematch_answers(&flipped_bare, &features, &answers, Some(&evidence), None).unwrap();
    assert!(rematched.answers.is_empty(), "{:?}", rematched.answers);
    assert_eq!(rematched.unresolved.len(), 3, "{:?}", rematched.unresolved);
    assert!(rematched.unverified.is_some());
}

#[test]
fn direct_material_edit_after_bore_move_preserves_locating_and_warns() {
    let dir = temp("direct-edit-bore-move");
    let given = json!({
        "part.material": "steel",
        "part.locating": specify_pmi::rules::LOCATING[1],
    });
    let (out, _) = written_plate(&dir, &given);
    let raw = std::fs::read(&out).unwrap();
    // Simulate a bore moving 0.1 mm since the saved answers were written.
    let mut payload = Value::Object(stored(&raw).remove(&0).unwrap());
    assert_eq!(
        payload["answers"]["part.locating"],
        json!(specify_pmi::rules::LOCATING[1])
    );
    let bore = payload["geometry"]["faces"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["surface"] == "cylinder")
        .unwrap();
    let bx = bore["centroid"][0].as_f64().unwrap();
    bore["centroid"][0] = json!(bx + 0.1);
    let moved_bytes = with_payload_bytes(&raw, &payload);
    let moved = specify_pmi::part::load_bytes("bore-moved.step", moved_bytes).unwrap();

    // Direct material edit must not silently drop part.locating or return empty warnings.
    let applied = common::apply_edits(
        &moved,
        json!({"change": {"part.material": {"expect": "steel", "to": "6061-T6"}}}),
    );
    assert!(
        applied
            .report
            .warnings
            .iter()
            .any(|w| w == resume::NOT_REUSED_MESSAGE),
        "direct edit must warn when saved answers were not reused: {:?}",
        applied.report.warnings
    );

    // The saved payload (including part.locating) is preserved, material is updated,
    // and reopening the output still reports the non-reuse warning.
    let stored_after = stored(&applied.bytes)
        .remove(&0)
        .expect("saved payload kept");
    assert_eq!(
        stored_after["answers"]["part.locating"],
        json!(specify_pmi::rules::LOCATING[1])
    );
    let reopened = specify_pmi::part::load_bytes("edited.step", applied.bytes).unwrap();
    let (analysis, warnings) = specify_pmi::api::analyse_reporting(&reopened).unwrap();
    assert_eq!(analysis["part"]["material"], json!("6061-T6"));
    assert_eq!(
        analysis["resume_warning"].as_str(),
        Some(resume::NOT_REUSED_MESSAGE)
    );
    assert!(
        warnings.iter().any(|w| w == resume::NOT_REUSED_MESSAGE),
        "{warnings:?}"
    );
}

/// `face_grooves.step` with face `face` given the authored name `name`.
fn name_face(
    bytes: &[u8],
    loaded: &specify_pmi::part::LoadedPart,
    face: usize,
    name: &str,
) -> Vec<u8> {
    use haecceity::p21::{Attribute, Document, Edit, RawEntity};
    let doc = Document::parse(bytes.to_vec()).unwrap();
    let id = loaded.face_entity(face);
    let RawEntity::Simple {
        name: entity,
        attributes,
        ..
    } = doc.get(id).unwrap()
    else {
        panic!("face entity")
    };
    let mut attributes = attributes.clone();
    attributes[0] = haecceity::p21::Attribute::String(name.into());
    let _: Option<&Attribute> = None;
    let mut edit = Edit::new();
    edit.replace(id, haecceity::p21::simple(entity, attributes));
    doc.apply(&edit).unwrap().bytes
}

#[test]
fn mixed_names_and_geometry_that_collide_hold_the_answer_back_not_drop_it() {
    let fg = load(
        &common::fixtures().join("quiddity023/face_grooves.step"),
        None,
    )
    .unwrap();
    let named_bytes = name_face(&fg.bytes, &fg, 2, "f:fg.land;M:fg.body");
    let named = specify_pmi::part::load_bytes("fg-named.step", named_bytes).unwrap();
    let applied = common::apply_edits(&named, json!({"none": ["length:planes:2-8"]}));
    let saved = specify_pmi::part::load_bytes("fg-saved.step", applied.bytes.clone()).unwrap();
    let payload = stored(&applied.bytes).remove(&0).unwrap();
    assert!(payload.contains_key("face_names") && payload.contains_key("geometry"));
    assert_eq!(
        specify_pmi::api::analyse(&saved).unwrap()["resume"]["answers"]["length:planes:2-8"],
        json!("none")
    );

    let flipped = specify_pmi::part::load_bytes(
        "fg-flipped.step",
        flip_face_grooves_end_for_end(&applied.bytes),
    )
    .unwrap();
    let (analysis, warnings) = specify_pmi::api::analyse_reporting(&flipped).unwrap();
    let answers = analysis["resume"]["answers"].as_object().unwrap();
    assert!(
        !answers.contains_key("length:planes:2-2") && !answers.contains_key("length:planes:2-8"),
        "{answers:?}"
    );
    let unresolved = analysis["resume"]["unresolved"].as_array().unwrap();
    let held = unresolved
        .iter()
        .find(|u| u["id"] == "length:planes:2-8")
        .unwrap_or_else(|| panic!("the answer must be reported: {unresolved:?}"));
    assert_eq!(held["answer"], json!("none"));
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("length:planes:2-8") && w.contains("not reused")),
        "{warnings:?}"
    );
}

/// `bytes` with every `CARTESIAN_POINT` moved by `dx` along X: a pure rigid translation.
fn translate_x(bytes: &[u8], dx: f64) -> Vec<u8> {
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    let num = r"([-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?)";
    let pt = Regex::new(&format!(
        r"(CARTESIAN_POINT\('[^']*',)\({num},{num},{num}\)"
    ))
    .unwrap();
    pt.replace_all(&text, |c: &regex::Captures<'_>| {
        let x: f64 = c[2].parse().unwrap();
        format!("{}({:?},{},{})", &c[1], x + dx, &c[3], &c[4])
    })
    .into_owned()
    .into_bytes()
}

#[test]
fn translating_a_symmetric_plate_keeps_its_saved_answers() {
    // Four identical corner holes: the plate is symmetric under turns and flips.
    let plate = load(
        &fixture("four_corner_holes_are_quiddity_rectangle_with_members"),
        None,
    )
    .unwrap();
    let locating = json!(specify_pmi::rules::LOCATING[1]);
    let answers = json!({"part.material": "steel", "part.locating": locating, "datum.A": [0]})
        .as_object()
        .unwrap()
        .clone();
    let saved = embed_payload_bytes(&plate, &resume::payload(&plate, &answers).unwrap());
    let moved =
        specify_pmi::part::load_bytes("plate-moved.step", translate_x(&saved, 100.0)).unwrap();
    let (analysis, warnings) = specify_pmi::api::analyse_reporting(&moved).unwrap();
    assert!(analysis.get("resume_warning").is_none(), "{warnings:?}");
    assert_eq!(analysis["resume"]["answers"]["part.locating"], locating);
    // Order is unchanged, so the face answers rebind too rather than being held back.
    let features: Vec<specify_pmi::features::Feature> =
        serde_json::from_value(analysis["features"].clone()).unwrap();
    let rebound = resume::rematch_answers(
        &moved,
        &features,
        &answers,
        Some(&resume::evidence(&plate).unwrap()),
        None,
    )
    .unwrap();
    assert!(rebound.unresolved.is_empty(), "{:?}", rebound.unresolved);
    assert!(rebound.unverified.is_none());
    assert_eq!(rebound.answers, answers);

    // A later edit keeps the choice.
    let applied = common::apply_edits(&moved, json!({"none": ["part.coating"]}));
    let after = stored(&applied.bytes).remove(&0).unwrap();
    assert_eq!(after["answers"]["part.locating"], locating);
    assert!(after["answers"].get("part.coating").is_some());
    assert!(after.get("unreused").is_none());
}

#[test]
fn part_answers_stand_when_face_answers_are_held_back_and_a_later_edit_keeps_both() {
    let fg = load(
        &common::fixtures().join("quiddity023/face_grooves.step"),
        None,
    )
    .unwrap();
    let locating = json!(specify_pmi::rules::LOCATING[1]);
    let answers = json!({
        "part.locating": locating,
        "extra.end_flat": {"kind": "flatness", "faces": [0], "value": "0.02"},
        "datum.A": [0],
    })
    .as_object()
    .unwrap()
    .clone();
    let payload = resume::payload(&fg, &answers).unwrap();
    let flipped_bare =
        specify_pmi::part::load_bytes("fg-flip.step", flip_face_grooves_end_for_end(&fg.bytes))
            .unwrap();
    let flipped = specify_pmi::part::load_bytes(
        "fg-flip-saved.step",
        embed_payload_bytes(&flipped_bare, &payload),
    )
    .unwrap();

    // The part-wide choice stands; the face answers are held back and reported.
    let (analysis, warnings) = specify_pmi::api::analyse_reporting(&flipped).unwrap();
    assert_eq!(analysis["resume"]["answers"]["part.locating"], locating);
    let ids = |a: &Value| -> Vec<String> {
        let mut ids: Vec<String> = a["resume"]["unresolved"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["id"].as_str().unwrap().to_string())
            .collect();
        ids.sort();
        ids
    };
    assert_eq!(ids(&analysis), ["datum.A", "extra.end_flat"]);
    assert!(
        warnings.iter().any(|w| w.contains("datum.A")),
        "{warnings:?}"
    );

    // A later edit merges into the saved answers: nothing held back is lost, and reopening
    // still reports it.
    let applied = common::apply_edits(&flipped, json!({"none": ["part.coating"]}));
    let after = stored(&applied.bytes).remove(&0).unwrap();
    assert_eq!(after["answers"]["part.locating"], locating);
    assert!(after["answers"].get("part.coating").is_some());
    assert_eq!(after["unreused"]["datum.A"], json!([0]));
    assert!(after["unreused"].get("extra.end_flat").is_some());
    let reopened = specify_pmi::part::load_bytes("fg-edited.step", applied.bytes).unwrap();
    let (again, warnings) = specify_pmi::api::analyse_reporting(&reopened).unwrap();
    assert_eq!(again["resume"]["answers"]["part.locating"], locating);
    assert_eq!(ids(&again), ["datum.A", "extra.end_flat"]);
    assert!(
        warnings.iter().any(|w| w.contains("datum.A")),
        "{warnings:?}"
    );
}
