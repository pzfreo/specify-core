//! For #79: exercise question/completeness/intent/write through the actual public APIs and CLI.
mod common;
use serde_json::{Map, Value, json};
use specify_core::{api, part::File, rules};
use std::{path::Path, process::Command};
#[test]
fn no_opt_in_matches_v046_presentation_through_api_and_cli() {
    let dir = std::env::temp_dir().join(format!("bom-opt-in-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let complete: Vec<Map<String, Value>> =
        serde_json::from_str(include_str!("fixtures/bom/legacy-assembly-answers.json")).unwrap();
    let plate_complete: Map<String, Value> =
        serde_json::from_str(include_str!("fixtures/bom/legacy-plate-answers.json")).unwrap();
    // ADR P11: the stored answers are the storable ones only, which changed every whole-file
    // hash pinned here; output hashes are not pinned (they moved with every presentation
    // change). What no opt-in means is asserted directly: no BOM record is written, and the
    // API and CLI write the same bytes.
    for (fixture, all_answered, defaults) in [
        ("plate", false, true),
        ("assembly", true, false),
        ("assembly", true, true),
        ("assembly", false, true),
        ("plate", true, false),
        ("plate", true, true),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/parts/{fixture}.step"));
        let file = File::read(&path).unwrap();
        let mut jobs = Vec::new();
        let mut cli_jobs = Vec::new();
        let output = dir.join(format!("{fixture}-{all_answered}-{defaults}.step"));
        let mut pairs = Vec::new();
        for part in file.load_all().unwrap() {
            let analysis = api::analyse_reporting_in(&file, &part).unwrap().0;
            let answers = if all_answered {
                if fixture == "plate" {
                    plate_complete.clone()
                } else {
                    complete[part.binding.part].clone()
                }
            } else {
                json!({"part.material":"6061-T6"})
                    .as_object()
                    .unwrap()
                    .clone()
            };
            // The frozen fixtures are saved plain answer files, with no face evidence.
            // Import them through the public ID matcher before the strict current-ID
            // question/apply APIs. Keep their old keys to exercise floor-ID migration.
            let mut answers = if all_answered {
                let imported = specify_core::resume::rematch_answers(
                    &part,
                    &specify_core::features::recognise(&part).unwrap(),
                    &answers,
                    None,
                    None,
                )
                .unwrap();
                assert!(imported.unresolved.is_empty(), "{:?}", imported.unresolved);
                assert_eq!(imported.answers.len(), answers.len());
                for (old, new) in &imported.migrated {
                    assert_eq!(new, &format!("{}.5", old));
                    assert!(old.ends_with(":holes:3"));
                    assert_eq!(imported.answers[new], answers[old]);
                }
                imported.answers
            } else {
                answers
            };
            if all_answered {
                // The legacy fixture predates separately approved measurements. Take
                // exactly the new gaps so this case still exercises a complete CLI write
                // without opt-in to general defaults or BOM.
                let questions = api::questions(&analysis, &answers).unwrap();
                for q in rules::open_questions(&questions, &answers) {
                    assert!(
                        q.id.starts_with("length:planes:") || q.id.starts_with("location:holes:"),
                        "{}",
                        q.id
                    );
                    answers.insert(q.id.clone(), q.default.clone().unwrap());
                }
                assert!(
                    rules::open_questions(&api::questions(&analysis, &answers).unwrap(), &answers)
                        .is_empty()
                );
            }
            let merged = rules::with_all_defaults(&analysis, &answers).unwrap();
            assert!(merged.keys().all(|k| !k.starts_with("bom.")));
            let intent = api::apply(&analysis, &answers, defaults).unwrap();
            assert!(!intent.part.contains_key("bom"));
            let bpath = dir.join(format!("{fixture}-{}.edits.json", part.binding.part));
            let edits = if defaults {
                common::approve_edits(&analysis, &answers)
            } else {
                json!({"fill": answers})
            };
            std::fs::write(&bpath, edits.to_string()).unwrap();
            cli_jobs.push(edits);
            pairs.push(bpath);
            jobs.push((intent, Some(answers)));
        }
        let bytes = api::write_bytes(file.bytes().as_ref().clone(), &jobs)
            .unwrap()
            .0;
        assert!(
            !String::from_utf8_lossy(&bytes).contains(&format!("'{}", specify_core::bom::PROPERTY)),
            "API {fixture}, complete={all_answered}, defaults={defaults}"
        );
        let mut input = path;
        for (k, (edits, list)) in pairs.iter().zip(cli_jobs).enumerate() {
            let mut command = Command::new(env!("CARGO_BIN_EXE_specify-core"));
            command.arg("write").arg(&input).arg(edits);
            if fixture == "assembly" {
                command.arg("--part").arg(k.to_string());
            }
            let selected = (fixture == "assembly").then_some(k);
            let loaded = specify_core::part::load(&input, selected).unwrap();
            let list = specify_core::edits::parse_value(list).unwrap();
            let expected = specify_core::writer::apply::apply(&loaded, &list).unwrap();
            let next = output.with_extension(format!("{k}.step"));
            let result = command.arg("-o").arg(&next).output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let actual = std::fs::read(&next).unwrap();
            assert_eq!(actual, expected.bytes);
            assert!(
                !String::from_utf8_lossy(&actual)
                    .contains(&format!("'{}", specify_core::bom::PROPERTY))
            );
            input = next;
        }
    }
}
