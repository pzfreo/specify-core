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
            let answers = if all_answered {
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
                assert!(
                    rules::open_questions(&api::questions(&analysis, &answers).unwrap(), &answers)
                        .is_empty()
                );
            }
            let merged = rules::with_all_defaults(&analysis, &answers).unwrap();
            assert!(merged.keys().all(|k| !k.starts_with("bom.")));
            let intent = api::apply(&analysis, &answers, defaults).unwrap();
            assert!(!intent.part.contains_key("bom"));
            let apath = dir.join(format!("{fixture}-{}.analysis.json", part.binding.part));
            let bpath = dir.join(format!("{fixture}-{}.answers.json", part.binding.part));
            std::fs::write(&apath, analysis.to_string()).unwrap();
            std::fs::write(&bpath, json!(answers).to_string()).unwrap();
            pairs.push((apath, bpath));
            // With every default taken, the command line also stores those that write no PMI
            // (ADR P9): the same PMI, more stored answers.
            let stored = if defaults {
                let stored = api::stored_with_defaults(&part, &analysis, &answers).unwrap();
                // ADR P11: the writer stores only the answers that leave no PMI, so those are
                // what must agree.
                let storable = specify_core::feature_table::storable;
                assert_eq!(
                    storable(&stored),
                    storable(&common::accept_all::stored(
                        &path,
                        part.binding.part,
                        &analysis,
                        &answers
                    )),
                    "{fixture} part {}",
                    part.binding.part
                );
                stored
            } else {
                answers.clone()
            };
            cli_jobs.push((intent.clone(), Some(stored)));
            jobs.push((intent, Some(answers)));
        }
        let bytes = api::write_bytes(file.bytes().as_ref().clone(), &jobs)
            .unwrap()
            .0;
        assert!(
            !String::from_utf8_lossy(&bytes).contains(&format!("'{}", specify_core::bom::PROPERTY)),
            "API {fixture}, complete={all_answered}, defaults={defaults}"
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_specify-core"));
        command.arg("write").arg(&path);
        for (a, b) in pairs {
            command.arg(a).arg(b);
        }
        command.arg("-o").arg(&output);
        if defaults {
            command.arg("--accept-defaults");
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let cli_bytes = api::write_bytes(file.bytes().as_ref().clone(), &cli_jobs)
            .unwrap()
            .0;
        assert_eq!(std::fs::read(output).unwrap(), cli_bytes);
    }
}
