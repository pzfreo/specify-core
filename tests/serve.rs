//! The child-process protocol a host such as a web app's server speaks: specify-pmi's
//! `tests/test_serve.py`, against the built binary over pipes (`specify-pmi serve`), plus the
//! protocol's promises: every stdout line is a reply, malformed lines are answered and the
//! child carries on, and the child exits 0 when stdin closes.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};
use specify_pmi::serve::Server;

fn part(stem: &str) -> PathBuf {
    common::fixtures()
        .join("parts")
        .join(format!("{stem}.step"))
}

#[test]
fn serve_write_applies_an_edit_list_and_keeps_a_stored_decision() {
    let dir = std::env::temp_dir().join(format!("serve-write-edits-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("decided.step");
    let output = dir.join("written.step");
    let source = specify_pmi::part::load(&part("assembly"), Some(0)).unwrap();
    let saved = common::apply_edits(&source, json!({"none": ["part.general_tolerance"]}));
    std::fs::write(&input, saved.bytes).unwrap();
    let result = Server::default().reply(&json!({
        "id": 1, "op": "write", "step": input, "output": output, "part": 0,
        "edits": {"fill": {"part.material": "steel"}},
    }));
    assert_eq!(result["ok"], true, "{result}");
    let reopened = specify_pmi::api::analyse_file(&output, Some(0)).unwrap();
    assert!(
        reopened["table"]["decided_none"]
            .as_array()
            .unwrap()
            .contains(&json!("part.general_tolerance")),
        "serve write replaced the saved general-tolerance decision"
    );
    // An edit list that cannot be applied writes nothing and names the entry.
    let refused_output = dir.join("refused.step");
    let refused = Server::default().reply(&json!({
        "id": 2, "op": "write", "step": input, "output": refused_output,
        "edits": {"fill": {"no.such.id": "x"}},
    }));
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"]["code"], "invalid", "{refused}");
    assert!(!refused_output.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn full_thread_depth_is_exposed_and_edited_over_serve() {
    let analysis = specify_pmi::api::analyse_file(&part("GRM-02_shaft"), None).unwrap();
    let mut child = Session::start();
    let asked = child.ask(&json!({"id":1, "op":"questions", "analysis":analysis, "answers":{}}));
    assert_eq!(asked["ok"], true, "{asked}");
    let q = asked["result"]["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"].as_str().unwrap().starts_with("hole.full_thread:"))
        .unwrap();
    let id = q["id"].as_str().unwrap();
    assert_eq!(q["default"], "2");
    assert_eq!(q["kind"], "text");
    let function = id.replace("hole.full_thread:", "hole.function:");
    let reply = child.ask(&json!({"id":2, "op":"preview", "analysis":analysis, "answers":{function:"tapped:M2", id:"1.8"}}));
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(
        reply["result"]["intent"]["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"] == "thread")
            .unwrap()["full_thread"],
        1.8
    );
    child.finish();
}

/// A `specify-pmi serve` child, asked one line at a time.
struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Session {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_specify-pmi"))
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("serve starts");
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Session {
            child,
            stdin,
            stdout,
        }
    }

    /// Sends `line` and reads one reply line, which must be JSON (every stdout line is a reply).
    fn line(&mut self, line: &str) -> Value {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        let mut reply = String::new();
        self.stdout.read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap_or_else(|e| panic!("not a reply: {reply:?}: {e}"))
    }

    fn ask(&mut self, request: &Value) -> Value {
        self.line(&request.to_string())
    }

    /// Closes stdin; the child must exit 0 with nothing more on stdout.
    fn finish(mut self) {
        drop(self.stdin.take());
        let mut rest = String::new();
        std::io::Read::read_to_string(&mut self.stdout, &mut rest).unwrap();
        assert_eq!(rest, "", "nothing but replies on stdout");
        assert!(self.child.wait().unwrap().success());
    }
}

/// test_serve.py::test_a_session_over_the_pipe
#[test]
fn a_session_over_the_pipe() {
    let dir = std::env::temp_dir().join(format!("specify-pmi-serve-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Session::start();
    assert_eq!(
        child.ask(&json!({"id": 1, "op": "hello"})),
        json!({"id": 1, "ok": true, "result": {"protocol": 1}})
    );
    let analysed = child.ask(&json!({"id": 2, "op": "analyse", "step": part("plate")}));
    assert_eq!(analysed["ok"], json!(true), "{analysed}");
    let analysis = analysed["result"]["analysis"].clone();
    assert_eq!(analysed["result"]["mesh"]["binding"], analysis["binding"]);

    let questions =
        child.ask(&json!({"id": 3, "op": "questions", "analysis": analysis, "answers": {}}));
    assert!(
        questions["result"]["open"]
            .as_array()
            .unwrap()
            .contains(&json!("part.material"))
    );
    assert!(questions["result"]["problems"].is_array());

    let answers = json!({"part.material": "6082-T6"});
    let preview =
        child.ask(&json!({"id": 4, "op": "preview", "analysis": analysis, "answers": answers}));
    assert_eq!(
        preview["result"]["intent"]["part"]["material"],
        json!("6082-T6")
    );

    let out = dir.join("plate-pmi.step");
    let written = child.ask(&json!({
        "id": 5, "op": "write", "step": part("plate"), "edits": answers, "output": out,
    }));
    assert_eq!(written["ok"], json!(true), "{written}");
    assert!(
        written["result"]["report"]["written"]
            .as_array()
            .unwrap()
            .contains(&json!("material 6082-T6"))
    );
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .starts_with("ISO-10303-21")
    );
    child.finish();
    let _ = std::fs::remove_dir_all(&dir);
}

/// test_serve.py::test_errors_are_structured_and_the_child_survives
#[test]
fn errors_are_structured_and_the_child_survives() {
    let mut child = Session::start();
    let bad = child.ask(&json!({
        "id": 6, "op": "questions", "analysis": {"faces": [], "features": []},
        "answers": {"datum.A": [5]},
    }));
    assert_eq!(bad["ok"], json!(false));
    assert_eq!(bad["error"]["code"], json!("invalid"));
    assert_eq!(bad["id"], json!(6));
    assert_eq!(
        child.ask(&json!({"id": 7, "op": "hello"}))["ok"],
        json!(true)
    );
    child.finish();
}

/// A write of an analysis and answers (before 0.4.0) is refused, saying what to send.
#[test]
fn a_whole_intent_write_is_refused() {
    for extra in [json!({"analysis": {}, "answers": {}}), json!({"parts": []})] {
        let mut request = json!({"id": 8, "op": "write", "step": "x", "output": "y"});
        request
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let out = Server::default().reply(&request);
        assert_eq!(out["error"]["code"], json!("invalid"), "{out}");
        assert!(out["error"]["message"].as_str().unwrap().contains("edits"));
    }
}

/// test_serve.py::test_a_crash_inside_one_request_is_reported
#[test]
fn a_crash_inside_one_request_is_reported() {
    let out =
        Server::default().reply(&json!({"id": 9, "op": "analyse", "step": "/no/such/file.step"}));
    assert_eq!(out["ok"], json!(false));
    assert!(["failed", "invalid"].contains(&out["error"]["code"].as_str().unwrap()));
}

/// test_serve.py::test_a_line_that_is_not_an_object_is_refused_and_the_child_survives, and a
/// line that is not JSON at all, and blank lines (no reply).
#[test]
fn a_line_that_is_not_an_object_is_refused_and_the_child_survives() {
    let mut child = Session::start();
    for line in ["[]", "null", "3", "\"hello\"", "{", "not json"] {
        let out = child.line(line);
        assert_eq!(out["ok"], json!(false), "{line}: {out}");
        assert_eq!(out["error"]["code"], json!("invalid"), "{line}: {out}");
        assert_eq!(out["id"], json!(null));
    }
    // A blank line gets no reply: the next reply is the hello's.
    let stdin = child.stdin.as_mut().unwrap();
    writeln!(stdin, "   ").unwrap();
    assert_eq!(
        child.ask(&json!({"id": 9, "op": "hello"}))["result"]["protocol"],
        json!(1)
    );
    child.finish();
}

/// A missing field is answered as Python's KeyError reads, and an unknown op is named.
#[test]
fn missing_fields_and_unknown_ops_are_invalid() {
    let mut server = Server::default();
    let out = server.reply(&json!({"id": 1, "op": "parts"}));
    assert_eq!(
        out["error"],
        json!({"code": "invalid", "message": "missing field 'step'"})
    );
    let out = server.reply(&json!({"id": 2}));
    assert_eq!(
        out["error"],
        json!({"code": "invalid", "message": "missing field 'analysis'"})
    );
    let out = server.reply(&json!({"id": 3, "op": "bogus", "analysis": {}}));
    assert_eq!(
        out["error"],
        json!({"code": "invalid", "message": "unknown op 'bogus'"})
    );
}

/// `interpret` reads what was typed for a choice question as the answer it means, with the
/// call-out for a hole.
#[test]
fn what_was_typed_is_interpreted() {
    let analysis = specify_pmi::api::analyse_file(&part("plate"), None).unwrap();
    let qs = specify_pmi::api::questions(&analysis, &Default::default()).unwrap();
    let q = qs
        .iter()
        .find(|q| q.id.starts_with("hole.function:") && q.default == Some(json!("tapped:M5")))
        .expect("the plate's tapped hole");
    let out = Server::default().reply(&json!({
        "id": 1, "op": "interpret", "analysis": analysis, "answers": {},
        "question": q.id, "text": "M5",
    }));
    assert_eq!(
        out["result"],
        json!({"value": "tapped:M5", "code": "M5 tapped", "callout": "M5×0.8-6H, ⌀4.2 tap drill"}),
        "{out}"
    );
}

#[test]
fn a_dropped_carried_datum_is_required_not_defaulted() {
    let new = common::fixtures().join("revisions/plate_split.new.step.gz");
    let after = specify_pmi::part::load(&new, None).unwrap();
    // An answers file whose datum could not be applied to this geometry, as one carried by an
    // earlier `diff` was (diff no longer writes answers: ADR P11).
    let carried = json!({"part.material":"6061-T6", "_specify": {
        "evidence": specify_pmi::resume::evidence(&after).unwrap(),
        "unresolved": {"datum.A": "face 4 has no match in the new part"},
    }});
    let analysis = specify_pmi::api::analyse(&after).unwrap();
    let mut server = Server::default();
    let asked =
        server.reply(&json!({"id":2,"op":"questions","analysis":analysis,"answers":carried}));
    assert_eq!(asked["ok"], true, "{asked}");
    let q = asked["result"]["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"] == "datum.A")
        .unwrap();
    assert_eq!(q["default"], json!(null));
    assert_eq!(q["attention"], "required");
    assert!(
        q["reason"]
            .as_str()
            .unwrap()
            .contains("face 4 has no match in the new part")
    );
}

#[test]
fn serve_write_refuses_saved_answers_on_changed_geometry() {
    let old = specify_pmi::part::load(
        &common::fixtures().join("revisions/plate_split.old.step.gz"),
        None,
    )
    .unwrap();
    let new_path = common::fixtures().join("revisions/plate_split.new.step.gz");
    // Answers saved with the old geometry's evidence.
    let saved = json!({"part.material":"6061-T6", "datum.A":[4], "_specify": {
        "evidence": specify_pmi::resume::evidence(&old).unwrap(),
        "unresolved": {},
    }});
    let output =
        std::env::temp_dir().join(format!("serve-stale-evidence-{}.step", std::process::id()));
    let _ = std::fs::remove_file(&output);
    let mut server = Server::default();
    let reply = server.reply(&json!({"id":1,"op":"write","step":new_path,
        "output":output,"edits":saved}));
    assert_eq!(reply["ok"], false, "{reply}");
    assert_eq!(reply["error"]["code"], "invalid", "{reply}");
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("stale answers")
    );
    assert!(!output.exists());
}

#[test]
fn analyse_accepts_and_validates_max_deviation_mm() {
    let mut child = Session::start();
    let good =
        child.ask(&json!({"op":"analyse", "step":part("mesh_cylinder"), "max_deviation_mm":0.01}));
    assert_eq!(good["ok"], true, "{good}");
    assert_eq!(good["result"]["mesh"]["max_deviation_mm"], 0.01);
    assert!(
        good["result"]["mesh"]["achieved_deviation_mm"]
            .as_f64()
            .unwrap()
            <= 0.01
    );
    for value in [json!(0), json!(-1), json!("NaN"), json!(true)] {
        let bad = child
            .ask(&json!({"op":"analyse", "step":part("mesh_cylinder"), "max_deviation_mm":value}));
        assert_eq!(bad["ok"], false);
        assert_eq!(bad["error"]["code"], "invalid");
        assert!(
            bad["error"]["message"]
                .as_str()
                .unwrap()
                .contains("positive finite")
        );
    }
    child.finish();
}
