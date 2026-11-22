//! The feature table (Specify 0.3.0, ADR P11): read from the file alone, per feature and kind.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::part::{LoadedPart, load, load_bytes};
use specify_core::resume;

fn map(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn plate() -> LoadedPart {
    load(&common::fixtures().join("parts/plate.step"), None).unwrap()
}

fn fixture(name: &str) -> LoadedPart {
    load(&common::fixtures().join(name), None).unwrap()
}

fn table(loaded: &LoadedPart) -> (Value, Vec<String>) {
    let (analysis, warnings) = api::analyse_reporting(loaded).unwrap();
    (analysis["table"].clone(), warnings)
}

fn row<'a>(table: &'a Value, id: &str) -> &'a Value {
    table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no row {id} in {table}"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "specify-feature-table-{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cli_write(
    part: &LoadedPart,
    analysis: &Value,
    answers: &Map<String, Value>,
    output: &Path,
    defaults: bool,
) -> Value {
    let result = cli_write_result(part, analysis, answers, output, defaults);
    assert!(
        result.status.success(),
        "{}: {}",
        part.path.display(),
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

fn cli_write_result(
    part: &LoadedPart,
    analysis: &Value,
    answers: &Map<String, Value>,
    output: &Path,
    defaults: bool,
) -> std::process::Output {
    let suggestions: Vec<_> = analysis["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        // The table also carries dormant follow-ups. Accept the interview's open
        // rows and its optional none decisions, as all-defaults did.
        .filter(|r| {
            defaults
                && r["state"] == "gap"
                && !r["suggestion"].is_null()
                && (r["open"] == true
                    || specify_core::feature_table::is_none_value(&r["suggestion"]))
        })
        .filter_map(|r| r["id"].as_str())
        .filter(|id| !answers.contains_key(*id))
        .collect();
    let edits = output.with_extension("edits.json");
    std::fs::write(
        &edits,
        json!({"fill":answers, "accept_suggestions":suggestions}).to_string(),
    )
    .unwrap();
    let mut cli = Command::new(env!("CARGO_BIN_EXE_specify-core"));
    cli.arg("write")
        .arg(&part.path)
        .arg(edits)
        .arg("-o")
        .arg(output);
    if part.part_count > 1 {
        cli.arg("--part").arg(part.binding.part.to_string());
    }
    cli.output().unwrap()
}

fn assert_no_open(table: &Value, case: &str) {
    let open: Vec<_> = table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["open"] == true)
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert!(open.is_empty(), "{case} reopened with open rows: {open:?}");
}

fn assert_only_intentional_no_pmi(part: &LoadedPart, analysis: &Value, case: &str) {
    let payloads = resume::stored(&part.bytes);
    if let Some(payload) = payloads.get(&part.binding.part) {
        for id in payload
            .get("no_pmi")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let id = id.as_str().unwrap();
            let answer = &payload["answers"][id];
            let row = row(&analysis["table"], id);
            let datum_owns_size =
                matches!(row["kind"].as_str(), Some("diameter.fit" | "hole.function"))
                    && answer == "general"
                    && analysis["existing"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|item| item["kind"] == "datum" && item["faces"] == row["faces"]);
            assert!(
                datum_owns_size,
                "{case} stored PMI-bearing approval {id} = {answer} under no_pmi"
            );
        }
    }
}

#[test]
fn ctc02_location_approval_is_not_stored_as_no_pmi() {
    let part = fixture("nist/nist_ctc_02_asme1_ap242.stp.gz");
    let analysis = api::analyse(&part).unwrap();
    let scratch = scratch("ctc02-location");
    let output = scratch.join("ctc02.step");
    cli_write(
        &part,
        &analysis,
        &map(json!({"part.material":"6061-T6"})),
        &output,
        true,
    );
    let reopened = load(&output, None).unwrap();
    let after = api::analyse(&reopened).unwrap();
    assert_no_open(&after["table"], "CTC-02");
    assert_eq!(
        row(&after["table"], "length:planes:1-151")["state"],
        "stated"
    );
    assert_only_intentional_no_pmi(&reopened, &after, "CTC-02");
    let existing = after["existing"].as_array().unwrap();
    assert!(
        existing.iter().any(|item| {
            item["type"] == "Location_LinearDistance"
                && item["value"] == 185.0
                && item["origin"] == "general_tolerance"
                && item["faces"] == json!([1])
                && [165, 166, 185, 186].iter().all(|f| {
                    item["faces2"]
                        .as_array()
                        .is_some_and(|faces| faces.contains(&json!(f)))
                })
                && [101, 102, 303, 304]
                    .iter()
                    .all(|f| !item["faces2"].as_array().unwrap().contains(&json!(f)))
        }),
        "CTC-02: 185 must locate the unpositioned holes from datum D"
    );
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn ctc03_reopen_does_not_ask_for_positions_the_file_locates() {
    let part = fixture("nist/nist_ctc_03_asme1_ap242.stp.gz");
    let analysis = api::analyse(&part).unwrap();
    let scratch = scratch("ctc03-location");
    let output = scratch.join("ctc03.step");
    cli_write(
        &part,
        &analysis,
        &map(json!({"part.material":"6061-T6"})),
        &output,
        true,
    );
    let reopened = load(&output, None).unwrap();
    let after = api::analyse(&reopened).unwrap();
    assert_no_open(&after["table"], "CTC-03");
    let table = &after["table"];
    for (id, text) in [
        ("location:holes:18.19-16.17-0", "⌖ ⌀0.05Ⓜ A BⓂ CⓂ"),
        ("location:holes:16.17-11.12-1", "datum B"),
    ] {
        let row = row(table, id);
        assert_eq!(row["state"], "stated", "{row}");
        assert_eq!(row["value"], text, "{row}");
    }
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn ctc04_writes_the_position_and_shared_locations_for_unpositioned_holes() {
    let part = fixture("nist/nist_ctc_04_asme1_ap242.stp.gz");
    let analysis = api::analyse(&part).unwrap();
    let scratch = scratch("ctc04");
    let output = scratch.join("ctc04.step");
    let pattern = "hole_patterns.linear:11.12.13.14.15.16.17.18.19.20";
    let location = "location:holes:316.330.372-25.26";
    assert_eq!(row(&analysis["table"], location)["open"], true);
    for kind in ["hole.frame", "hole.position"] {
        assert_eq!(
            row(&analysis["table"], &format!("{kind}:{pattern}"))["open"],
            true
        );
    }
    cli_write(
        &part,
        &analysis,
        &map(json!({"part.material":"6061-T6"})),
        &output,
        true,
    );
    let reopened = load(&output, None).unwrap();
    let after = api::analyse(&reopened).unwrap();
    assert_only_intentional_no_pmi(&reopened, &after, "CTC-04");
    assert_no_open(&after["table"], "CTC-04");
    for kind in ["hole.frame", "hole.position"] {
        let r = row(&after["table"], &format!("{kind}:{pattern}"));
        assert_eq!(r["state"], "stated", "{r}");
        assert!(
            r["requirements"].as_array().unwrap().iter().any(|item| {
                item["type"] == "Position"
                    && item["text"] == "⌖ ⌀0.75 A B C"
                    && item["faces"] == json!([11, 12, 13, 14, 15, 16, 17, 18, 19, 20])
            }),
            "{r}"
        );
    }
    for (value, faces) in [
        (5.0, json!([5, 6])),
        (115.0, json!([25, 26, 29, 30])),
        (
            66.25,
            json!([92, 93, 94, 113, 114, 115, 136, 137, 138, 139, 140, 141]),
        ),
    ] {
        assert!(
            after["existing"].as_array().unwrap().iter().any(|item| {
                item["type"] == "Location_LinearDistance"
                    && item["value"] == value
                    && item["origin"] == "general_tolerance"
                    && item["faces"] == json!([316, 330, 372])
                    && item["faces2"] == faces
            }),
            "CTC-04: missing {value} from faces 316,330,372 to {faces}"
        );
    }
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn ctc04_one_pattern_does_not_close_the_position_of_its_identical_neighbour() {
    let part = fixture("nist/nist_ctc_04_asme1_ap242.stp.gz");
    let id = "hole.position:hole_patterns.linear:11.12.13.14.15.16.17.18.19.20";
    let (before, _) = table(&part);
    let first: Vec<_> = (11..=20).collect();
    let second: Vec<_> = (49..=58).collect();
    let both: Vec<_> = first.iter().chain(&second).copied().collect();
    assert_eq!(row(&before, id)["faces"], json!(both));
    assert_eq!(row(&before, id)["state"], "gap");
    let mut intent = specify_core::rules::Intent::new(serde_json::to_value(&part.binding).unwrap());
    intent.datums = [("A", vec![382]), ("B", vec![247, 248]), ("C", vec![3, 4])]
        .into_iter()
        .map(|(letter, faces)| map(json!({"letter":letter, "faces":faces})))
        .collect();
    intent.requirements.push(map(json!({
        "kind":"position", "feature":"hole_patterns.linear:49.50.51.52.53.54.55.56.57.58",
        "faces":second, "tolerance":0.75, "diametral":true, "datums":["A","B","C"]
    })));
    let (written, _) = specify_core::writer::write_bytes(&part, &intent, None).unwrap();
    let reopened = load_bytes("ctc04-one-position.step", written).unwrap();
    let (after, _) = table(&reopened);
    let r = row(&after, id);
    assert_eq!(r["faces"], json!(both));
    let positions: Vec<_> = r["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "Position")
        .map(|item| item["faces"].clone())
        .collect();
    assert_eq!(positions, vec![json!(second)], "{r}");
    assert_eq!(
        r["state"], "gap",
        "the first pattern is still unpositioned: {r}"
    );
    assert_eq!(r["open"], true, "{r}");

    // This is an intentional partial save. The CLI reports the entire remaining
    // list, including the row for the first, unpositioned pattern.
    let dir = scratch("ctc04-open");
    let input = dir.join("ctc04.step");
    std::fs::write(&input, reopened.bytes.as_slice()).unwrap();
    let part = load(&input, None).unwrap();
    let analysis = api::analyse(&part).unwrap();
    let report = cli_write(
        &part,
        &analysis,
        &Map::new(),
        &dir.join("saved.step"),
        false,
    );
    let missing: Vec<_> = after["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["open"] == true)
        .filter_map(|row| row["id"].as_str())
        .collect();
    assert!(missing.contains(&id));
    assert_eq!(report["missing"], json!(missing), "{report}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn ctc05_pattern_answers_are_stated_after_reopening() {
    // CTC-05 AP242 has the same holes as the corpus AP203 input, with faces 78–89.
    let scratch = scratch("ctc05");
    for (index, part) in [fixture("nist/nist_ctc_05_asme1_ap242.stp.gz")]
        .into_iter()
        .chain(
            common::corpus_dir()
                .map(|root| load(&root.join("nist/nist_ctc_05_asme1_rd.stp"), None).unwrap()),
        )
        .enumerate()
    {
        let (before, _) = table(&part);
        let feature = before["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|r| {
                (r["kind"] == "hole.position"
                    && r["suggestion"] == "0.72"
                    && r["feature"].as_str()?.starts_with("hole_patterns.linear:"))
                .then(|| r["feature"].as_str().unwrap())
            })
            .expect("CTC-05 clearance pattern");
        let function = format!("hole.function:{feature}");
        let position = format!("hole.position:{feature}");
        let mut partial_answers =
            map(json!({function.clone(): "clearance:M10", position.clone(): "0.72"}));
        let analysis = api::analyse(&part).unwrap();
        for q in api::questions(&analysis, &partial_answers).unwrap() {
            if matches!(q.id.as_str(), "datum.A" | "datum.B") {
                partial_answers.insert(q.id, q.default.unwrap());
            }
        }
        let saved = common::apply_edits(&part, json!({"fill": partial_answers}));
        let partial = load_bytes("ctc05.step", saved.bytes).unwrap();
        let (partial_table, _) = table(&partial);
        assert_eq!(row(&partial_table, &function)["state"], "stated");
        assert_eq!(row(&partial_table, &position)["state"], "stated");
        let saved_again = common::apply_edits(&partial, json!({"fill": partial_answers}));
        assert!(
            saved_again.report.written.is_empty(),
            "{:?}",
            saved_again.report
        );
        assert!(saved_again.unchanged);
        assert_eq!(
            table(&load_bytes("ctc05.step", saved_again.bytes).unwrap()).0,
            partial_table
        );
        let analysis = api::analyse(&part).unwrap();
        let answers = map(json!({
            "part.material":"6061-T6", function.clone():"clearance:M10", position.clone():"0.72"
        }));
        let output = scratch.join(format!("{index}.step"));
        cli_write(&part, &analysis, &answers, &output, true);
        let reopened = load(&output, None).unwrap();
        let analysis = api::analyse(&reopened).unwrap();
        let after = &analysis["table"];
        for (id, requirement) in [(&function, "⌀10.7163 H11"), (&position, "⌖ ⌀0.72Ⓜ A B")]
        {
            let r = row(after, id);
            assert_eq!(r["state"], "stated", "{id}: {r}");
            assert!(!r["faces"].as_array().unwrap().is_empty(), "{r}");
            if index == 0 {
                assert_eq!(r["faces"], json!([79, 81, 83, 85, 87, 89]), "{r}");
            }
            assert!(r["value"].as_str().unwrap().contains(requirement), "{r}");
            assert!(
                !after["file_pmi"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["text"] == requirement),
                "{id}"
            );
        }
        assert_no_open(after, "CTC-05");
        let ids = |table: &Value| {
            table["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["id"].as_str().unwrap().to_owned())
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(ids(&before), ids(after), "CTC-05 row IDs on reopen");
        let second = scratch.join(format!("{index}-again.step"));
        let report = cli_write(&reopened, &analysis, &Map::new(), &second, false);
        assert_eq!(report["written"], json!([]), "{report}");
        assert_eq!(table(&load(&second, None).unwrap()).0, *after, "{report}");
    }
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn accepting_corpus_defaults_leaves_no_open_rows() {
    let scratch = scratch("corpus");
    let corpus = common::corpus_dir();
    assert!(
        corpus.is_some()
            || ["SPECIFY_CORPUS", "QUIDDITY_CORPUS"]
                .iter()
                .all(|key| std::env::var_os(key).is_none()),
        "configured corpus directory is missing"
    );
    let mut inputs = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut gaps = Vec::new();
    for (index, case) in common::cases().into_iter().enumerate() {
        if case.kind() == "corpus" && corpus.is_none() {
            continue;
        }
        let input = case
            .input()
            .unwrap_or_else(|| panic!("missing selected case {}", case.name));
        if !inputs.insert(input.clone()) {
            continue;
        }
        // The native loader has one part per product. Python captures also number
        // surface representations separately, so enumerate native parts by file.
        let file =
            specify_core::part::File::read(&input).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let parts = file
            .load_all()
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert!(!parts.is_empty(), "{}: no native parts", case.name);
        for part in parts {
            let analysis = api::analyse(&part).unwrap();
            let mut answers = Map::new();
            if row(&analysis["table"], "part.material")["open"] == true {
                answers.insert("part.material".into(), json!("6061-T6"));
            }
            let part_index = part.binding.part;
            let output = scratch.join(format!("{index}-{part_index}.step"));
            // FTC-09 e1's second native product consists of surface geometry.
            // It has no material volume for the kernel's dimensioning frame.
            if case.name == "nist/nist_ftc_09_asme1_ap242-e1" && part_index == 1 {
                assert_eq!(
                    analysis["dimensioning"]["reason"],
                    "dimensioning frame: no-material"
                );
                let result = cli_write_result(&part, &analysis, &answers, &output, true);
                // The edit-list route reports a refused plan as exit 2.
                assert_eq!(result.status.code(), Some(2), "{result:?}");
                assert!(
                    String::from_utf8_lossy(&result.stderr)
                        .ends_with("dimensioning frame: no-material\n"),
                    "{result:?}"
                );
                assert!(!output.exists());
                continue;
            }
            let report = cli_write(&part, &analysis, &answers, &output, true);
            let reopened = load(&output, Some(part_index)).unwrap();
            let after = api::analyse(&reopened).unwrap();
            assert_only_intentional_no_pmi(&reopened, &after, &case.name);
            let expected_missing: Vec<_> = after["table"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["open"] == true)
                .map(|row| &row["id"])
                .collect();
            if expected_missing.is_empty() {
                assert!(report.get("missing").is_none(), "{}: {report}", case.name);
            } else {
                assert_eq!(
                    report["missing"],
                    json!(expected_missing),
                    "{}: {report}",
                    case.name
                );
            }
            for row in after["table"]["rows"].as_array().unwrap() {
                if row["open"] == true {
                    gaps.push(format!(
                        "{}#{part_index} {}: {}",
                        case.name, row["id"], row["suggestion"]
                    ));
                    eprintln!(
                        "{}#{part_index} {} still open. Report: {report}",
                        case.name, row["id"]
                    );
                }
            }
        }
        visited.insert(case.name);
    }
    for name in [
        "nist/nist_ctc_01_asme1_ap242",
        "nist/nist_ctc_05_asme1_ap242",
    ] {
        assert!(visited.contains(name), "did not visit {name}");
    }
    if corpus.is_some() {
        for name in [
            "corpus/nist/nist_ctc_01_asme1_rd",
            "corpus/nist/nist_ctc_02_asme1_rc",
            "corpus/nist/nist_ctc_04_asme1_rd",
            "corpus/nist/nist_ctc_05_asme1_rd",
            "corpus/gramel/GRM-05_depth_lock_bolt",
        ] {
            assert!(visited.contains(name), "did not visit {name}");
        }
    }
    assert!(
        gaps.is_empty(),
        "corpus rows left open:\n{}",
        gaps.join("\n")
    );
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn enclosure_depth_reads_back_on_the_hole_function_row() {
    let part = fixture("parts/electronics-enclosure-base.step.gz");
    let analysis = api::analyse(&part).unwrap();
    let scratch = scratch("enclosure");
    let output = scratch.join("enclosure.step");
    cli_write(
        &part,
        &analysis,
        &map(json!({"part.material":"6061-T6"})),
        &output,
        true,
    );
    let (table, _) = table(&load(&output, None).unwrap());
    let depth = |r: &Value| r["number"] == "#12" && r["text"] == "8";
    let hole = row(
        &table,
        "hole.function:hole_patterns.rectangle:3.11.24.25.26.28.29.30",
    );
    assert_eq!(hole["state"], "stated", "{hole}");
    assert!(
        hole["requirements"].as_array().unwrap().iter().any(depth),
        "expected enclosure depth #12 on its hole function row: {hole}"
    );
    assert!(!table["file_pmi"].as_array().unwrap().iter().any(depth));
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn ctc05_depths_read_back_on_the_hole_function_rows() {
    let part = fixture("nist/nist_ctc_05_asme1_ap242.stp.gz");
    let analysis = api::analyse(&part).unwrap();
    let scratch = scratch("ctc05-depths");
    let output = scratch.join("ctc05.step");
    cli_write(
        &part,
        &analysis,
        &map(json!({"part.material":"6061-T6"})),
        &output,
        true,
    );
    let (table, _) = table(&load(&output, None).unwrap());
    for (id, value, faces) in [
        (
            "hole.function:hole_patterns.set:29.30.31.32.33.34.35.36",
            "30.48",
            json!([23, 27]),
        ),
        (
            "hole.function:hole_patterns.set:39.41.42.44.45.47.48.50.51.53.54.56.57.59.60.62.63.65.66.68",
            "12.7",
            json!([4, 5, 40, 43, 46, 49, 52, 55, 58, 61, 64, 67]),
        ),
    ] {
        let depth = |r: &Value| {
            r["number"].is_string()
                && r["type"] == "Location_LinearDistance"
                && r["text"] == value
                && r["faces"] == faces
        };
        let hole = row(&table, id);
        assert_eq!(hole["state"], "stated", "{hole}");
        assert!(
            hole["requirements"].as_array().unwrap().iter().any(&depth),
            "missing depth {value} on {hole}"
        );
        assert!(!table["file_pmi"].as_array().unwrap().iter().any(depth));
    }
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn corpus_feature_rows_that_can_write_pmi_have_faces() {
    for case in common::cases() {
        let Some(input) = case.input() else { continue };
        let Ok(part) = load(&input, case.part()) else {
            continue;
        };
        let (table, _) = table(&part);
        for row in table["rows"].as_array().unwrap() {
            // A datum selector acquires its support from the answer. An empty
            // selection or unresolvable imported support cannot write PMI.
            let datum_without_support = row["kind"].as_str().is_some_and(|kind| {
                kind.strip_prefix("datum.")
                    .is_some_and(|letter| letter.len() == 1)
            }) && (row["suggestion"] == json!([])
                || row["value"] == json!([])
                || row["requirements"].as_array().is_some_and(|requirements| {
                    requirements.iter().all(|r| r["faces"] == json!([]))
                }));
            if row["feature"] == "part" {
                assert_eq!(row["scope"], "part", "{}: {row}", case.name);
                assert_eq!(row["faces"], json!([]), "{}: {row}", case.name);
            } else if !datum_without_support {
                assert!(
                    !row["faces"].as_array().unwrap().is_empty(),
                    "{}: {}",
                    case.name,
                    row["id"]
                );
            }
        }
    }
}

/// The rows as (id, state, value), and the file PMI's notation: what the table says,
/// without characteristic numbers.
fn shape(table: &Value) -> (Vec<(String, String, Value)>, Vec<String>) {
    let rows = table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap().to_string(),
                r["state"].as_str().unwrap().to_string(),
                r["value"].clone(),
            )
        })
        .collect();
    let file = table["file_pmi"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["text"].as_str().unwrap().to_string())
        .collect();
    (rows, file)
}

fn write(loaded: &LoadedPart, answers: &Map<String, Value>, stored: bool) -> Vec<u8> {
    let analysis = api::analyse(loaded).unwrap();
    let intent = api::apply(&analysis, answers, true).unwrap();
    api::write_bytes(
        loaded.bytes.to_vec(),
        &[(intent, stored.then(|| answers.clone()))],
    )
    .unwrap()
    .0
}

/// `bytes` with every requirement copied by haecceity's own writer onto the bare plate: the
/// same PMI, written by another tool (no Specify marker, no stored answers, no numbers).
fn as_another_tool(bytes: &[u8]) -> Vec<u8> {
    use haecceity::p21::Document;
    use haecceity::pmi;
    let defs = haecceity::step::read_part_definitions(bytes).unwrap();
    let read = pmi::read(&Document::parse(bytes.to_vec()).unwrap(), &defs).unwrap();
    let mut copied = read.parts[0].clone();
    for t in &mut copied.tolerances {
        t.description = None;
    }
    copied.attributes.retain(|set| {
        !resume::is_answers(set) && set.name != specify_core::characteristics::PROPERTY
    });
    let bare = plate().bytes.to_vec();
    let doc = Document::parse(bare).unwrap();
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

#[test]
fn another_tools_requirement_and_specifys_give_the_same_table() {
    let answers = map(json!({
        "part.material": "6082-T6",
        "hole.position:holes:12": "0.1",
        "datum.A.flatness": "0.05",
    }));
    let specify = write(&plate(), &answers, false);
    let other = as_another_tool(&specify);
    let (mine, _) = table(&load_bytes("plate.step", specify).unwrap());
    let (theirs, _) = table(&load_bytes("plate.step", other).unwrap());
    assert_eq!(shape(&mine), shape(&theirs));
    // The answers' requirements are stated in both, whoever wrote them.
    for t in [&mine, &theirs] {
        for id in ["hole.position:holes:12", "datum.A.flatness", "datum.A"] {
            assert_eq!(row(t, id)["state"], "stated", "{id}: {t}");
        }
        assert!(
            row(t, "hole.position:holes:12")["value"]
                .as_str()
                .unwrap()
                .contains("⌖ ⌀0.1 A B C"),
            "{t}"
        );
    }
}

#[test]
fn a_gap_whose_suggestion_is_none_is_not_open() {
    let (t, _) = table(&plate());
    // Every feature-and-kind of a bare part is a gap.
    assert!(
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["state"] == "gap"),
        "{t}"
    );
    for id in [
        "datum.A.flatness",
        "part.coating",
        "part.heat_treatment",
        "part.notes",
    ] {
        let r = row(&t, id);
        assert_eq!(r["open"], false, "{r}");
    }
    assert_eq!(row(&t, "datum.A.flatness")["suggestion"], "none");
    // A gap with a suggestion that writes a requirement is open, and so is one with none at
    // all (the material).
    assert_eq!(row(&t, "hole.position:holes:12")["open"], true);
    assert_eq!(row(&t, "hole.position:holes:12")["suggestion"], "0.25");
    assert_eq!(row(&t, "part.material")["open"], true);
    assert_eq!(row(&t, "part.material")["suggestion"], Value::Null);
    assert!(t["decided_none"].as_array().unwrap().is_empty());
    assert!(t["file_pmi"].as_array().unwrap().is_empty());
}

#[test]
fn decided_none_is_remembered() {
    let answers = map(json!({
        "part.material": "6082-T6",
        "datum.A.flatness": "none",
        "part.coating": "None (as machined)",
        "hole.position:holes:12": "0.1",
    }));
    let bytes = write(&plate(), &answers, true);
    let (t, _) = table(&load_bytes("plate.step", bytes.clone()).unwrap());
    for id in ["datum.A.flatness", "part.coating"] {
        assert_eq!(row(&t, id)["state"], "decided_none", "{id}: {t}");
        assert_eq!(row(&t, id)["open"], false);
    }
    let decided = t["decided_none"].as_array().unwrap();
    assert!(decided.contains(&json!("datum.A.flatness")), "{t}");
    assert!(decided.contains(&json!("part.coating")), "{t}");
    // Stated requirements are not stored: the file states them.
    let stored = &resume::stored(&bytes)[&0];
    let saved = stored["answers"].as_object().unwrap();
    assert_eq!(saved["datum.A.flatness"], "none");
    assert!(!saved.contains_key("hole.position:holes:12"), "{saved:?}");
    assert!(!saved.contains_key("part.material"), "{saved:?}");
    assert_eq!(row(&t, "hole.position:holes:12")["state"], "stated");
}

#[test]
fn a_0_2_files_repeats_are_ignored_on_read_and_dropped_at_save() {
    // Specify 0.2 stored every answer beside the PMI that states it.
    let old = fixture("revisions/spindle_pmi.old.step.gz");
    let stored = &resume::stored(&old.bytes)[&0]["answers"];
    assert_eq!(stored["hole.position:holes:14"], "0.3", "{stored}");
    let (analysis, warnings) = api::analyse_reporting(&old).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let resumed = analysis["resume"]["answers"].as_object().unwrap();
    // Read back: only what leaves no PMI behind.
    for id in resumed.keys() {
        assert!(
            id == "part.locating" || specify_core::feature_table::is_none_value(&resumed[id]),
            "{id} repeats the file's PMI: {resumed:?}"
        );
    }
    let before = analysis["table"].clone();
    assert_eq!(row(&before, "hole.position:holes:14")["state"], "stated");
    assert!(
        row(&before, "hole.position:holes:14")["value"]
            .as_str()
            .unwrap()
            .contains("⌖ ⌀0.3")
    );
    // A save with the session's answers drops the repeats from the file, and changes no
    // requirement.
    let intent = api::apply(&analysis, resumed, false).unwrap();
    let (saved, _) =
        api::write_bytes(old.bytes.to_vec(), &[(intent, Some(resumed.clone()))]).unwrap();
    let now = &resume::stored(&saved)[&0]["answers"];
    assert_eq!(now, &Value::Object(resumed.clone()));
    let reopened = load_bytes("spindle.step", saved).unwrap();
    let (after, warnings) = table(&reopened);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(shape(&before), shape(&after));
    assert_eq!(before["decided_none"], after["decided_none"]);
    let numbers = |t: &Value| -> Vec<Value> {
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| r["requirements"].as_array().cloned().unwrap_or_default())
            .map(|q| q["number"].clone())
            .collect()
    };
    assert_eq!(numbers(&before), numbers(&after));
}

#[test]
fn a_stored_answer_whose_pmi_was_deleted_is_a_gap_again_mentioned_once() {
    // The edited plate stores flatness 0.05 on datum A; the file no longer has it.
    let edited = fixture("resume-p9/edited-plate.step.gz");
    let (analysis, warnings) = api::analyse_reporting(&edited).unwrap();
    let t = &analysis["table"];
    assert_eq!(row(t, "datum.A.flatness")["state"], "gap");
    let mentions: Vec<_> = warnings
        .iter()
        .filter(|w| w.contains("datum.A.flatness"))
        .collect();
    assert_eq!(mentions.len(), 1, "{warnings:?}");
    // The stored position 0.1 the file changed to 0.2: the file's value is shown, no
    // conflict.
    assert_eq!(row(t, "hole.position:holes:12")["state"], "stated");
    assert!(
        row(t, "hole.position:holes:12")["value"]
            .as_str()
            .unwrap()
            .contains("⌖ ⌀0.2")
    );
    assert!(analysis["resume"].get("conflicts").is_none());
    assert!(analysis["resume"].get("foreign").is_none());
    // The deletion stands: a save (the "none" suggestions taken, as the old interview
    // requires) does not bring it back, and the next opening says nothing.
    let answers = analysis["resume"]["answers"].as_object().unwrap().clone();
    let intent = api::apply(&analysis, &answers, true).unwrap();
    let (saved, _) = api::write_bytes(edited.bytes.to_vec(), &[(intent, Some(answers))]).unwrap();
    let (t, warnings) = table(&load_bytes("plate.step", saved).unwrap());
    assert_eq!(row(&t, "datum.A.flatness")["state"], "gap");
    assert!(
        !warnings.iter().any(|w| w.contains("datum.A.flatness")),
        "{warnings:?}"
    );
}

#[test]
fn pmi_on_no_recognised_feature_is_file_pmi() {
    let edited = fixture("resume-p9/edited-plate.step.gz");
    let (t, _) = table(&edited);
    // general dimensions are rows (Paul, 2026-11-21). The plate's #5 is a stated length.
    let length = row(&t, "length:planes:0-5");
    assert_eq!(length["state"], "stated");
    assert_eq!(length["requirements"][0]["number"], "#5");
    assert_eq!(length["requirements"][0]["type"], "Location_LinearDistance");
    assert_eq!(length["requirements"][0]["faces"], json!([0, 5]));
    assert!(
        t["file_pmi"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["number"] != "#5")
    );

    // An imported surface profile with no recognised owning feature still appears as file PMI.
    let (foreign, _) = table(&fixture("nist/solid-edge-ctc01.step.gz"));
    let profile = |item: &Value| item["type"] == "ProfileOfSurface" && item["faces"] == json!([6]);
    assert!(foreign["file_pmi"].as_array().unwrap().iter().any(profile));
    for r in foreign["rows"].as_array().unwrap() {
        assert!(
            !r["requirements"]
                .as_array()
                .into_iter()
                .flatten()
                .any(profile),
            "{r}"
        );
    }
}

/// The ids `review` asks and counts open, for `analysis` with its resumed answers.
fn asked(analysis: &Value) -> (Vec<String>, Vec<String>) {
    let answers = analysis["resume"]["answers"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let review = api::review(analysis, &answers).unwrap();
    let ids = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|q| q.as_str().or_else(|| q["id"].as_str()).map(str::to_owned))
            .collect()
    };
    (ids(&review["questions"]), ids(&review["open"]))
}

/// Questions and the table agree: nothing the file states is asked, and a feature-and-kind is
/// open in `questions` exactly when the table counts it open (a gap whose suggestion writes
/// nothing is open in neither).
fn agree(analysis: &Value) {
    let (questions, open) = asked(analysis);
    let rows = analysis["table"]["rows"].as_array().unwrap();
    for r in rows {
        let id = r["id"].as_str().unwrap().to_string();
        if r["state"] == "stated" {
            assert!(!questions.contains(&id), "{id} is stated but asked");
        }
        if r["open"] == true {
            assert!(
                open.contains(&id),
                "{id} is open in the table, not in questions"
            );
        }
    }
    let mut rowless = Vec::new();
    for id in &open {
        match rows.iter().find(|r| r["id"] == *id) {
            Some(r) => assert_eq!(
                r["open"], true,
                "{id} is open in questions, not in the table: {r}"
            ),
            None => rowless.push(id.clone()),
        }
    }
    // The open questions with no row are exactly the open choices that are not
    // requirements (today `part.locating` alone, when it is open).
    let choices: Vec<_> = open
        .iter()
        .filter(|id| !specify_core::feature_table::is_requirement(id))
        .cloned()
        .collect();
    assert_eq!(rowless, choices, "open with no table row");
    assert!(
        choices.iter().all(|id| id == "part.locating"),
        "{choices:?}"
    );
}

#[test]
fn questions_and_the_table_agree_on_what_is_open() {
    for name in [
        "parts/plate.step",
        "spindle.step",
        "revisions/enclosure_pmi.old.step.gz",
    ] {
        let part = load(&common::fixtures().join(name), None).unwrap();
        agree(&api::analyse(&part).unwrap());
    }
    // The plate's "none" suggestions are gaps, not open, in both.
    let analysis = api::analyse(&plate()).unwrap();
    let (_, open) = asked(&analysis);
    // Its one open question with no row is the locating choice.
    assert!(open.contains(&"part.locating".to_string()), "{open:?}");
    assert!(
        analysis["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["id"] != "part.locating")
    );
    for id in ["datum.A.flatness", "part.coating", "part.notes"] {
        assert!(!open.contains(&id.to_string()), "{id}");
    }
}

#[test]
fn a_hole_with_its_position_stated_is_still_asked_its_thread() {
    let plate = plate();
    let analysis = api::analyse(&plate).unwrap();
    let answers = map(json!({"part.material": "6082-T6"}));
    let mut intent = specify_core::rules::apply_with_defaults(&analysis, &answers, true).unwrap();
    // Only the hole's position is written: its thread (hole.function) is left out.
    intent
        .requirements
        .retain(|r| r.get("question") != Some(&json!("hole.function:holes:12")));
    let bytes = api::write_bytes(plate.bytes.to_vec(), &[(intent, Some(answers))])
        .unwrap()
        .0;
    let reopened = api::analyse(&load_bytes("plate.step", bytes).unwrap()).unwrap();
    let t = &reopened["table"];
    assert_eq!(row(t, "hole.position:holes:12")["state"], "stated");
    assert_eq!(row(t, "hole.function:holes:12")["state"], "gap");
    let (questions, open) = asked(&reopened);
    assert!(
        questions.contains(&"hole.function:holes:12".to_string()),
        "{questions:?}"
    );
    assert!(
        open.contains(&"hole.function:holes:12".to_string()),
        "{open:?}"
    );
    assert!(!questions.contains(&"hole.position:holes:12".to_string()));
    agree(&reopened);
}

#[test]
fn a_knurl_states_its_diameter_on_the_spindle() {
    let spindle = load(&common::fixtures().join("spindle.step"), None).unwrap();
    let (analysis, warnings) = api::analyse_reporting(&spindle).unwrap();
    let t = &analysis["table"];
    let knurled = row(t, "diameter.fit:turned_steps:9");
    assert_eq!(knurled["state"], "stated", "{knurled}");
    assert!(knurled["value"].as_str().unwrap().contains("knurl"));
    assert!(
        !t["file_pmi"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["type"] == "knurl"),
        "{t}"
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("no requirement for this answer in the file")),
        "{warnings:?}"
    );
    // The thread the file states is not asked; the other hole is.
    let (questions, _) = asked(&analysis);
    assert!(!questions.contains(&"hole.function:holes:3.5".to_string()));
    assert!(questions.contains(&"hole.function:holes:7".to_string()));
    agree(&analysis);
}

#[test]
fn a_general_hole_writes_its_size_so_its_answer_is_not_stored() {
    let plate = plate();
    let analysis = api::analyse(&plate).unwrap();
    let answers = map(json!({"part.material": "6082-T6", "hole.function:holes:9": "general"}));
    let intent = specify_core::rules::apply_with_defaults(&analysis, &answers, true).unwrap();
    let bytes = api::write_bytes(
        plate.bytes.to_vec(),
        &[(intent.clone(), Some(answers.clone()))],
    )
    .unwrap()
    .0;
    let stored = &resume::stored(&bytes)[&0]["answers"];
    assert!(stored.get("hole.function:holes:9").is_none(), "{stored}");
    let (t, _) = table(&load_bytes("plate.step", bytes).unwrap());
    assert_eq!(row(&t, "hole.function:holes:9")["state"], "stated");
    // Its dimension deleted (the file written without it, with the answers the first file
    // stores: not the hole's, which wrote its size): a gap, not decided none.
    let mut without = intent;
    let on_nine = |r: &Map<String, Value>| r.get("faces") == Some(&json!([9]));
    without.general.retain(|r| !on_nine(r));
    without.requirements.retain(|r| !on_nine(r));
    let mut kept = answers;
    kept.shift_remove("hole.function:holes:9");
    let bytes = api::write_bytes(plate.bytes.to_vec(), &[(without, Some(kept))])
        .unwrap()
        .0;
    let (t, _) = table(&load_bytes("plate.step", bytes).unwrap());
    let hole = row(&t, "hole.function:holes:9");
    assert_eq!(hole["state"], "gap", "{hole}");
    assert!(
        !t["decided_none"]
            .as_array()
            .unwrap()
            .contains(&json!("hole.function:holes:9"))
    );
}

#[test]
fn a_direct_intent_measuring_a_stated_feature_otherwise_is_not_added() {
    let p = plate();
    let mut intent = specify_core::rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    intent.datums.push(map(json!({"letter":"A", "faces":[4]})));
    // D8: a location states its own direction only, so the first measures x (from face 0).
    intent.requirements.push(map(
        json!({"kind":"location", "faces":[9], "reference":[0], "distance":50., "basic":true}),
    ));
    let first = api::write_bytes(p.bytes.to_vec(), &[(intent.clone(), None)])
        .unwrap()
        .0;
    let first = load_bytes("plate.step", first).unwrap();
    let locations = |l: &LoadedPart| {
        specify_core::existing::read_existing(l)
            .unwrap()
            .iter()
            .filter(|e| e.r#type.starts_with("Location_"))
            .count()
    };
    assert_eq!(locations(&first), 1);
    // The same feature-and-kind measured from another reference: the file's stands.
    let mut other = specify_core::rules::Intent::new(serde_json::to_value(&first.binding).unwrap());
    // D8: from the other end face 5, in the same direction (x) the file already locates.
    other.requirements.push(map(
        json!({"kind":"location", "faces":[9], "reference":[5], "distance":50., "basic":true}),
    ));
    let (bytes, report) = api::write_bytes(first.bytes.to_vec(), &[(other, None)]).unwrap();
    assert_eq!(locations(&load_bytes("plate.step", bytes).unwrap()), 1);
    assert!(
        report
            .not_written
            .iter()
            .any(|l| l.contains("the file already states it")),
        "{:?}",
        report.not_written
    );
}

/// Position mode, one hole located by a ± location dimension and no position tolerance: the
/// file locates it, so it is stated (Paul, 2026-11-21: a hole is never located twice by two
/// schemes). It is not asked, and a save writes no position tolerance for it.
#[test]
fn a_hole_located_by_a_plus_minus_dimension_is_stated_in_position_mode() {
    let p = plate();
    let mut intent = specify_core::rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    for (letter, faces) in [("A", [2]), ("B", [1]), ("C", [0])] {
        intent
            .datums
            .push(map(json!({"letter": letter, "faces": faces})));
    }
    intent.requirements.push(map(json!({
        "kind": "location", "faces": [12], "reference": [0],
        "distance": 95., "upper": 0.1, "lower": -0.1,
    })));
    // D8: a hole is stated when located in every direction it needs, so y (from B) too.
    intent.requirements.push(map(json!({
        "kind": "location", "faces": [12], "reference": [1],
        "distance": 30., "upper": 0.1, "lower": -0.1,
    })));
    let bytes = api::write_bytes(p.bytes.to_vec(), &[(intent, None)])
        .unwrap()
        .0;
    let located = load_bytes("plate.step", bytes).unwrap();
    let analysis = api::analyse(&located).unwrap();
    let hole = "hole.position:holes:12";
    let r = row(&analysis["table"], hole);
    assert_eq!(r["state"], "stated", "{r}");
    assert!(r["value"].as_str().unwrap().contains("95 ±0.1"), "{r}");
    let (questions, _) = asked(&analysis);
    assert!(!questions.contains(&hole.to_string()), "{questions:?}");
    assert_eq!(analysis["part"].get("locating"), None);
    // A save with every default taken (position tolerances for holes that have nothing).
    let answers = map(json!({"part.material": "6082-T6", "part.locating": "position tolerances"}));
    let save = api::apply(&analysis, &answers, true).unwrap();
    assert!(
        !save
            .requirements
            .iter()
            .any(|r| r["kind"] == "position" && r["faces"] == json!([12])),
        "{:?}",
        save.requirements
    );
    let (saved, report) =
        api::write_bytes(located.bytes.to_vec(), &[(save, Some(answers))]).unwrap();
    let positions =
        specify_core::existing::read_existing(&load_bytes("plate.step", saved).unwrap())
            .unwrap()
            .into_iter()
            .filter(|e| e.r#type == "Position" && e.faces == [12])
            .count();
    assert_eq!(positions, 0, "{:?}", report.written);
}

/// A blind hole's depth, a location dimension to its floor, does not locate the hole: its
/// position row stays a gap (the hole's faces are its bore; the floor is not one of them).
#[test]
fn a_depth_dimension_to_a_hole_floor_does_not_locate_the_hole() {
    let shaft = load(&common::fixtures().join("parts/GRM-02_shaft.step"), None).unwrap();
    let mut intent =
        specify_core::rules::Intent::new(serde_json::to_value(&shaft.binding).unwrap());
    intent
        .datums
        .push(map(json!({"letter": "A", "faces": [0]})));
    // Hole 15's floor is face 16: a 3.5 mm depth from the end face 7.
    intent.requirements.push(map(json!({
        "kind": "location", "faces": [16], "reference": [7], "distance": 3.5,
        "upper": 0.1, "lower": -0.1,
    })));
    let bytes = api::write_bytes(shaft.bytes.to_vec(), &[(intent, None)])
        .unwrap()
        .0;
    let written = load_bytes("shaft.step", bytes).unwrap();
    assert!(
        specify_core::existing::read_existing(&written)
            .unwrap()
            .iter()
            .any(|e| e.r#type.starts_with("Location_")
                && (e.faces.contains(&16) || e.faces2.contains(&16))),
        "the depth is written"
    );
    let (t, _) = table(&written);
    assert_eq!(row(&t, "hole.position:holes:15.16")["state"], "gap");
}

#[test]
fn a_size_on_a_hole_floor_is_not_a_bore_size() {
    let shaft = fixture("parts/GRM-02_shaft.step");
    let mut intent = specify_core::rules::Intent::new(json!(shaft.binding));
    intent.requirements.push(map(json!({
        "kind":"size", "size_kind":"size", "faces":[16], "nominal":3.5, "general":true
    })));
    let (bytes, _) = api::write_bytes(shaft.bytes.to_vec(), &[(intent, None)]).unwrap();
    let saved = load_bytes("shaft.step", bytes).unwrap();
    let (table, _) = table(&saved);
    let hole = row(&table, "hole.function:holes:15.16");
    assert_eq!(hole["state"], "gap", "{hole}");
    assert!(
        table["file_pmi"].as_array().unwrap().iter().any(|r| {
            r["type"].as_str().unwrap().starts_with("Size_") && r["faces"] == json!([16])
        }),
        "{table}"
    );
}

/// A file whose datums the rules cannot take as answers still analyses (a table problem never
/// fails analyse): NIST FTC-09 part 0's datum E is four faces that form no one datum feature.
#[test]
fn nist_ftc09_part_0_analyses_and_lists_its_pmi() {
    let path = common::fixtures().join("nist/nist_ftc_09_asme1_ap242-e1.stp.gz");
    let analysis = api::analyse(&load(&path, Some(0)).unwrap()).unwrap();
    let existing = analysis["existing"].as_array().unwrap();
    assert!(!existing.is_empty());
    let table = &analysis["table"];
    let listed =
        table["rows"].as_array().unwrap().len() + table["file_pmi"].as_array().unwrap().len();
    assert!(listed > 0, "{table}");
    agree(&analysis);
}

/// NIST CTC-02 (its datums form questions the rules refused before) analyses too.
#[test]
fn nist_ctc02_analyses_and_lists_its_pmi() {
    let path = common::fixtures().join("nist/nist_ctc_02_asme1_ap242.stp.gz");
    let analysis = api::analyse(&load(&path, None).unwrap()).unwrap();
    assert!(!analysis["existing"].as_array().unwrap().is_empty());
    agree(&analysis);
}

/// P9 / D1 (Paul): a general-tolerance answer writes only its note. Answering the material
/// and "ISO 2768-m" and saving as the editor does (the preview, plus the unasked rows) adds no
/// dimension; every hole's size and location stays a gap with its suggestion, to be approved
/// one by one or with "accept suggestions".
fn the_general_tolerance_writes_only_its_note(loaded: &LoadedPart, name: &str) {
    let analysis = api::analyse(loaded).unwrap();
    // Every dimension in the file, those written under the general tolerance included.
    let dimensions = |l: &LoadedPart| {
        specify_core::existing::read(l)
            .unwrap()
            .model()
            .dimensions
            .len()
    };
    let before = dimensions(loaded);
    let answers = map(json!({"part.material": "6082-T6", "part.general_tolerance": "ISO 2768-m"}));
    let mut intent = api::preview(&analysis, &answers).unwrap();
    intent
        .requirements
        .extend(specify_core::rules::unasked_rows(&analysis, &answers).unwrap());
    let (bytes, report) =
        specify_core::writer::write_bytes(loaded, &intent, Some(&answers)).unwrap();
    let saved = load_bytes(name, bytes).unwrap();
    assert_eq!(dimensions(&saved), before, "{:?}", report.written);
    let reopened = api::analyse(&saved).unwrap();
    assert_eq!(reopened["part"]["general_tolerance"], "ISO 2768-m");
    let rows: Vec<&Value> = reopened["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| {
            [
                "hole.position:",
                "hole.location:",
                "hole.function:",
                "diameter.fit:",
            ]
            .iter()
            .any(|p| r["id"].as_str().unwrap().starts_with(p))
        })
        .collect();
    assert!(
        rows.iter()
            .any(|r| r["id"].as_str().unwrap().starts_with("hole.function:")),
        "no hole size rows: {rows:?}"
    );
    // Every size and location row the save could have stated is a gap with its suggestion,
    // unless the file stated it before the save.
    let (old, _) = table(loaded);
    for r in rows {
        let id = r["id"].as_str().unwrap();
        if old["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["id"] == id && o["state"] == "stated")
        {
            continue;
        }
        assert_eq!(r["state"], "gap", "{r}");
        assert!(r["suggestion"].is_string(), "{r}");
    }
}

#[test]
fn a_general_tolerance_answer_writes_only_its_note_on_the_plate() {
    the_general_tolerance_writes_only_its_note(&plate(), "plate.step");
}

#[test]
fn a_general_tolerance_answer_writes_only_its_note_on_the_alpha7_part() {
    the_general_tolerance_writes_only_its_note(
        &fixture("thread-depth-alpha7.step.gz"),
        "alpha7.step",
    );
}

// --- D8: one location dimension locates a whole line, per direction ------------------------

/// The plate with `datums` and the location `dimensions` (located faces, reference faces,
/// distance) written by another tool's intent, its locating choice stored as `locating`.
fn located_plate(
    datums: &[(&str, &[u64])],
    dimensions: &[(&[u64], &[u64], f64)],
    locating: &str,
) -> LoadedPart {
    let p = plate();
    let mut intent = specify_core::rules::Intent::new(serde_json::to_value(&p.binding).unwrap());
    for (letter, faces) in datums {
        intent
            .datums
            .push(map(json!({"letter": letter, "faces": faces})));
    }
    for (faces, reference, distance) in dimensions {
        intent.requirements.push(map(json!({
            "kind": "location", "faces": faces, "reference": reference,
            "distance": distance, "upper": 0.1, "lower": -0.1,
        })));
    }
    let answers = map(json!({"part.locating": locating}));
    let bytes = api::write_bytes(p.bytes.to_vec(), &[(intent, Some(answers))])
        .unwrap()
        .0;
    load_bytes("plate.step", bytes).unwrap()
}

const ABC: [(&str, &[u64]); 3] = [("A", &[2]), ("B", &[1]), ("C", &[0])];
const PATTERN: &str = "hole_patterns.grid:6.7.8.10.11.13";

/// The location dimensions `loaded` states (those under the general tolerance too), as
/// (located faces, reference faces).
fn locations(loaded: &LoadedPart) -> Vec<(Vec<usize>, Vec<usize>)> {
    let ids = |v: &Value| -> Vec<usize> {
        v.as_array()
            .into_iter()
            .flatten()
            .map(|f| f.as_u64().unwrap() as usize)
            .collect()
    };
    api::analyse(loaded).unwrap()["existing"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["type"].as_str().unwrap().starts_with("Location_"))
        .map(|e| (ids(&e["faces2"]), ids(&e["faces"])))
        .collect()
}

/// Paul's example: one 50 from the left edge (datum C) to the 6× pattern's centre line. The
/// pattern is located in x by it, and so is the Ø8 hole, whose axis lies on that line: both
/// are asked y alone, and a save with every suggestion taken writes no second x location.
#[test]
fn one_dimension_locates_every_hole_on_its_line() {
    let loaded = located_plate(&ABC, &[(&[6, 7, 8, 10, 11, 13], &[0], 50.)], "± dimensions");
    let analysis = api::analyse(&loaded).unwrap();
    let t = &analysis["table"];
    for id in [
        format!("hole.location:{PATTERN}"),
        "hole.location:holes:9".to_string(),
    ] {
        let r = row(t, &id);
        assert_eq!(r["state"], "gap", "{r}");
        assert_eq!(r["missing"], json!(["y"]), "{r}");
        assert!(
            r["requirements"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("50 "),
            "{r}"
        );
    }
    agree(&analysis);
    let answers = map(json!({"part.material": "6082-T6", "part.locating": "± dimensions"}));
    let intent = api::apply(&analysis, &answers, true).unwrap();
    let (saved, report) =
        api::write_bytes(loaded.bytes.to_vec(), &[(intent, Some(answers))]).unwrap();
    let saved = load_bytes("plate.step", saved).unwrap();
    let on_the_line = [6, 7, 8, 9, 10, 11, 13];
    // Measured along x: from the end faces 0 and 5, or between two holes at the same y.
    let along_x = |(to, from): &(Vec<usize>, Vec<usize>)| {
        to.iter().any(|f| on_the_line.contains(f))
            && (from.iter().any(|f| [0, 5].contains(f))
                || (from.iter().all(|f| on_the_line.contains(f))
                    && from.iter().chain(to).all(|f| [6, 7, 8].contains(f))
                    || from.iter().chain(to).all(|f| [10, 11, 13].contains(f))))
    };
    let x: Vec<_> = locations(&saved).into_iter().filter(along_x).collect();
    assert_eq!(
        x,
        [(vec![6, 7, 8, 10, 11, 13], vec![0])],
        "{:?}",
        report.written
    );
    let (after, _) = table(&saved);
    for id in [
        format!("hole.location:{PATTERN}"),
        "hole.location:holes:9".into(),
    ] {
        assert_eq!(row(&after, &id)["state"], "stated", "{}", row(&after, &id));
    }
}

/// A dimension from hole A (the Ø8) to hole B (the Ø4.2) locates B, not A.
#[test]
fn a_dimension_from_one_hole_to_another_locates_the_second() {
    let loaded = located_plate(&ABC, &[(&[12], &[9], 45.)], "± dimensions");
    let (t, _) = table(&loaded);
    let b = row(&t, "hole.location:holes:12");
    assert_eq!(
        (&b["state"], &b["missing"]),
        (&json!("gap"), &json!(["y"])),
        "{b}"
    );
    assert!(
        b["requirements"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("45 "),
        "{b}"
    );
    let a = row(&t, "hole.location:holes:9");
    assert_eq!(a["state"], "gap", "{a}");
    assert!(
        a.get("requirements").is_none() && a.get("missing").is_none(),
        "{a}"
    );
}

/// A hole that only ever appears on a reference side (the Ø8, the 45's "from") is not the
/// origin the part is measured from: it is still asked its location.
#[test]
fn a_hole_only_ever_on_a_reference_side_is_still_asked() {
    let loaded = located_plate(&ABC, &[(&[12], &[9], 45.)], "± dimensions");
    let analysis = api::analyse(&loaded).unwrap();
    let hole = "hole.location:holes:9";
    assert_eq!(row(&analysis["table"], hole)["open"], true);
    let (questions, open) = asked(&analysis);
    assert!(questions.contains(&hole.to_string()), "{questions:?}");
    assert!(open.contains(&hole.to_string()), "{open:?}");
    agree(&analysis);
}

/// Position mode, a hole located in x alone: a gap whose suggestion is its y location only.
/// Taking it writes no position tolerance (a second location in x) and no second x location.
#[test]
fn a_hole_located_in_x_only_is_asked_y_only() {
    let loaded = located_plate(&ABC, &[(&[12], &[0], 95.)], "position tolerances");
    let analysis = api::analyse(&loaded).unwrap();
    let hole = "hole.position:holes:12";
    let r = row(&analysis["table"], hole);
    assert_eq!(r["state"], "gap", "{r}");
    assert_eq!(r["missing"], json!(["y"]), "{r}");
    assert_eq!(r["located"], json!(["x"]), "{r}");
    assert_eq!(r["open"], true, "{r}");
    let (questions, _) = asked(&analysis);
    assert!(questions.contains(&hole.to_string()), "{questions:?}");
    agree(&analysis);
    let answers = map(json!({"part.material": "6082-T6", "part.locating": "position tolerances"}));
    let save = api::apply(&analysis, &answers, true).unwrap();
    let (saved, report) =
        api::write_bytes(loaded.bytes.to_vec(), &[(save, Some(answers))]).unwrap();
    let saved = load_bytes("plate.step", saved).unwrap();
    let on_twelve: Vec<_> = locations(&saved)
        .into_iter()
        .filter(|(to, _)| to.contains(&12))
        .collect();
    // The file's 95 in x, and one new location in y (from datum B or the far side).
    assert_eq!(on_twelve.len(), 2, "{on_twelve:?} {:?}", report.written);
    assert!(on_twelve.contains(&(vec![12], vec![0])), "{on_twelve:?}");
    assert!(
        on_twelve
            .iter()
            .any(|(_, from)| from.iter().all(|f| [1, 3].contains(f))),
        "{on_twelve:?}"
    );
    let positions = specify_core::existing::read_existing(&saved)
        .unwrap()
        .into_iter()
        .filter(|e| e.r#type == "Position" && e.faces == [12])
        .count();
    assert_eq!(positions, 0, "{:?}", report.written);
    let (after, _) = table(&saved);
    assert_eq!(
        row(&after, hole)["state"],
        "stated",
        "{}",
        row(&after, hole)
    );
}

/// The datum hole the part is measured from is not asked its location in the directions it
/// is the origin of; in a direction another datum plane measures, it is.
#[test]
fn the_origin_hole_is_not_asked_its_origin_directions() {
    let hole = "hole.location:holes:9";
    let origin = located_plate(
        &[("A", &[2]), ("B", &[9])],
        &[(&[12], &[9], 45.)],
        "± dimensions",
    );
    let analysis = api::analyse(&origin).unwrap();
    let r = row(&analysis["table"], hole);
    assert_eq!(r["state"], "stated", "{r}");
    assert!(r["value"].as_str().unwrap().contains("datum B"), "{r}");
    let (questions, _) = asked(&analysis);
    assert!(!questions.contains(&hole.to_string()), "{questions:?}");
    agree(&analysis);
    // Datum C, the end face 0, measures x: the hole is the origin of y alone.
    let partly = located_plate(
        &[("A", &[2]), ("B", &[9]), ("C", &[0])],
        &[(&[12], &[9], 45.)],
        "± dimensions",
    );
    let (t, _) = table(&partly);
    let r = row(&t, hole);
    assert_eq!(
        (&r["state"], &r["missing"]),
        (&json!("gap"), &json!(["x"])),
        "{r}"
    );
}
