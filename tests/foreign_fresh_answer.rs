//! ADR P11: the file's PMI is the only copy. A file Specify wrote with a hole position of 0.5
//! is re-authored by another tool as 1.5: reopening shows 1.5 with no conflict, a save keeps
//! 1.5 and drops the stale answer, and answering 0.5 afresh then must not replace the file's
//! 1.5 (changing it is an explicit edit), on the CLI's path and MCP's.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};
use specify_pmi::api;
use specify_pmi::existing::read_existing;
use specify_pmi::part::{LoadedPart, load_bytes};

const HOLE: &str = "hole.position:holes:12";

fn plate() -> PathBuf {
    common::fixtures().join("parts").join("plate.step")
}

fn load(bytes: Vec<u8>) -> LoadedPart {
    load_bytes("plate.step", bytes).unwrap()
}

fn answers(position: &str) -> Map<String, Value> {
    json!({
        "part.material": "6082-T6",
        "part.locating": "position tolerances",
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
        "datum.A.flatness": "0.03",
        "datum.B.control": "0.1",
        (HOLE): position,
    })
    .as_object()
    .unwrap()
    .clone()
}

/// The plate as Specify wrote it with position 0.5 on hole 12, then another tool re-authored
/// that position as its own: provenance marker removed, value 1.5.
fn re_authored() -> LoadedPart {
    let given = answers("0.5");
    let analysis = api::analyse_file(&plate(), None).unwrap();
    let intent = api::apply(&analysis, &given, true).unwrap();
    let bytes = api::write_bytes(std::fs::read(plate()).unwrap(), &[(intent, Some(given))])
        .unwrap()
        .0;
    let text = String::from_utf8(bytes).unwrap();
    // The position tolerance without modifiers (the pattern's carries MMC).
    let line = text
        .lines()
        .find(|l| l.contains("POSITION_TOLERANCE()") && !l.contains("WITH_MODIFIERS"))
        .unwrap()
        .to_string();
    let measure = line
        .split("GEOMETRIC_TOLERANCE(")
        .nth(1)
        .unwrap()
        .split(',')
        .nth(2)
        .unwrap()
        .trim()
        .to_string();
    let value = text
        .lines()
        .find(|l| l.starts_with(&format!("{measure}=")))
        .unwrap()
        .to_string();
    assert!(line.contains("'specify-core:written:v1'") && value.contains("(0.5)"));
    let text = text
        .replace(&line, &line.replace("'specify-core:written:v1'", "''"))
        .replace(&value, &value.replace("(0.5)", "(1.5)"));
    let loaded = load(text.into_bytes());
    assert_eq!(positions(&loaded), [(1.5, false)]);
    loaded
}

/// The hole's position tolerances: value and whether Specify's marker is on it.
pub fn positions(l: &LoadedPart) -> Vec<(f64, bool)> {
    read_existing(l)
        .unwrap()
        .into_iter()
        .filter(|e| e.r#type == "Position" && e.faces == [12])
        .map(|e| (e.value.unwrap(), e.provenance.is_some()))
        .collect()
}

fn assert_no_conflict(analysis: &Value) {
    assert!(
        analysis["resume"].get("conflicts").is_none(),
        "{}",
        analysis["resume"]
    );
    assert!(
        analysis["resume"]["answers"].get(HOLE).is_none(),
        "{}",
        analysis["resume"]
    );
}

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-pmi-foreign-fresh-answer")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cli(args: &[&Path]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_specify-pmi"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or(Value::Null)
}

/// Apply the list and inspect the saved file or the untouched source after refusal.
fn cli_write(bytes: &[u8], fill: Option<&Map<String, Value>>, name: &str) -> (LoadedPart, Value) {
    let dir = dir(name);
    let input = dir.join("in.step");
    std::fs::write(&input, bytes).unwrap();
    let file = dir.join("edits.json");
    std::fs::write(
        &file,
        fill.map_or_else(|| json!({}), |given| json!({"fill": given}))
            .to_string(),
    )
    .unwrap();
    let output = dir.join("out.step");
    let args = [Path::new("write"), &input, &file, Path::new("-o"), &output];
    if fill.is_none() {
        let report = cli(&args);
        return (load(std::fs::read(output).unwrap()), report);
    }
    let ran = Command::new(env!("CARGO_BIN_EXE_specify-pmi"))
        .args(args)
        .output()
        .unwrap();
    assert_eq!(ran.status.code(), Some(2), "{ran:?}");
    assert!(!output.exists());
    (
        load(std::fs::read(input).unwrap()),
        json!(String::from_utf8(ran.stderr).unwrap()),
    )
}

/// Reopen (no conflict), save, reopen, answer 0.5, save: 1.5 stays alone.
#[test]
fn a_fresh_answer_does_not_replace_another_tools_requirement_on_the_cli() {
    let edited = re_authored();
    assert_no_conflict(&api::analyse(&edited).unwrap());
    let (saved, _) = cli_write(&edited.bytes, None, "save");
    assert_eq!(positions(&saved), [(1.5, false)]);
    let reopened = api::analyse(&saved).unwrap();
    assert_no_conflict(&reopened);
    let mut given = reopened["resume"]["answers"].as_object().unwrap().clone();
    given.insert(HOLE.into(), json!("0.5"));
    let (answered, report) = cli_write(&saved.bytes, Some(&given), "answer");
    assert_eq!(positions(&answered), [(1.5, false)], "{report}");
    let error = report.as_str().unwrap();
    assert!(
        error.contains(HOLE) && error.contains("change with \"to\": \"0.5\", \"expect\": \"1.5\""),
        "{error}"
    );
}

/// The same over MCP: `open`, `save`, then reopen; `answer` 0.5 is refused, and so is `save`
/// with a fill of 0.5 (Specify 0.3.0: a fill differing from what the file states refuses the whole list, naming it, and writes
/// nothing; the other tool's 1.5 stands).
#[test]
fn a_fresh_answer_does_not_replace_another_tools_requirement_over_mcp() {
    let dir = dir("mcp");
    let input = dir.join("plate.step");
    std::fs::write(&input, re_authored().bytes.as_slice()).unwrap();
    let mut server = specify_pmi::mcp::Server::new(std::slice::from_ref(&dir)).unwrap();
    let mut call = |name: &str, arguments: Value| -> Value {
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": name, "arguments": arguments,
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": specify_pmi::mcp::MODERN,
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }});
        server.handle(&request, None).body.unwrap()["result"].clone()
    };
    let ok = |result: Value| {
        assert_ne!(result["isError"], json!(true), "{result}");
        result["structuredContent"].clone()
    };
    let opened = ok(call("open", json!({"path": input})));
    assert!(opened.get("conflicts").is_none(), "{opened}");
    let saved = ok(call("save", json!({"doc": opened["doc"], "edits": {}})));
    let path = saved["path"].as_str().unwrap().to_string();
    let back = load(std::fs::read(&path).unwrap());
    assert_eq!(positions(&back), [(1.5, false)]);
    assert_no_conflict(&api::analyse(&back).unwrap());
    let reopened = ok(call("open", json!({"path": path})));
    let before = std::fs::read(&path).unwrap();
    // ADR P11: the file states the hole's position, so an answer is refused up front with the
    // edit list's reason.
    let refused = call(
        "answer",
        json!({"doc": reopened["doc"], "set": {(HOLE): "0.5"}}),
    );
    assert_eq!(refused["isError"], json!(true), "{refused}");
    let message = refused["structuredContent"]["error"]["message"]
        .as_str()
        .unwrap();
    assert!(
        message.starts_with("the file states")
            && message.contains("⌖ ⌀1.5 A B C")
            && message.contains(&format!("for {HOLE}"))
            && message.contains("change with \"to\": \"0.5\", \"expect\": \"1.5\""),
        "{message}"
    );
    let checked = ok(call("check", json!({"doc": reopened["doc"], "edits": {}})));
    assert_eq!(checked["applicable"], true, "{checked}");
    assert_eq!(checked["entries"], 0, "{checked}");
    assert_eq!(checked["written"], false, "{checked}");
    let refused = call(
        "save",
        json!({"doc": reopened["doc"], "edits": {"fill": {(HOLE): "0.5"}}}),
    );
    assert_eq!(refused["isError"], json!(true), "{refused}");
    let failures = &refused["structuredContent"]["error"]["failures"];
    assert!(
        failures.as_array().is_some_and(|f| f.len() == 1
            && f[0]["entry"].as_str().is_some_and(|e| e.contains(HOLE))
            && f[0]["reason"]
                .as_str()
                .is_some_and(|r| r.contains("already states"))),
        "{refused}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before, "nothing written");
    let noop = ok(call("save", json!({"doc": reopened["doc"], "edits": {}})));
    assert_eq!(noop["report"]["written"], json!([]), "{noop}");
    assert_eq!(noop["report"]["unchanged"], true, "{noop}");
    let noop_bytes = std::fs::read(noop["path"].as_str().unwrap()).unwrap();
    let position_entity = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .lines()
            .find(|line| line.contains("POSITION_TOLERANCE()") && !line.contains("WITH_MODIFIERS"))
            .unwrap()
            .to_owned()
    };
    assert_eq!(position_entity(&noop_bytes), position_entity(&before));
    assert_eq!(positions(&load(noop_bytes)), [(1.5, false)]);

    let guarded = json!({"doc": reopened["doc"], "edits": {
        "change": {(HOLE): {"to": "0.5", "expect": "1.5"}}
    }});
    let checked = ok(call("check", guarded.clone()));
    assert_eq!(checked["applicable"], true, "{checked}");
    assert_eq!(checked["entries"], 1, "{checked}");
    assert_eq!(checked["written"], false, "{checked}");
    let modified = ok(call("save", guarded));
    let changed = load(std::fs::read(modified["path"].as_str().unwrap()).unwrap());
    let positions = positions(&changed);
    assert_eq!(positions.iter().map(|p| p.0).collect::<Vec<_>>(), [0.5]);
    assert!(
        modified["report"]["written"]
            .as_array()
            .is_some_and(|lines| !lines.is_empty()),
        "{modified}"
    );
}
