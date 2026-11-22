//! ADR P9 data-loss regressions: a file Specify wrote and something else then changed, or
//! wrote with defaults it never stored, keeps that PMI when it is reopened and written again.
//! Both paths dropped PMI before P9: the editor's preview write (WASM `writePmi`) and the
//! CLI's `write` of the resumed answers.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use haecceity::pmi::{self, PartId, PartPmi};
use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::existing::read_existing;
use specify_core::part::{LoadedPart, load_bytes};
use specify_core::process;

fn plate() -> PathBuf {
    common::fixtures().join("parts").join("plate.step")
}

fn map(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn answers(flatness: &str) -> Map<String, Value> {
    map(json!({
        "part.material": "6082-T6",
        "part.locating": "position tolerances",
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
        "datum.A.flatness": flatness,
        "datum.B.control": "0.1",
        "hole.position:holes:12": "0.1",
    }))
}

/// The plate written by Specify with `given` (defaults taken), storing only `given`.
fn written(given: &Map<String, Value>) -> Vec<u8> {
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let intent = api::apply(&analysis, given, true).unwrap();
    api::write_bytes(
        std::fs::read(plate()).unwrap(),
        &[(intent, Some(given.clone()))],
    )
    .unwrap()
    .0
}

fn load(bytes: Vec<u8>) -> LoadedPart {
    load_bytes("plate.step", bytes).unwrap()
}

/// `bytes` with a flatness of 0.03 on face 2 added by another tool: haecceity's own writer,
/// so the tolerance carries no Specify provenance marker.
fn with_foreign_flatness(bytes: Vec<u8>) -> Vec<u8> {
    let source = written(&answers("0.03"));
    let defs = haecceity::step::read_part_definitions(&source).unwrap();
    let read = pmi::read(&Document::parse(source.clone()).unwrap(), &defs).unwrap();
    let mut flatness = read.parts[0]
        .tolerances
        .iter()
        .find(|t| t.kind == pmi::ToleranceKind::Flatness)
        .unwrap()
        .clone();
    flatness.description = None;
    let added = PartPmi {
        features: read.parts[0].features.clone(),
        tolerances: vec![flatness],
        ..Default::default()
    };
    let doc = Document::parse(bytes).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(PartId(0), added)],
        pmi::write::Mode::Add,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    doc.apply(&edit).unwrap().bytes
}

fn flatness(loaded: &LoadedPart) -> Vec<Value> {
    read_existing(loaded)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "geometric_tolerance" && e.r#type == "Flatness")
        .map(|e| json!({"faces": e.faces, "value": e.value, "marked": e.provenance.is_some()}))
        .collect()
}

fn count(loaded: &LoadedPart, kind: &str) -> usize {
    read_existing(loaded)
        .unwrap()
        .iter()
        .filter(|e| e.kind == kind)
        .count()
}

/// The editor's write (WASM `writePmi`): the preview of the answers plus the unasked rows.
fn preview_write(
    loaded: &LoadedPart,
    analysis: &Value,
    answers: &Map<String, Value>,
) -> LoadedPart {
    let mut intent = api::preview(analysis, answers).unwrap();
    intent
        .requirements
        .extend(specify_core::rules::unasked_rows(analysis, answers).unwrap());
    let bytes = specify_core::writer::write_bytes(loaded, &intent, Some(answers))
        .unwrap()
        .0;
    load(bytes)
}

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-p9-regressions")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cli(args: &[&Path]) {
    let out = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The CLI's rewrite of a reopened file: `analyse`, then `write --accept-defaults` with the
/// resumed answers.
fn cli_rewrite(bytes: &[u8], name: &str) -> LoadedPart {
    let dir = dir(name);
    let input = dir.join("in.step");
    std::fs::write(&input, bytes).unwrap();
    let edits = dir.join("edits.json");
    std::fs::write(&edits, "{}").unwrap();
    let output = dir.join("out.step");
    cli(&[Path::new("write"), &input, &edits, Path::new("-o"), &output]);
    load(std::fs::read(output).unwrap())
}

/// Change the UDA item in place, optionally deleting the provenance on its property link.
fn edited_process(marked: bool) -> LoadedPart {
    let mut given = answers("0.05");
    given.insert(process::QUESTION.into(), json!(process::MACHINED));
    change_process(written(&given), marked)
}

fn change_process(bytes: Vec<u8>, marked: bool) -> LoadedPart {
    let original = String::from_utf8(bytes).unwrap();
    let item = |value| {
        format!(
            "DESCRIPTIVE_REPRESENTATION_ITEM('{}','{value}')",
            process::ITEM
        )
    };
    assert_eq!(original.matches(&item(process::MACHINED)).count(), 1);
    let bytes = original
        .replace(&item(process::MACHINED), &item(process::CAST))
        .into_bytes();
    let doc = Document::parse(bytes).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let i = read.parts[0]
        .attributes
        .iter()
        .position(|set| set.name == process::PROPERTY)
        .unwrap();
    let ids = &read.provenance.parts[0].attributes[i];
    let markers: Vec<_> = ids.iter().flat_map(|&id| doc.referrers(id)).copied().filter(|&id| {
        matches!(doc.get(id), Some(RawEntity::Simple {name, attributes, ..})
            if name == "DESCRIPTION_ATTRIBUTE"
                && matches!(attributes.first(), Some(Attribute::String(s)) if s == "specify-core:written:v1"))
    }).collect();
    assert_eq!(markers.len(), 1);
    if marked {
        return load(doc.bytes().to_vec());
    }
    let mut edit = Edit::new();
    edit.remove(markers[0]);
    load(doc.apply(&edit).unwrap().bytes)
}

fn assert_process(loaded: &LoadedPart, expected: &str) {
    let doc = Document::parse(loaded.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    let sets: Vec<_> = read.parts[0]
        .attributes
        .iter()
        .filter(|set| set.name == process::PROPERTY)
        .collect();
    assert_eq!(
        sets.len(),
        1,
        "expected exactly one process UDA with {expected}"
    );
    assert_eq!(process::read_pmi(&read.parts[0]).unwrap().value, expected);
}

fn process_conflict(analysis: &Value) -> Option<&Value> {
    analysis["resume"]["conflicts"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|conflict| conflict["id"] == process::QUESTION)
}

/// ADR P11: an externally edited process is simply the file's process on reopening, marked
/// or not: no conflict, and the stored answer is not resumed over it.
#[test]
fn externally_edited_process_is_the_files_on_reopening() {
    for marked in [true, false] {
        let loaded = edited_process(marked);
        assert_process(&loaded, process::CAST);
        let analysis = api::analyse(&loaded).unwrap();
        assert!(process_conflict(&analysis).is_none());
        assert_eq!(analysis["part"]["process"], process::CAST);
        assert!(
            analysis["resume"]["answers"]
                .get(process::QUESTION)
                .is_none()
        );
    }
}

#[test]
fn cli_keeps_a_process_edited_with_provenance() {
    assert_process(
        &cli_rewrite(&edited_process(true).bytes, "marked-process"),
        process::CAST,
    );
}

#[test]
fn cli_keeps_a_process_edited_without_provenance() {
    assert_process(
        &cli_rewrite(&edited_process(false).bytes, "unmarked-process"),
        process::CAST,
    );
}

#[test]
fn editor_keeps_a_process_edited_with_provenance() {
    editor_keeps_edited_process(true);
}

#[test]
fn editor_keeps_a_process_edited_without_provenance() {
    editor_keeps_edited_process(false);
}

fn editor_keeps_edited_process(marked: bool) {
    let loaded = edited_process(marked);
    let analysis = api::analyse(&loaded).unwrap();
    let mut resumed = map(analysis["resume"]["answers"].clone());
    resumed.remove(process::QUESTION);
    let written = preview_write(&loaded, &analysis, &resumed);
    assert_process(&written, process::CAST);
    let reopened = api::analyse(&written).unwrap();
    assert!(process_conflict(&reopened).is_none());
    assert_eq!(reopened["part"]["process"], process::CAST);
    // ADR P11 (D5a): the stated process is the file's, whoever wrote it.
    assert_eq!(reopened["process"]["source"], "file");
}

/// Case 1: a bare part Specify wrote (a replace payload, no `pmi_scope`) to which another
/// tool added PMI. Reopened, the added PMI is the file's own (`existing`), and writing again,
/// by the editor or the CLI, keeps it: only Specify's marked requirements are replaced.
#[test]
fn pmi_another_tool_added_to_a_replace_written_part_is_kept() {
    let original = written(&answers("none"));
    let loaded = load(with_foreign_flatness(original));
    let added = flatness(&loaded);
    assert_eq!(
        added,
        [json!({"faces": [2], "value": 0.03, "marked": false})]
    );
    let analysis = api::analyse(&loaded).unwrap();
    assert!(analysis["resume"]["answers"].is_object());
    let resumed = analysis["resume"]["answers"].as_object().unwrap().clone();
    let editor = preview_write(&loaded, &analysis, &resumed);
    assert_eq!(flatness(&editor), added, "editor write");
    let cli = cli_rewrite(&loaded.bytes, "foreign");
    assert_eq!(flatness(&cli), added, "CLI write");
    // Shown on reopening as the file's own PMI.
    assert!(
        analysis["existing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "Flatness" && e["faces"] == json!([2]) && e["value"] == 0.03),
        "{}",
        analysis["existing"]
    );
    // Specify's own requirements are replaced, not stated twice.
    assert_eq!(count(&cli, "datum"), count(&loaded, "datum"));
    assert_eq!(
        count(&cli, "geometric_tolerance"),
        count(&loaded, "geometric_tolerance")
    );
}

/// A file Specify wrote before it marked its requirements (#22): its PMI is the file's (ADR
/// P11), and writing it again states nothing twice: each datum letter once, every
/// requirement once.
#[test]
fn an_unmarked_replace_written_part_is_rewritten_with_nothing_stated_twice() {
    let path = common::fixtures().join("legacy-schema1/legacy_specify.step");
    let loaded = load(std::fs::read(path).unwrap());
    let before = read_existing(&loaded).unwrap();
    assert!(before.iter().all(|e| e.provenance.is_none()));
    let analysis = api::analyse(&loaded).unwrap();
    assert!(!analysis["existing"].as_array().unwrap().is_empty());
    assert!(analysis["resume"].get("conflicts").is_none());
    let rewritten = cli_rewrite(&loaded.bytes, "unmarked");
    let after = read_existing(&rewritten).unwrap();
    let letters: Vec<String> = after
        .iter()
        .filter(|e| e.kind == "datum")
        .map(|e| e.r#type.clone())
        .collect();
    let mut unique = letters.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(letters.len(), unique.len(), "{letters:?}");
    assert_eq!(after.len(), before.len(), "nothing is stated twice");
}

/// Case 2: a file `write --accept-defaults` wrote, storing only the given answers. Reopened,
/// its defaulted requirements are present (not open questions, no conflict), and the editor's
/// write of the resumed answers keeps them.
#[test]
fn defaults_written_without_their_answers_are_kept() {
    let dir = dir("defaults");
    let given = dir.join("edits.json");
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let mut explicit = answers("0.05");
    explicit.remove("part.locating");
    std::fs::write(
        &given,
        common::approve_edits(&analysis, &explicit).to_string(),
    )
    .unwrap();
    let output = dir.join("out.step");
    cli(&[
        Path::new("write"),
        &plate(),
        &given,
        Path::new("-o"),
        &output,
    ]);
    let loaded = load(std::fs::read(&output).unwrap());
    // The given answers are stored, and of the defaults taken only those that write no PMI
    // (2026-11-19: "accept all" keeps answers the file cannot state).
    // The P11 list never stored the implicit locating method.
    let mut stored = explicit;
    for (id, value) in [
        ("part.coating", json!("None (as machined)")),
        ("part.heat_treatment", json!("None")),
        ("part.notes", json!([])),
        ("hole.function:holes:9", json!("general")),
        ("datum.C.control", json!("none")),
    ] {
        stored.insert(id.into(), value);
    }
    // ADR P11: of those, only what leaves no PMI behind; the file states the rest.
    assert_eq!(
        specify_core::resume::answers_for(&loaded).unwrap(),
        Some(specify_core::feature_table::storable(&stored))
    );
    let pattern = "hole.position:hole_patterns.grid:6.7.8.10.11.13";
    let positions = |l: &LoadedPart| {
        read_existing(l)
            .unwrap()
            .into_iter()
            .filter(|e| e.r#type == "Position" && e.faces.contains(&6))
            .count()
    };
    assert_eq!(positions(&loaded), 1);
    let reopened = api::analyse(&loaded).unwrap();
    assert!(reopened["resume"].get("conflicts").is_none());
    let resumed = reopened["resume"]["answers"].as_object().unwrap().clone();
    let editor = preview_write(&loaded, &reopened, &resumed);
    assert_eq!(
        positions(&editor),
        1,
        "editor write keeps the defaulted position"
    );
    for kind in ["datum", "geometric_tolerance", "dimension", "note"] {
        assert_eq!(count(&editor, kind), count(&loaded, kind), "{kind}");
    }
    let review = api::review(&reopened, &resumed).unwrap();
    assert!(
        !review["open"].as_array().unwrap().contains(&json!(pattern)),
        "the defaulted position reads back as present: {}",
        review["open"]
    );
}

/// Case 1 through a session (the MCP `save`, as the page's Save writes): the NX-case file
/// reopened, one question answered, saved, and the other tool's flatness is still there.
#[test]
fn a_session_save_keeps_pmi_another_tool_added() {
    let dir = dir("session");
    let input = dir.join("nx.step");
    let loaded = load(with_foreign_flatness(written(&answers("none"))));
    let added = flatness(&loaded);
    std::fs::write(&input, loaded.bytes.as_slice()).unwrap();
    let mut server = specify_core::mcp::Server::new(std::slice::from_ref(&dir)).unwrap();
    let mut call = |name: &str, arguments: Value| -> Value {
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": name, "arguments": arguments,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": specify_core::mcp::MODERN,
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }});
        let result = server.handle(&request, None).body.unwrap()["result"].clone();
        assert_ne!(result["isError"], json!(true), "{name}: {result}");
        result["structuredContent"].clone()
    };
    let opened = call("open", json!({"path": input}));
    let doc = opened["doc"].clone();
    // The imported flatness withholds "none" and follows ordinary file-PMI handling.
    let analysis = api::analyse(&loaded).unwrap();
    assert!(
        analysis["resume"]["answers"]
            .get("datum.A.flatness")
            .is_none()
    );
    assert!(opened.get("conflicts").is_none(), "{opened}");
    let saved = call(
        "save",
        json!({"doc": doc, "edits": {"fill": {"part.coating": "Anodise, clear"}}}),
    );
    let back = load(std::fs::read(saved["path"].as_str().unwrap()).unwrap());
    assert_eq!(
        flatness(&back),
        added,
        "session save keeps the added flatness"
    );
    // ADR P11: the coating is the file's (a note), not a stored answer.
    let reopened = api::analyse(&back).unwrap();
    assert_eq!(reopened["part"]["coating"], "Anodise, clear");
    assert!(reopened["resume"]["answers"].get("part.coating").is_none());
}

/// A case-2 file whose defaulted pattern position another tool changed (0.6 → 0.3), keeping
/// Specify's marker: the file answers that question (`resume.file`). It is not a conflict,
/// it is not asked, its default is not restated, and the editor's and the CLI's writes keep
/// 0.3, written again and again.
#[test]
fn a_default_another_tool_changed_is_the_files_answer() {
    let given = answers("0.05");
    let text = String::from_utf8(written(&given)).unwrap();
    // The pattern's position zone, written as Specify writes it.
    let zone = "LENGTH_MEASURE(0.6)";
    assert_eq!(text.matches(zone).count(), 1, "one 0.6 zone");
    let loaded = load(text.replace(zone, "LENGTH_MEASURE(0.3)").into_bytes());
    let pattern = "hole.position:hole_patterns.grid:6.7.8.10.11.13";
    let positions = |l: &LoadedPart| -> Vec<f64> {
        read_existing(l)
            .unwrap()
            .into_iter()
            .filter(|e| e.r#type == "Position" && e.faces.contains(&6))
            .map(|e| e.value.unwrap())
            .collect()
    };
    assert_eq!(positions(&loaded), [0.3]);
    let reopened = |l: &LoadedPart| {
        let analysis = api::analyse(l).unwrap();
        assert!(
            analysis["resume"].get("conflicts").is_none(),
            "{}",
            analysis["resume"]
        );
        // ADR P11: the file's position is stated, whoever changed it.
        let row = analysis["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == pattern)
            .unwrap();
        assert_eq!(row["state"], "stated", "{row}");
        assert!(
            row["value"].as_str().unwrap().contains("⌖ ⌀0.3Ⓜ A B C"),
            "{row}"
        );
        let resumed = analysis["resume"]["answers"].as_object().unwrap().clone();
        let review = api::review(&analysis, &resumed).unwrap();
        assert!(
            !review["questions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|q| q["id"] == pattern),
            "the file's position is not asked"
        );
        (analysis, resumed)
    };
    let (analysis, resumed) = reopened(&loaded);
    let editor = preview_write(&loaded, &analysis, &resumed);
    assert_eq!(positions(&editor), [0.3], "editor write");
    let (again, resumed) = reopened(&editor);
    let second = preview_write(&editor, &again, &resumed);
    assert_eq!(positions(&second), [0.3], "editor write, again");
    let cli = cli_rewrite(&second.bytes, "file-answer");
    assert_eq!(positions(&cli), [0.3], "CLI write");
    reopened(&cli);
    let cli_again = cli_rewrite(&cli.bytes, "file-answer-again");
    assert_eq!(positions(&cli_again), [0.3], "CLI write, again");
}

/// The CLI's write of `answers` on a file: `analyse`, then `write --accept-defaults`.
fn cli_refuses_fill(bytes: &[u8], answers: &Map<String, Value>, name: &str) -> LoadedPart {
    let dir = dir(name);
    let input = dir.join("in.step");
    std::fs::write(&input, bytes).unwrap();
    let given = dir.join("edits.json");
    std::fs::write(&given, json!({"fill": answers}).to_string()).unwrap();
    let output = dir.join("out.step");
    let ran = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .args([Path::new("write"), &input, &given, Path::new("-o"), &output])
        .output()
        .unwrap();
    assert_eq!(ran.status.code(), Some(2), "{ran:?}");
    let error = String::from_utf8(ran.stderr).unwrap();
    assert!(
        error.contains("datum.A.flatness") && error.contains("0.05"),
        "{error}"
    );
    assert!(!output.exists());
    load(std::fs::read(input).unwrap())
}

/// A file Specify wrote (flatness 0.03 on face 2, stored as answered) whose flatness another
/// tool re-authored as its own: unmarked, 0.05.
fn re_authored_flatness() -> LoadedPart {
    let text = String::from_utf8(written(&answers("0.03"))).unwrap();
    let line = text
        .lines()
        .find(|l| l.contains("FLATNESS_TOLERANCE("))
        .unwrap()
        .to_string();
    let measure = line.split(',').nth(2).unwrap().trim().to_string();
    let value = text
        .lines()
        .find(|l| l.starts_with(&format!("{measure}=")))
        .unwrap()
        .to_string();
    assert!(line.contains("'specify-core:written:v1'") && value.contains("(0.03)"));
    let text = text
        .replace(&line, &line.replace("'specify-core:written:v1'", "''"))
        .replace(&value, &value.replace("(0.03)", "(0.05)"));
    load(text.into_bytes())
}

/// The other tool's 0.05 stands against answers saved before its edit (which still hold
/// 0.03): written with them through the CLI, it is kept and 0.03 is not written over it.
#[test]
fn another_tools_requirement_stands_against_saved_answers_on_the_cli() {
    let loaded = re_authored_flatness();
    assert_eq!(
        flatness(&loaded),
        [json!({"faces": [2], "value": 0.05, "marked": false})]
    );
    // ADR P11: no conflict; the file's 0.05 is what the table shows.
    let analysis = api::analyse(&loaded).unwrap();
    assert!(analysis["resume"].get("conflicts").is_none());
    let row = analysis["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "datum.A.flatness")
        .unwrap();
    assert_eq!(row["value"], "⏥ 0.05", "{row}");
    // Answers saved before the edit: they say 0.03.
    let saved = answers("0.03");
    let written = cli_refuses_fill(&loaded.bytes, &saved, "saved-answers");
    assert_eq!(
        flatness(&written),
        [json!({"faces": [2], "value": 0.05, "marked": false})]
    );
    let editor = preview_write(&loaded, &analysis, &saved);
    assert_eq!(
        flatness(&editor),
        [json!({"faces": [2], "value": 0.05, "marked": false})]
    );
}

/// The same over MCP: the answers saved before the edit, sent to `save` as a 0.2 answers map
/// (read as fills), are refused all or nothing: the 0.03 differs from what the file states, so
/// nothing is written and the other tool's 0.05 stands.
#[test]
fn another_tools_requirement_stands_against_an_mcp_answers_file() {
    let dir = dir("mcp-foreign");
    let input = dir.join("plate.step");
    std::fs::write(&input, re_authored_flatness().bytes.as_slice()).unwrap();
    let mut server = specify_core::mcp::Server::new(std::slice::from_ref(&dir)).unwrap();
    let mut call = |name: &str, arguments: Value| -> Value {
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": name, "arguments": arguments,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": specify_core::mcp::MODERN,
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }});
        let result = server.handle(&request, None).body.unwrap()["result"].clone();
        assert_ne!(result["isError"], json!(true), "{name}: {result}");
        result["structuredContent"].clone()
    };
    let opened = call("open", json!({"path": input}));
    assert!(opened.get("conflicts").is_none(), "{opened}");
    let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
        "name": "save", "arguments": {"doc": opened["doc"], "edits": answers("0.03")},
        "_meta": {
            "io.modelcontextprotocol/protocolVersion": specify_core::mcp::MODERN,
            "io.modelcontextprotocol/clientCapabilities": {}
        }
    }});
    let refused = server.handle(&request, None).body.unwrap()["result"].clone();
    assert_eq!(refused["isError"], json!(true), "{refused}");
    let failures = refused["structuredContent"]["error"]["failures"].to_string();
    assert!(failures.contains("datum.A.flatness"), "{refused}");
    assert!(
        !dir.join("plate.specified.step").exists(),
        "nothing written"
    );
    assert_eq!(
        flatness(&load(std::fs::read(&input).unwrap())),
        [json!({"faces": [2], "value": 0.05, "marked": false})]
    );
}

/// Saved with the answers from before another tool's edit, the contradicted answer is not
/// stored: reopening resumes no 0.03, shows the file's 0.05, and lists no conflict.
#[test]
fn a_save_does_not_store_an_answer_another_tools_requirement_holds() {
    let loaded = re_authored_flatness();
    let saved = cli_refuses_fill(&loaded.bytes, &answers("0.03"), "saved-over");
    let again = api::analyse(&saved).unwrap();
    assert!(
        again["resume"].get("conflicts").is_none(),
        "{}",
        again["resume"]
    );
    assert!(
        again["resume"]["answers"].get("datum.A.flatness").is_none(),
        "{}",
        again["resume"]
    );
    assert!(
        again["existing"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "Flatness" && e["value"] == 0.05),
        "{}",
        again["existing"]
    );
    assert_eq!(
        flatness(&saved),
        [json!({"faces": [2], "value": 0.05, "marked": false})]
    );
}
