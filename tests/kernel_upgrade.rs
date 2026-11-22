//! Independent compatibility evidence for the v0.1.1 kernel upgrade. These are Rust-written
//! alpha.6 inputs, not outputs recaptured from Python or regenerated after the pin changed.
use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use serde_json::{Map, Value, json};
use specify_core::{
    api,
    features::{self, Feature},
    part, resume, rules, writer,
};
mod common;

fn open_measurements(analysis: &Value) -> Vec<String> {
    analysis["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            row["open"] == true && matches!(row["kind"].as_str(), Some("length" | "location"))
        })
        .map(|row| row["id"].as_str().unwrap().to_owned())
        .collect()
}
use std::{collections::BTreeSet, path::Path};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// ADR P11 (D6): of the saved answers `expected`, a resumed analysis keeps those that leave no
/// PMI and that the file does not state; one whose requirement the file states, or whose
/// question has no row, is dropped silently; one with no requirement for it in the file is dropped with exactly one warning,
/// and its question is open again. Returns the kept answers and the reopened ids.
fn p11_resumed(
    expected: &Map<String, Value>,
    analysis: &Value,
    warnings: &[String],
) -> (Map<String, Value>, BTreeSet<String>) {
    use specify_core::feature_table::{is_requirement, leaves_nothing};
    let state = |id: &str| {
        analysis["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .map(|r| r["state"].as_str().unwrap().to_string())
    };
    let tail = ": no requirement for this answer in the file; the question is open again";
    let mut kept = Map::new();
    let mut reopened = BTreeSet::new();
    for (id, value) in expected {
        let row = state(id);
        if !is_requirement(id) || (leaves_nothing(value) && row.as_deref() != Some("stated")) {
            kept.insert(id.clone(), value.clone());
            continue;
        }
        let said = format!(
            "saved answer {id} ({}){tail}",
            specify_core::choices::short_repr(value, 6)
        );
        let times = warnings.iter().filter(|w| **w == said).count();
        match row.as_deref() {
            Some("gap") => {
                assert_eq!(times, 1, "{said}: {warnings:?}");
                reopened.insert(id.clone());
            }
            // Stated, or no row: the question is not asked (another feature's row covers its
            // faces); neither is said.
            other => {
                assert!(matches!(other, Some("stated") | None), "{id}: {other:?}");
                assert!(
                    !warnings
                        .iter()
                        .any(|w| w.starts_with(&format!("saved answer {id} "))),
                    "{id}: {warnings:?}"
                );
            }
        }
    }
    assert_eq!(analysis["resume"]["answers"], json!(kept));
    assert_eq!(
        warnings.iter().filter(|w| w.ends_with(tail)).count(),
        reopened.len(),
        "{warnings:?}"
    );
    (kept, reopened)
}

/// The saved answers `saved` as the resumed analysis carries them: migrated ids renamed,
/// unresolved ones left out (they are reported apart).
fn carried(saved: &Value, analysis: &Value) -> Map<String, Value> {
    let resume = &analysis["resume"];
    let moves: Vec<(&str, &str)> = resume["migrated"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|pair| (pair[0].as_str().unwrap(), pair[1].as_str().unwrap()))
        .collect();
    let unresolved: Vec<&str> = resume["unresolved"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|u| u["id"].as_str().unwrap())
        .collect();
    saved
        .as_object()
        .unwrap()
        .iter()
        .filter(|(id, _)| !unresolved.contains(&id.as_str()))
        .map(|(id, value)| {
            let id = moves
                .iter()
                .find(|(old, _)| old == id)
                .map_or(id.as_str(), |(_, new)| new);
            (id.to_string(), value.clone())
        })
        .collect()
}

#[test]
fn five_real_alpha6_files_resume_all_87_answers_including_a_rotated_part() {
    let mut total = 0;
    let mut changed_parts = 0;
    for name in ["plate", "plate-moved", "shaft", "corners", "pin"] {
        let base: Value = serde_json::from_slice(
            &std::fs::read(fixture(&format!("resume-alpha6/{name}.json"))).unwrap(),
        )
        .unwrap();
        let loaded = part::load(&fixture(&format!("resume-alpha6/{name}.step.gz")), None).unwrap();
        let (analysis, warnings) = api::analyse_reporting(&loaded).unwrap();
        // #216 adds planar floor ownership. Only these two saved hole IDs grow;
        // compare every answer value and the complete migration list exactly.
        let rekey = |id: &str| {
            match name {
                "shaft" => id
                    .strip_suffix(":holes:15")
                    .map(|q| format!("{q}:holes:15.16")),
                "pin" => id
                    .strip_suffix(":holes:3")
                    .map(|q| format!("{q}:holes:3.5")),
                _ => None,
            }
            .unwrap_or_else(|| id.into())
        };
        let expected: Map<String, Value> = base["answers"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(id, value)| (rekey(id), value.clone()))
            .collect();
        let migrated: Vec<_> = base["answers"]
            .as_object()
            .unwrap()
            .keys()
            .filter(|id| rekey(id) != **id)
            .map(|id| json!([id, rekey(id)]))
            .collect();
        // ADR P11 (D6): the answers the file states, or whose requirement it lacks (each said
        // once, its question open again), are not resumed; every saved answer is accounted for.
        let (kept, reopened) = p11_resumed(&expected, &analysis, &warnings);
        assert_eq!(warnings.len(), reopened.len(), "{name}: {warnings:?}");
        assert_eq!(
            analysis["resume"]
                .get("migrated")
                .cloned()
                .unwrap_or_else(|| json!([])),
            json!(migrated),
            "{name}"
        );
        assert!(analysis["resume"]["unresolved"].is_null());
        let ids = |value: &Value| {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["id"].as_str().unwrap().to_string())
                .collect::<BTreeSet<_>>()
        };
        if ids(&base["features"]) != ids(&analysis["features"]) {
            changed_parts += 1;
        }
        let answers = analysis["resume"]["answers"].as_object().unwrap();
        total += expected.len();
        // ADR P11 (D6): the reopened legacy questions stay open. Standalone
        // measurements also stay open because their suggestions write PMI.
        let measurements = open_measurements(&analysis);
        let review = api::review(&analysis, answers).unwrap();
        let open = review["open"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let (new, old): (Vec<_>, Vec<_>) =
            open.into_iter().partition(|id| measurements.contains(id));
        assert_eq!(json!(old), json!(reopened), "{name}");
        assert_eq!(
            new.into_iter().collect::<BTreeSet<_>>(),
            measurements.iter().cloned().collect(),
            "{name}"
        );
        // The reopened questions are answered again as saved: their PMI is written, and the
        // answers stored are the kept ones (ADR P11: an answer that writes PMI is not stored).
        let mut given = answers.clone();
        for id in &reopened {
            given.insert(id.clone(), expected[id].clone());
        }
        let Err(api::Error::Rules(rules::Error::Incomplete(incomplete))) =
            api::apply(&analysis, &given, false)
        else {
            panic!("{name}: expected the measurement gaps to remain open");
        };
        assert_eq!(
            incomplete.missing.into_iter().collect::<BTreeSet<_>>(),
            measurements.into_iter().collect(),
            "{name}"
        );
        // Replay the legacy answers as a raw intent, including answers whose
        // value is retained because the writer cannot emit PMI for it.
        let mut intent = api::preview(&analysis, &given).unwrap();
        intent
            .requirements
            .extend(rules::unasked_rows(&analysis, &given).unwrap());
        let (bytes, _) = writer::write_bytes(&loaded, &intent, Some(&given)).unwrap();
        let back = part::File::of_bytes(Path::new(name), bytes)
            .unwrap()
            .load(None)
            .unwrap();
        // ADR P11 rule 2: a reopened answer that wrote nothing (the plate's Ø8 left to the
        // general tolerance, on datum B's own faces) is stored too, listed as such.
        let stored = resume::answers_for(&back).unwrap().unwrap();
        let mut kept = kept.clone();
        let listed: Vec<String> = stored
            .get("_specify")
            .and_then(|m| m.get("no_pmi"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        for id in &listed {
            assert!(reopened.contains(id), "{name}: {id} was not reopened");
            kept.insert(id.clone(), expected[id].clone());
        }
        let payload_no_pmi = (!listed.is_empty()).then(|| json!(listed));
        if let Some(list) = &payload_no_pmi {
            kept.insert("_specify".into(), json!({"no_pmi": list}));
        }
        assert_eq!(stored, kept, "{name}");
        kept.shift_remove("_specify");
        // Stored base64 bytes and literal Unicode/quote/backslash text survive step-io 0.3.
        let mut before = resume::stored(&loaded.bytes);
        // Every payload field and string byte survives apart from the migrated keys.
        before
            .get_mut(&0)
            .unwrap()
            .insert("answers".into(), json!(kept)); // ADR P11: only the kept answers are stored
        if let Some(list) = &payload_no_pmi {
            before
                .get_mut(&0)
                .unwrap()
                .insert("no_pmi".into(), list.clone());
        }
        // Every save marks the payload it writes, so it is the one read (ADR P11).
        before
            .get_mut(&0)
            .unwrap()
            .insert("pmi_scope".into(), json!("specify-core:written:v1"));
        let after = resume::stored(&back.bytes);
        assert_eq!(
            resume::encode(&json!(before[&0])),
            resume::encode(&json!(after[&0]))
        );
        let settings = specify_core::existing::part_settings(&back).unwrap();
        assert_eq!(settings["notes"], base["answers"]["part.notes"]);
    }
    assert_eq!(total, 87);
    assert_eq!(
        changed_parts, 2,
        "shaft and pin feature IDs changed; saved answers must still resume"
    );
}

fn feature(id: &str, family: &str, faces: Vec<usize>) -> Feature {
    Feature {
        id: id.into(),
        family: family.into(),
        faces,
        record: Map::new(),
        members: vec![],
        notes: vec![],
    }
}

#[test]
fn rematching_requires_unique_same_family_support_and_reports_every_refusal() {
    let answers = json!({"hole.function:holes:6": "plain", "part.notes": ["do not edit holes:6"]});
    let current = vec![feature("holes:6.7", "holes", vec![6, 7])];
    let migrated = resume::rematch(answers.as_object().unwrap(), &current);
    assert_eq!(migrated.answers["hole.function:holes:6.7"], "plain");
    assert_eq!(
        migrated.migrated,
        [(
            "hole.function:holes:6".into(),
            "hole.function:holes:6.7".into()
        )]
    );
    assert_eq!(migrated.answers["part.notes"], answers["part.notes"]);
    for (features, reason) in [
        (vec![], "no same-family"),
        (
            vec![feature("bosses:6", "bosses", vec![6])],
            "no same-family",
        ),
        (
            vec![
                feature("holes:6.7", "holes", vec![6, 7]),
                feature("holes:6.8", "holes", vec![6, 8]),
            ],
            "ambiguous",
        ),
    ] {
        let result = resume::rematch(answers.as_object().unwrap(), &features);
        assert_eq!(result.unresolved.len(), 1);
        assert_eq!(result.unresolved[0].id, "hole.function:holes:6");
        assert!(result.unresolved[0].reason.contains(reason));
        assert!(
            !result
                .answers
                .keys()
                .any(|id| id.starts_with("hole.function:"))
        );
    }
    for id in [
        "hole.function:holes:",
        "hole.function:holes:not-a-face",
        "hole.function:missing-family-and-faces",
    ] {
        let malformed = serde_json::Map::from_iter([(id.to_string(), json!("plain"))]);
        let result = resume::rematch(&malformed, &current);
        assert_eq!(result.unresolved.len(), 1, "{id}");
        assert!(result.answers.is_empty(), "{id}");
    }
    let collision = json!({"hole.function:holes:6":"plain", "hole.function:holes:7":"fit:H7"});
    let result = resume::rematch(collision.as_object().unwrap(), &current);
    assert_eq!(result.unresolved.len(), 2);
    assert!(result.answers.is_empty());
    let cross_question_collision =
        json!({"hole.function:holes:6":"plain", "hole.position:holes:7":"0.05"});
    let result = resume::rematch(cross_question_collision.as_object().unwrap(), &current);
    assert_eq!(result.unresolved.len(), 2);
    assert!(
        result.answers.is_empty(),
        "different question types cannot hide merged-feature ambiguity"
    );
}

fn moved_bytes(bytes: Vec<u8>, motion: &quiddity::framed_records::Rigid) -> Vec<u8> {
    let doc = Document::parse(bytes).unwrap();
    let mut edit = Edit::new();
    for id in doc.ids() {
        let Some(RawEntity::Simple {
            name, attributes, ..
        }) = doc.get(id)
        else {
            continue;
        };
        if !matches!(name.as_str(), "CARTESIAN_POINT" | "DIRECTION") {
            continue;
        }
        let Some(Attribute::List(items)) = attributes.get(1) else {
            continue;
        };
        let values: Option<Vec<f64>> = items
            .iter()
            .map(|a| match a {
                Attribute::Real(v) => Some(*v),
                _ => None,
            })
            .collect();
        let Some(values) = values else { continue };
        let Ok(v) = <[f64; 3]>::try_from(values) else {
            continue;
        };
        let v = if name == "CARTESIAN_POINT" {
            motion.point(v)
        } else {
            motion.direction(v)
        };
        let mut attributes = attributes.clone();
        attributes[1] = Attribute::List(v.into_iter().map(Attribute::Real).collect());
        edit.replace(id, haecceity::p21::simple(name, attributes));
    }
    doc.apply(&edit).unwrap().bytes
}

fn close(a: &[f64; 3], b: &[f64; 3], tolerance: f64) {
    assert!(
        (0..3).all(|i| (a[i] - b[i]).abs() < tolerance),
        "{a:?} != {b:?}"
    );
}

#[test]
fn named_parts_keep_questions_defaults_locations_and_written_pmi_under_rigid_motion() {
    let (s, c) = 37_f64.to_radians().sin_cos();
    let (t, d) = 23_f64.to_radians().sin_cos();
    let motion = quiddity::framed_records::Rigid {
        rotation: [[d, t * s, t * c], [0., c, -s], [-t, d * s, d * c]],
        translation: [31., -17., 43.],
    };
    let mut placements_checked = 0;
    let mut inputs: Vec<_> = [
        "plate",
        "pin",
        "GRM-02_shaft",
        "four_corner_holes_are_quiddity_rectangle_with_members",
    ]
    .into_iter()
    .map(|name| (name.to_string(), fixture(&format!("parts/{name}.step"))))
    .collect();
    // Fractional slot/channel ends exercise the same published precision in both paths.
    for case in ["corpus/mfcadpp/10047", "corpus/mfcadpp/10224"] {
        if let Some(path) = common::case(case).input() {
            inputs.push((case.into(), path));
        }
    }
    for (name, path) in inputs {
        let name = name.as_str();
        let bytes = std::fs::read(path).unwrap();
        let a = part::File::of_bytes(Path::new(name), bytes.clone())
            .unwrap()
            .load(None)
            .unwrap();
        let b = part::File::of_bytes(Path::new(name), moved_bytes(bytes, &motion))
            .unwrap()
            .load(None)
            .unwrap();
        // Independent Part::transformed route proves source face indices are preserved.
        let transformed = a
            .part
            .transformed(&[
                [d, t * s, t * c, 31.],
                [0., c, -s, -17.],
                [-t, d * s, d * c, 43.],
            ])
            .unwrap();
        for i in 0..a.part.faces.len() {
            close(
                &b.part.face_moments(i).unwrap().centroid,
                &transformed.face_moments(i).unwrap().centroid,
                1e-6,
            );
        }
        let aa = api::analyse(&a).unwrap();
        let ab = api::analyse(&b).unwrap();
        let qa = rules::questions(&aa, &Map::new()).unwrap();
        let qb = rules::questions(&ab, &Map::new()).unwrap();
        let differences = common::compare(
            name,
            &json!(qa.iter().map(|q| q.to_dict()).collect::<Vec<_>>()),
            &json!(qb.iter().map(|q| q.to_dict()).collect::<Vec<_>>()),
            &common::Tolerances::default().with("*", 1e-5, 1e-8),
        );
        assert!(differences.is_empty(), "{name}: {differences:?}");
        let mut seed = json!({"part.material":"steel"})
            .as_object()
            .unwrap()
            .clone();
        if let Some((question, option)) = qa.iter().find_map(|q| {
            q.id.starts_with("diameter.fit:")
                .then(|| {
                    q.options
                        .iter()
                        .find(|option| option.starts_with("thread:"))
                        .map(|option| (&q.id, option))
                })
                .flatten()
        }) {
            seed.insert(question.clone(), json!(option));
        }
        let answers = rules::with_all_defaults(&aa, &seed).unwrap();
        let mut ia = rules::apply(&aa, &answers).unwrap();
        let mut ib = rules::apply(&ab, &answers).unwrap();
        ia.binding = json!(null);
        ib.binding = json!(null);
        // Planner quantities/supports are in the part frame; its world placement
        // must follow the known motion rather than compare byte-identically (#75).
        let mut expected_intent = ia.to_dict();
        for collection in ["requirements", "general"] {
            for row in expected_intent[collection].as_array_mut().unwrap() {
                if let Some(frame) = row.get_mut("planner").and_then(|p| p.get_mut("frame")) {
                    let point: [f64; 3] = serde_json::from_value(frame["origin"].clone()).unwrap();
                    frame["origin"] = json!(motion.point(point));
                    for axis in ["x", "y", "z"] {
                        let direction: [f64; 3] =
                            serde_json::from_value(frame[axis].clone()).unwrap();
                        frame[axis] = json!(motion.direction(direction));
                    }
                }
            }
        }
        let differences = common::compare(
            name,
            &expected_intent,
            &ib.to_dict(),
            &common::Tolerances::default().with("*", 1e-5, 1e-8),
        );
        assert!(differences.is_empty(), "{name}: {differences:?}");
        // Use each source's binding for the real PMI writer and its independent readback check.
        let mut written_pmi = Vec::new();
        let mut written_parts = Vec::new();
        let mut readbacks = Vec::new();
        let mut reports = Vec::new();
        for loaded in [&a, &b] {
            let analysis = api::analyse(loaded).unwrap();
            let intent = rules::apply(&analysis, &answers).unwrap();
            let (written, report) = writer::write_bytes(loaded, &intent, Some(&answers)).unwrap();
            reports.push((report.written, report.not_written));
            let back = part::File::of_bytes(Path::new(name), written)
                .unwrap()
                .load(None)
                .unwrap();
            let existing = specify_core::existing::read_existing(&back).unwrap();
            if name.starts_with("corpus/") {
                // Some generic dimensions on 10224 have no semantic writer mapping.
                // The explicit write/refusal report must also be invariant under motion.
                assert!(!existing.is_empty(), "{name}");
            } else {
                assert_eq!(
                    existing.len(),
                    intent
                        .requirements
                        .iter()
                        .filter(|r| !r.contains_key("not_written_reason"))
                        .count()
                        + intent.datums.len(),
                    "{name}"
                );
            }
            readbacks.push(
                serde_json::to_value(specify_core::existing::read_existing(&back).unwrap())
                    .unwrap(),
            );
            let doc = Document::parse(back.bytes.to_vec()).unwrap();
            written_pmi.push(
                haecceity::pmi::read(&doc, std::slice::from_ref(&back.definition))
                    .unwrap()
                    .parts
                    .remove(0),
            );
            written_parts.push(back);
        }
        assert_eq!(reports[0], reports[1], "{name}: write and refusal reports");
        let mut expected_readback = readbacks[0].clone();
        // #77 sources are keyed in each stored pose's schema-2 face namespace. A rigid
        // move changes that fingerprint while leaving the face supports/intent intact.
        for (readback, loaded) in readbacks.iter().zip(&written_parts) {
            let fingerprint = resume::fingerprint(loaded).unwrap();
            for row in readback.as_array().unwrap() {
                if let Some(source) = row.get("source") {
                    assert_eq!(source["faces"], fingerprint);
                    let target = if row["type"]
                        .as_str()
                        .is_some_and(|s| s.starts_with("Location"))
                    {
                        &row["faces2"]
                    } else {
                        &row["faces"]
                    };
                    assert_eq!(&source["support"], target);
                }
            }
        }
        let moved_fingerprint = resume::fingerprint(&written_parts[1]).unwrap();
        for row in expected_readback.as_array_mut().unwrap() {
            if let Some(source) = row.get_mut("source") {
                source["faces"] = json!(moved_fingerprint);
            }
        }
        if name == "GRM-02_shaft" {
            // For #65: these are independent FIRST writes of two numberless poses.
            // #75 omits the unsupported blind-depth row. The exact Ø2.5 drill
            // now defaults to a thread rather than an H11 size; positions precede
            // the two internal threads. Stored-frame view/position order can change under rotation. Pin both
            // allocations explicitly; every other PMI field still compares below.
            let numbers: Vec<_> = readbacks[0]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["number"].clone())
                .collect();
            assert_eq!(json!(numbers), json!([9, 10, 1, 2, 11, 12]));
            // Rotated Z coordinates separate the end-on holes and side flat that
            // coincide in the original right view. Datums A/B remain first.
            for (row, number) in expected_readback
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .zip([9, 12, 1, 2, 10, 13])
            {
                row["number"] = json!(number);
            }
        }
        if name == "corpus/mfcadpp/10224" {
            // general dimensions are rows (Paul, 2026-11-21). On these first writes,
            // the separately approved measurements shift the four allocations by one.
            // The two poses still receive different stored-frame view positions.
            for (index, before, after) in [(3, 18, 19), (5, 19, 20), (8, 17, 18), (10, 20, 21)] {
                assert_eq!(expected_readback[index]["number"], before);
                expected_readback[index]["number"] = json!(after);
            }
        }
        let differences = common::compare(
            name,
            &expected_readback,
            &readbacks[1],
            &common::Tolerances::default().with("*", 1e-5, 1e-8),
        );
        assert!(
            differences.is_empty(),
            "{name}: written PMI {differences:?}"
        );
        let (base_pmi, moved_pmi) = (&written_pmi[0], &written_pmi[1]);
        assert_eq!(
            base_pmi.features, moved_pmi.features,
            "{name}: PMI source-face attachments"
        );
        assert_eq!(
            base_pmi.datums, moved_pmi.datums,
            "{name}: written datum associations"
        );
        // This writer places semantic PMI by source-face associations, not supplemental
        // geometry. Inspect the actual geometry those written anchors address in both files.
        for feature in &base_pmi.features {
            if let haecceity::pmi::Feature::Items(anchors) = feature {
                for anchor in anchors {
                    if let haecceity::pmi::Anchor::Face(face) = anchor {
                        placements_checked += 1;
                        let a = &written_parts[0].part;
                        let b = &written_parts[1].part;
                        close(
                            &motion.point(a.face_moments(face.0).unwrap().centroid),
                            &b.face_moments(face.0).unwrap().centroid,
                            1e-5,
                        );
                        close(
                            &motion.direction(a.face_normal(face.0, 0.0, 0.0).unwrap()),
                            &b.face_normal(face.0, 0.0, 0.0).unwrap(),
                            1e-6,
                        );
                    }
                }
            }
        }
        let fa = features::recognise(&a).unwrap();
        let fb = features::recognise(&b).unwrap();
        for hole in fa.iter().filter(|f| f.family == "holes") {
            let other = fb.iter().find(|f| f.id == hole.id).unwrap();
            let vector = |f: &Feature, k: &str| -> [f64; 3] {
                serde_json::from_value(f.record[k].clone()).unwrap()
            };
            close(
                &motion.point(vector(hole, "location")),
                &vector(other, "location"),
                1e-5,
            );
            close(
                &motion.direction(vector(hole, "axis")),
                &vector(other, "axis"),
                1e-6,
            );
        }
    }
    assert!(
        placements_checked > 0,
        "written PMI geometry must be tested, not just item counts"
    );
}

#[test]
fn unresolved_saved_answers_reach_review_and_the_file_answers_the_current_question() {
    let loaded = part::load(&fixture("resume-alpha6/pin.step.gz"), None).unwrap();
    let mut payload = json!(resume::stored(&loaded.bytes)[&0]);
    let key = payload["answers"]
        .as_object()
        .unwrap()
        .keys()
        .find(|id| id.starts_with("hole.function:"))
        .unwrap()
        .clone();
    let value = payload["answers"]
        .as_object_mut()
        .unwrap()
        .remove(&key)
        .unwrap();
    payload["answers"]["hole.function:holes:999"] = value.clone();
    let rewrite = |payload: &Value| {
        let doc = Document::parse(loaded.bytes.as_ref().clone()).unwrap();
        let mut edit = Edit::new();
        for id in doc.ids() {
            if let Some(RawEntity::Simple {
                name, attributes, ..
            }) = doc.get(id)
                && name == "DESCRIPTIVE_REPRESENTATION_ITEM"
                && matches!(attributes.first(), Some(Attribute::String(s)) if s == resume::NAME)
            {
                let mut attributes = attributes.clone();
                attributes[1] = Attribute::String(resume::encode(payload));
                edit.replace(id, haecceity::p21::simple(name, attributes));
            }
        }
        doc.apply(&edit).unwrap().bytes
    };
    let bytes = rewrite(&payload);
    let part = part::File::of_bytes(Path::new("orphan.step"), bytes)
        .unwrap()
        .load(None)
        .unwrap();
    let (analysis, warnings) = api::analyse_reporting(&part).unwrap();
    // ADR P11 (D6): beside the unresolved answer, each saved answer whose PMI the alpha.6 file
    // never wrote is said once, its question open again.
    let (_, reopened) = p11_resumed(
        &carried(&payload["answers"], &analysis),
        &analysis,
        &warnings,
    );
    assert_eq!(warnings.len(), reopened.len() + 1, "{warnings:?}");
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.contains("hole.function:holes:999"))
            .count(),
        1
    );
    assert_eq!(analysis["resume"]["unresolved"][0]["answer"], value);
    assert_eq!(analysis["resume"]["unresolved"][0]["feature"], "holes:999");
    // File-only consumers get applicable answers, while analysis keeps the
    // unsupported answer above for review instead of applying an unknown ID.
    assert!(
        !resume::answers_for(&part)
            .unwrap()
            .unwrap()
            .contains_key("hole.function:holes:999")
    );
    let review = api::review(
        &analysis,
        analysis["resume"]["answers"].as_object().unwrap(),
    )
    .unwrap();
    // The unanswered current question includes the floor; the orphan retains its saved key.
    // ADR P9: the file still states the hole's requirement as its default would, so the
    // question is answered by the file (`resume.present`), not open.
    assert_eq!(key, "hole.function:holes:3");
    // ADR P11: `resume.present` is gone; the feature table's row says the file states it.
    assert!(
        analysis["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "hole.function:holes:3.5" && r["state"] == "stated"),
        "{}",
        analysis["table"]
    );
    assert!(
        !review["open"]
            .as_array()
            .unwrap()
            .contains(&json!("hole.function:holes:3.5"))
    );
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["code"] == "saved_answer_unresolved"
                && p["message"].as_str().unwrap().contains("holes:999"))
    );
    // Edited geometry opens as a foreign file, retaining written PMI and part settings.
    // Migration is restricted to the original geometry, for both independent checks.
    let original = payload.clone();
    for field in ["faces", "face_count"] {
        payload = original.clone();
        payload[field] = if field == "faces" {
            json!("unproved")
        } else {
            json!(999)
        };
        let part = part::File::of_bytes(Path::new("changed.step"), rewrite(&payload))
            .unwrap()
            .load(None)
            .unwrap();
        let (analysis, warnings) = api::analyse_reporting(&part).unwrap();
        assert!(analysis.get("resume").is_none(), "{field}");
        assert!(warnings.is_empty(), "{field}: {warnings:?}");
        assert!(
            !analysis["existing"].as_array().unwrap().is_empty(),
            "{field}"
        );
        assert_eq!(analysis["part"]["material"], "steel", "{field}");
        assert_eq!(
            analysis["part"]["notes"], original["answers"]["part.notes"],
            "{field}"
        );
    }
}

#[test]
fn real_alpha6_ctc02_answers_exercise_changed_ids_and_explicit_refusals() {
    let base: Value =
        serde_json::from_slice(&std::fs::read(fixture("resume-alpha6/ctc02-reid.json")).unwrap())
            .unwrap();
    assert_eq!(base["answers"].as_object().unwrap().len(), 34);
    let loaded = part::load(&fixture("resume-alpha6/ctc02-reid.step.gz"), Some(0)).unwrap();
    let (analysis, warnings) = api::analyse_reporting(&loaded).unwrap();
    let resumed = &analysis["resume"];

    // ADR P11 (D6): of the 32 resolved answers, those the file states or lacks are not
    // resumed; each one it lacks is said once.
    let resolved = carried(&base["answers"], &analysis);
    assert_eq!(resolved.len(), 32);
    let (_, reopened) = p11_resumed(&resolved, &analysis, &warnings);
    // #100 adds the same holes' tip cones: 13 exact answer moves, two #82 refusals remain.
    common::upgrade::check(&loaded, &base, &analysis, common::upgrade::CTC02);
    assert_eq!(resumed["unresolved"].as_array().unwrap().len(), 2);
    assert_eq!(warnings.len(), reopened.len() + 2, "{warnings:?}");
    for unresolved in resumed["unresolved"].as_array().unwrap() {
        assert!(
            unresolved["id"]
                .as_str()
                .unwrap()
                .contains("hole_patterns.linear:462.463.466.467.502.503")
        );
        assert_eq!(
            unresolved["answer"],
            base["answers"][unresolved["id"].as_str().unwrap()]
        );
        assert!(
            unresolved["reason"]
                .as_str()
                .unwrap()
                .contains("no same-family")
        );
    }
}

#[test]
fn real_alpha6_ftc10_answers_migrate_to_larger_geometric_supports() {
    let base: Value =
        serde_json::from_slice(&std::fs::read(fixture("resume-alpha6/ftc10-reid.json")).unwrap())
            .unwrap();
    assert_eq!(base["answers"].as_object().unwrap().len(), 29);
    let loaded = part::load(&fixture("resume-alpha6/ftc10-reid.step.gz"), Some(0)).unwrap();
    let (analysis, warnings) = api::analyse_reporting(&loaded).unwrap();
    let resumed = &analysis["resume"];
    // ADR P11 (D6): all 29 resolve; those the file states or lacks are not resumed, and each
    // one it lacks is said once.
    let resolved = carried(&base["answers"], &analysis);
    assert_eq!(resolved.len(), 29);
    let (_, reopened) = p11_resumed(&resolved, &analysis, &warnings);
    assert_eq!(warnings.len(), reopened.len(), "{warnings:?}");
    // #100 adds tip cones to two bores and one row: six new moves, plus the two
    // schema-6 counterbore-pattern moves. Exact IDs and cone adjacency are checked.
    common::upgrade::check(&loaded, &base, &analysis, common::upgrade::FTC10);
    assert!(resumed["unresolved"].is_null());
    let review = api::review(&analysis, resumed["answers"].as_object().unwrap()).unwrap();
    // ADR P11: the file's PMI on the 194 pattern states its two questions (whoever wrote it),
    // so they are not open. The reopened questions (D6) and newly offered
    // standalone measurements still need a decision.
    for id in [
        "hole.function:hole_patterns.linear:194.195.196.197.198.199",
        "hole.position:hole_patterns.linear:194.195.196.197.198.199",
    ] {
        assert!(
            analysis["table"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == id && r["state"] == "stated"),
            "{id}"
        );
    }
    assert_eq!(
        review["open"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().to_string())
            .collect::<BTreeSet<_>>(),
        reopened
            .into_iter()
            .chain(open_measurements(&analysis))
            .collect()
    );
    for pair in resumed["migrated"].as_array().unwrap() {
        assert!(!review["open"].as_array().unwrap().contains(&pair[1]));
    }
}

/// Sidecars/projects and legacy payloads lack face evidence: only an exact ID or
/// a unique same-family strict superset of the ID faces may carry an answer.
#[test]
fn id_only_answers_require_a_unique_same_family_strict_id_face_superset() {
    let loaded = part::load(&fixture("parts/plate.step"), None).unwrap();
    let answers = json!({"hole.function:holes:6":"tapped:M2", "part.material":"steel"});
    let run = |features: Vec<Feature>| {
        resume::rematch_answers(&loaded, &features, answers.as_object().unwrap(), None, None)
            .unwrap()
    };
    let unique = run(vec![feature("holes:6.7", "holes", vec![6, 7])]);
    assert_eq!(
        unique.answers,
        json!({"hole.function:holes:6.7":"tapped:M2", "part.material":"steel"})
            .as_object()
            .unwrap()
            .clone()
    );
    assert_eq!(
        unique.migrated,
        [(
            "hole.function:holes:6".into(),
            "hole.function:holes:6.7".into()
        )]
    );
    assert!(unique.unresolved.is_empty());
    assert!(unique.unverified.is_some());
    for features in [
        vec![
            feature("holes:6.7", "holes", vec![6, 7]),
            feature("holes:6.8", "holes", vec![6, 8]),
        ],
        vec![feature("bosses:6.7", "bosses", vec![6, 7])],
        // Record faces alone do not establish an ID-face superset.
        vec![feature("holes:7", "holes", vec![6, 7])],
    ] {
        let refused = run(features);
        assert_eq!(
            refused.answers,
            json!({"part.material":"steel"})
                .as_object()
                .unwrap()
                .clone()
        );
        assert!(refused.migrated.is_empty());
        assert_eq!(refused.unresolved.len(), 1);
        assert_eq!(refused.unresolved[0].id, "hole.function:holes:6");
        assert_eq!(refused.unresolved[0].answer, "tapped:M2");
    }
    let exact = run(vec![
        feature("holes:6", "holes", vec![6]),
        feature("holes:6.7", "holes", vec![6, 7]),
    ]);
    assert_eq!(exact.answers, *answers.as_object().unwrap());
    assert!(exact.migrated.is_empty());
    assert!(exact.unresolved.is_empty());
}
