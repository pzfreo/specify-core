//! The command line (`specify-core`, the built binary) and `serve`, against the library and the
//! Python oracle.
//!
//! The oracle sweep, [`the_command_line_matches_the_oracle`], is three checks (docs/design.md,
//! "Feature ids cascade"), and every difference they find must have a verdict in
//! `known_differences/cli.json`:
//!
//! 1. **Composition**, on every oracle case whose input is here: the binary's `analyse`, `mesh`,
//!    `questions` and `write` outputs equal the library's own `api` results on the same inputs,
//!    exactly (the analysis file byte for byte). This is where Rust-produced analyses are
//!    checked: not against Python's, because feature ids differ by design (U2) and every
//!    question id, answer key, intent requirement and report line embedding one would differ.
//!    In a composition difference, `python` holds the library's result and `rust` the binary's.
//! 2. **Python's inputs**: given Python's analysis and the oracle's answers (each case's base
//!    answers and defaults, every scenario), the binary's questions, open questions, intent
//!    (`write --intent`), write report and refusals are compared with Python's; and each
//!    synthetic part's serve transcript is replayed reply by reply through `specify-core serve`
//!    (an analyse reply by its envelope and mesh summary; questions, interpret, preview and
//!    write replies, made from Python's analysis, fully, except a preview's anchor points,
//!    which are compared with the library's on the same request and by which groups have one:
//!    their values against Python's are the anchors sweep's, `tests/anchors.rs`). What other
//!    areas' files explain is filtered out with `explained` (no stale check for them here).
//! 3. **Exit codes, stdout and stderr** of the command lines in `tests/fixtures/cli_oracle.json.gz`
//!    (Python's executable on the synthetic parts, captured by `tests/fixtures/cli_oracle.py`:
//!    usage errors, help, refusals, failures, the summaries) and its serve session (malformed
//!    lines, non-object requests, missing fields, unknown ops, refusals, anchoring another
//!    file).
//!
//! The other tests check behaviour directly: specify-core's `test_assembly.py` command-line
//! test, exit codes, and determinism (two runs give identical stdout and files).

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use common::{
    Difference, Tolerances, case, cases, check_known_with, compare, corpus_dir, explained,
    is_error, load_known, read_json_gz, scenarios,
};
use serde_json::{Map, Value, json};
use specify_core::api::{self, Error};
use specify_core::pyjson;
use specify_core::rules;
use specify_core::serve::Server;
use specify_core::writer::WriteError;

/// The areas whose known differences a CLI difference may already be explained by.
const AREAS: [&str; 7] = [
    "features", "existing", "rules", "mates", "mesh", "anchors", "writer",
];

// ---------------------------------------------------------------------------------------------
// Running the binary
// ---------------------------------------------------------------------------------------------

/// What one run of the binary did.
struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Runs the binary with `args`, in the repository root (so committed inputs can be named by
/// their repository paths, as the CLI oracle names them).
fn run<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the binary runs");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8(out.stdout).expect("UTF-8 stdout"),
        stderr: String::from_utf8(out.stderr).expect("UTF-8 stderr"),
    }
}

fn s(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A fresh scratch directory for this process.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-cli-tests")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Canonical, so the paths the binary repeats are the ones replaced back.
    dir.canonicalize().unwrap()
}

fn save(path: &Path, value: &Value) {
    std::fs::write(path, value.to_string()).unwrap();
}

fn part(stem: &str) -> PathBuf {
    common::fixtures()
        .join("parts")
        .join(format!("{stem}.step"))
}

fn object(value: &Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

/// A `specify-core serve` child, asked one line at a time.
struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Session {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_specify-core"))
            .arg("serve")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
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

    /// Sends `line`; the reply, or None for a blank line (which gets none).
    fn line(&mut self, line: &str) -> Option<Value> {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        if line.trim().is_empty() {
            return None;
        }
        let mut reply = String::new();
        self.stdout.read_line(&mut reply).unwrap();
        Some(serde_json::from_str(&reply).unwrap_or_else(|e| panic!("not a reply: {reply:?}: {e}")))
    }

    /// Closes stdin: the child's exit code, after checking nothing more came on stdout.
    fn finish(mut self) -> i32 {
        drop(self.stdin.take());
        let mut rest = String::new();
        std::io::Read::read_to_string(&mut self.stdout, &mut rest).unwrap();
        assert_eq!(rest, "", "nothing but replies on stdout");
        self.child.wait().unwrap().code().unwrap_or(-1)
    }
}

// ---------------------------------------------------------------------------------------------
// Differences
// ---------------------------------------------------------------------------------------------

fn difference(case: &str, path: &str, python: Option<Value>, rust: Option<Value>) -> Difference {
    Difference {
        case: case.to_string(),
        path: path.to_string(),
        python,
        rust,
    }
}

fn prefixed(diffs: Vec<Difference>, at: &str) -> Vec<Difference> {
    diffs
        .into_iter()
        .map(|mut d| {
            d.path = if d.path.is_empty() {
                at.to_string()
            } else {
                format!("{at}.{}", d.path)
            };
            d
        })
        .collect()
}

/// Exact comparison (composition): no numeric tolerance.
fn exactly(case: &str, at: &str, library: &Value, binary: &Value) -> Vec<Difference> {
    let tolerances = Tolerances {
        relative: 0.0,
        ..Tolerances::default()
    };
    prefixed(compare(case, library, binary, &tolerances), at)
}

/// Two texts that should be equal, as one difference if not.
fn same_text(case: &str, path: &str, expected: &str, got: &str) -> Vec<Difference> {
    if expected == got {
        return Vec::new();
    }
    vec![difference(
        case,
        path,
        Some(json!(expected)),
        Some(json!(got)),
    )]
}

/// The differences between two lists of report lines, as multisets: `path[line]` (as the
/// writer sweep, `tests/write_oracle.rs`, names them, so `writer.json` explains them here too).
fn lines(case: &str, path: &str, python: &Value, rust: &Value) -> Vec<Difference> {
    let count = |v: &Value| {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for l in v.as_array().into_iter().flatten() {
            *m.entry(l.as_str().unwrap_or_default().to_string())
                .or_default() += 1;
        }
        m
    };
    let (p, r) = (count(python), count(rust));
    let mut out = Vec::new();
    for line in p.keys().chain(r.keys()).collect::<BTreeSet<_>>() {
        let (a, b) = (
            p.get(line).copied().unwrap_or(0),
            r.get(line).copied().unwrap_or(0),
        );
        if a != b {
            out.push(difference(
                case,
                &format!("{path}[{line}]"),
                (a > 0).then(|| json!(a)),
                (b > 0).then(|| json!(b)),
            ));
        }
    }
    out
}

/// A write report against Python's: each list of lines as a multiset.
fn report_lines(case: &str, python: &Value, rust: &Value) -> Vec<Difference> {
    ["written", "not_written", "warnings"]
        .iter()
        .flat_map(|key| lines(case, &format!("write.{key}"), &python[key], &rust[key]))
        .collect()
}

/// `text` with every occurrence of each `(from, to)` replaced, in order.
fn replaced(text: &str, subs: &[(&str, &str)]) -> String {
    subs.iter()
        .fold(text.to_string(), |t, (from, to)| t.replace(from, to))
}

/// `value` with each `(from, to)` replaced inside every string.
fn replaced_in(value: &Value, subs: &[(&str, &str)]) -> Value {
    match value {
        Value::String(s) => Value::String(replaced(s, subs)),
        Value::Array(items) => Value::Array(items.iter().map(|v| replaced_in(v, subs)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), replaced_in(v, subs)))
                .collect(),
        ),
        other => other.clone(),
    }
}

// ---------------------------------------------------------------------------------------------
// 1. Composition: the binary equals the library
// ---------------------------------------------------------------------------------------------

/// `analyse`'s stdout for an analysis: faces, features by family in first-appearance order,
/// existing PMI items.
fn summary(analysis: &Value) -> Value {
    let mut counts = Map::new();
    for f in analysis["features"].as_array().into_iter().flatten() {
        let family = f["family"].as_str().unwrap().to_string();
        let n = counts.get(&family).and_then(Value::as_u64).unwrap_or(0);
        counts.insert(family, json!(n + 1));
    }
    let len = |key: &str| analysis[key].as_array().map_or(0, Vec::len);
    json!({"faces": len("faces"), "features": counts, "existing_pmi": len("existing")})
}

/// The command line's stderr and exit code for an API error, as `main.rs` words it.
fn refusal(e: &Error, output: &str) -> (i32, String) {
    match e {
        Error::Rules(rules::Error::Incomplete(e)) => (2, format!("incomplete: {e}\n")),
        Error::Rules(rules::Error::Invalid(m)) => (2, format!("invalid answer: {m}\n")),
        Error::Load(_) | Error::Write(WriteError::Invalid(_)) => {
            (2, format!("cannot write: {e}\n"))
        }
        Error::Write(_) | Error::Analysis(_) => {
            (1, format!("write failed, {output} left as it was: {e}\n"))
        }
    }
}

/// The case name of part `k` of a file case.
fn part_case(file: &str, k: Option<usize>) -> String {
    match k {
        Some(k) => format!("{file}#{k}"),
        None => file.to_string(),
    }
}

/// Each file case with its input here: (file case name, input, parts: None for a file of one).
fn file_cases() -> Vec<(String, PathBuf, Vec<Option<usize>>)> {
    cases()
        .into_iter()
        .filter(|c| c.part().is_none())
        .filter_map(|c| {
            let input = c.input()?;
            let count = c.meta["parts"].as_u64().unwrap_or(1) as usize;
            let parts = if count > 1 {
                (0..count).map(Some).collect()
            } else {
                vec![None]
            };
            Some((c.name, input, parts))
        })
        .collect()
}

fn composition(dir: &Path) -> Vec<Difference> {
    let mut out = Vec::new();
    let base = json!({"part.material": "steel"});
    let answers_path = dir.join("base.json");
    save(&answers_path, &base);
    let mut compared = 0;
    for (file, input, parts) in file_cases() {
        let mut pairs: Vec<String> = Vec::new();
        let mut analyses: Vec<Value> = Vec::new();
        for &k in &parts {
            let name = part_case(&file, k);
            let tag = k.map_or(String::new(), |k| k.to_string());
            let part_args: Vec<String> = k
                .map(|k| vec!["--part".to_string(), k.to_string()])
                .unwrap_or_default();

            // analyse: the file is Python's indent-1 text of the library's analysis.
            let a_path = dir.join(format!("analysis{tag}.json"));
            let mut args = vec!["analyse".to_string(), s(&input)];
            args.extend(part_args.clone());
            args.extend(["-o".to_string(), s(&a_path)]);
            let ran = run(&args);
            let analysis = match api::analyse_file(&input, k) {
                Ok(analysis) => {
                    out.extend(same_text(
                        &name,
                        "composition.analyse.exit",
                        "0",
                        &ran.code.to_string(),
                    ));
                    let text = std::fs::read_to_string(&a_path).unwrap_or_default();
                    out.extend(same_text(
                        &name,
                        "composition.analyse.file",
                        &pyjson::dumps_indent1(&analysis),
                        &text,
                    ));
                    out.extend(same_text(
                        &name,
                        "composition.analyse.stdout",
                        &format!("{}\n", pyjson::dumps(&summary(&analysis))),
                        &ran.stdout,
                    ));
                    Some(analysis)
                }
                Err(e) => {
                    out.extend(same_text(
                        &name,
                        "composition.analyse.exit",
                        "1",
                        &ran.code.to_string(),
                    ));
                    out.extend(same_text(
                        &name,
                        "composition.analyse.stderr",
                        &format!("error: {e}\n"),
                        &ran.stderr,
                    ));
                    None
                }
            };

            // mesh: compact JSON of the library's mesh, its summary, the faces it lacks.
            let m_path = dir.join(format!("mesh{tag}.json"));
            let mut args = vec!["mesh".to_string(), s(&input)];
            args.extend(part_args);
            args.extend(["-o".to_string(), s(&m_path)]);
            let ran = run(&args);
            match api::mesh_file(&input, k) {
                Ok(mesh) => {
                    let text = std::fs::read_to_string(&m_path).unwrap_or_default();
                    out.extend(same_text(
                        &name,
                        "composition.mesh.file",
                        &pyjson::dumps_compact(&mesh),
                        &text,
                    ));
                    let len = |key: &str| mesh[key].as_array().map_or(0, Vec::len);
                    let shown = json!({"faces": len("faces"), "triangles": len("indices") / 3});
                    out.extend(same_text(
                        &name,
                        "composition.mesh.stdout",
                        &format!("{}\n", pyjson::dumps(&shown)),
                        &ran.stdout,
                    ));
                    let missing = mesh["missing"].as_array().unwrap();
                    let warning = if missing.is_empty() {
                        String::new()
                    } else {
                        let faces: Vec<String> = missing.iter().map(Value::to_string).collect();
                        format!(
                            "warning: faces [{}] did not triangulate\n",
                            faces.join(", ")
                        )
                    };
                    out.extend(same_text(
                        &name,
                        "composition.mesh.stderr",
                        &warning,
                        &ran.stderr,
                    ));
                }
                Err(e) => {
                    out.extend(same_text(
                        &name,
                        "composition.mesh.stderr",
                        &format!("error: {e}\n"),
                        &ran.stderr,
                    ));
                }
            }
            std::fs::remove_file(&m_path).ok();

            let Some(analysis) = analysis else { continue };
            // questions: all, and the open ones given the base answers.
            let ran = run(&["questions".to_string(), s(&a_path)]);
            let library = api::questions(&analysis, &Map::new())
                .map(|qs| Value::Array(qs.iter().map(|q| q.to_dict()).collect()));
            match library {
                Ok(qs) => {
                    let got: Value = serde_json::from_str(&ran.stdout).unwrap_or(Value::Null);
                    out.extend(exactly(&name, "composition.questions", &qs, &got));
                }
                Err(e) => out.extend(same_text(
                    &name,
                    "composition.questions.stderr",
                    &format!("invalid answer: {e}\n"),
                    &ran.stderr,
                )),
            }
            let ran = run(&[
                "questions".to_string(),
                s(&a_path),
                "--open".to_string(),
                "--answers".to_string(),
                s(&answers_path),
            ]);
            if let Ok(qs) = api::questions(&analysis, &object(&base)) {
                let open: Vec<Value> = rules::open_questions(&qs, &object(&base))
                    .iter()
                    .map(|q| q.to_dict())
                    .collect();
                let got: Value = serde_json::from_str(&ran.stdout).unwrap_or(Value::Null);
                out.extend(exactly(
                    &name,
                    "composition.questions_open",
                    &Value::Array(open),
                    &got,
                ));
            }
            pairs.extend([s(&a_path), s(&answers_path)]);
            analyses.push(analysis);
        }
        if analyses.len() != parts.len() {
            continue;
        }
        compared += 1;

        // write: the intent file and the report are the library's.
        let cli_out = dir.join("cli.step");
        let intent_path = dir.join("intent.json");
        let mut args = vec!["write".to_string(), s(&input)];
        args.extend(pairs);
        args.extend([
            "-o".to_string(),
            s(&cli_out),
            "--accept-defaults".to_string(),
            "--intent".to_string(),
            s(&intent_path),
        ]);
        let ran = run(&args);
        let several = analyses.len() > 1;
        let mut intents = Vec::new();
        let mut refused = None;
        for analysis in &analyses {
            match api::apply(analysis, &object(&base), true) {
                Ok(intent) => intents.push((intent, Some(object(&base)))),
                Err(e) => {
                    // An assembly's refusal names the part (as main.rs words it).
                    let part = analysis["binding"]["part"].as_u64().unwrap_or(0) as usize;
                    let e = match e {
                        Error::Rules(rules::Error::Incomplete(e)) if several => {
                            Error::from(rules::IncompleteError::new(e.missing, Some(part)))
                        }
                        Error::Rules(rules::Error::Invalid(m)) if several => {
                            Error::Rules(rules::Error::Invalid(format!("part {part}: {m}")))
                        }
                        other => other,
                    };
                    refused = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = refused {
            let (code, stderr) = refusal(&e, &s(&cli_out));
            out.extend(same_text(
                &file,
                "composition.write.exit",
                &code.to_string(),
                &ran.code.to_string(),
            ));
            out.extend(same_text(
                &file,
                "composition.write.stderr",
                &stderr,
                &ran.stderr,
            ));
            continue;
        }
        let shown: Vec<Value> = intents.iter().map(|(i, _)| i.to_dict()).collect();
        let shown = if several {
            Value::Array(shown)
        } else {
            shown[0].clone()
        };
        let text = std::fs::read_to_string(&intent_path).unwrap_or_default();
        out.extend(same_text(
            &file,
            "composition.write.intent",
            &pyjson::dumps_indent1(&shown),
            &text,
        ));
        let lib_out = dir.join("library.step");
        let report = if several {
            api::write_parts(&input, &intents, &lib_out)
        } else {
            api::write_file(&input, &intents[0].0, &lib_out, intents[0].1.as_ref())
        };
        match report {
            Ok(report) => {
                let mut expected = report.to_dict();
                expected["output"] = json!(s(&cli_out));
                let got: Value = serde_json::from_str(&ran.stdout).unwrap_or(Value::Null);
                out.extend(exactly(&file, "composition.write.report", &expected, &got));
                // The two files are the same bytes: the write is deterministic.
                let same = std::fs::read(&lib_out).ok() == std::fs::read(&cli_out).ok();
                if !same {
                    out.push(difference(&file, "composition.write.file", None, None));
                }
            }
            Err(e) => {
                let (code, stderr) = refusal(&e, &s(&cli_out));
                out.extend(same_text(
                    &file,
                    "composition.write.exit",
                    &code.to_string(),
                    &ran.code.to_string(),
                ));
                // The library's message names its own output; the binary's, its own.
                let stderr = stderr.replace(&s(&lib_out), &s(&cli_out));
                out.extend(same_text(
                    &file,
                    "composition.write.stderr",
                    &stderr,
                    &ran.stderr,
                ));
            }
        }
        for p in [&cli_out, &lib_out, &intent_path] {
            std::fs::remove_file(p).ok();
        }
    }
    println!("composition: {compared} files analysed, meshed, asked and written by both");
    out
}

// ---------------------------------------------------------------------------------------------
// 2. Python's inputs
// ---------------------------------------------------------------------------------------------

/// The binary's questions (all) and open ids for `answers` on Python's `analysis`, compared
/// with Python's `questions` and `open` under `at`.
fn questions_against(
    dir: &Path,
    name: &str,
    at: &str,
    analysis: &Value,
    answers: &Value,
    questions: &Value,
    open: &Value,
) -> Vec<Difference> {
    let mut out = Vec::new();
    let (a, q) = (dir.join("a.json"), dir.join("q.json"));
    save(&a, analysis);
    save(&q, answers);
    let ran = run(&["questions", &s(&a), "--answers", &s(&q)]);
    let opened = run(&["questions", &s(&a), "--answers", &s(&q), "--open"]);
    if is_error(questions) {
        // Python refused (a `ValueError`): the command line says `invalid answer: ...`.
        let expected = format!(
            "invalid answer: {}\n",
            questions["message"].as_str().unwrap()
        );
        out.extend(same_text(
            name,
            &format!("{at}.questions.stderr"),
            &expected,
            &ran.stderr,
        ));
        out.extend(same_text(
            name,
            &format!("{at}.questions.exit"),
            "2",
            &ran.code.to_string(),
        ));
        return out;
    }
    let got: Value = serde_json::from_str(&ran.stdout).unwrap_or(Value::Null);
    out.extend(prefixed(
        compare(name, questions, &got, &Tolerances::default()),
        &format!("{at}.questions"),
    ));
    let got: Value = serde_json::from_str(&opened.stdout).unwrap_or(Value::Null);
    let ids: Vec<Value> = got
        .as_array()
        .into_iter()
        .flatten()
        .map(|q| q["id"].clone())
        .collect();
    out.extend(prefixed(
        compare(name, open, &Value::Array(ids), &Tolerances::default()),
        &format!("{at}.open"),
    ));
    out
}

/// The command line's stderr for Python's refusal `failure` of an apply.
fn apply_refusal(failure: &Value) -> String {
    let message = failure["message"].as_str().unwrap_or_default();
    match failure["error"].as_str() {
        Some("IncompleteError") => format!("incomplete: {message}\n"),
        _ => format!("invalid answer: {message}\n"),
    }
}

/// The binary's `write` of `input` with Python's analyses and `answers` (one pair per part),
/// its intent compared with Python's `applied` (one per part: a value or a refusal) and, when
/// given, its report with Python's `report`.
#[allow(clippy::too_many_arguments)]
fn write_against(
    dir: &Path,
    name: &str,
    input: &Path,
    analyses: &[(String, Value)],
    answers: &Value,
    defaults: bool,
    applied: &[Value],
    report: Option<&Value>,
) -> Vec<Difference> {
    let mut out = Vec::new();
    let mut args = vec!["write".to_string(), s(input)];
    for (k, (_, analysis)) in analyses.iter().enumerate() {
        let (a, q) = (
            dir.join(format!("a{k}.json")),
            dir.join(format!("q{k}.json")),
        );
        save(&a, analysis);
        save(&q, answers);
        args.extend([s(&a), s(&q)]);
    }
    let output = dir.join("written.step");
    let intent_path = dir.join("intent.json");
    args.extend([
        "-o".to_string(),
        s(&output),
        "--intent".to_string(),
        s(&intent_path),
    ]);
    if defaults {
        args.push("--accept-defaults".to_string());
    }
    let _ = std::fs::remove_file(&intent_path);
    let ran = run(&args);
    let several = analyses.len() > 1;
    if let Some((k, failure)) = applied.iter().enumerate().find(|(_, a)| is_error(a)) {
        // Python refused to apply: the same refusal, exit 2. In an assembly the command line
        // names the part: an incomplete one as the oracle's case-level write records it (an
        // IncompleteError naming the part), any other refusal prefixed `part K: `, as cli.py
        // re-raises it (the case-level write records that one unprefixed).
        let expected = match report.filter(|r| several && is_error(r)) {
            Some(named) if named["error"] == json!("IncompleteError") => apply_refusal(named),
            _ if several => apply_refusal(&json!({
                "error": failure["error"],
                "message": format!("part {k}: {}", failure["message"].as_str().unwrap_or_default()),
            })),
            _ => apply_refusal(failure),
        };
        out.extend(same_text(name, "apply.refusal", &expected, &ran.stderr));
        out.extend(same_text(name, "apply.exit", "2", &ran.code.to_string()));
        return out;
    }
    let text = std::fs::read_to_string(&intent_path).unwrap_or_default();
    let intent: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let intents: Vec<Value> = if several {
        intent.as_array().cloned().unwrap_or_default()
    } else {
        vec![intent]
    };
    for (k, python) in applied.iter().enumerate() {
        let rust = intents.get(k).cloned().unwrap_or(Value::Null);
        out.extend(prefixed(
            compare(&analyses[k].0, python, &rust, &Tolerances::default()),
            "apply",
        ));
    }
    let Some(report) = report else {
        let _ = std::fs::remove_file(&output);
        return out;
    };
    // As the writer sweep (tests/write_oracle.rs) words them, so writer.json explains them:
    // both refused with different messages, `write.message[<Rust's message>]`; one refused and
    // the other not, `write`; both written, the report's lines.
    let refused = ran.code != 0;
    if is_error(report) && refused {
        let stderr = ran.stderr.trim_end();
        let message = stderr
            .strip_prefix("cannot write: ")
            .or_else(|| stderr.split_once(" left as it was: ").map(|(_, m)| m))
            .unwrap_or(stderr);
        if report["message"].as_str() != Some(message) {
            out.push(difference(
                name,
                &format!("write.message[{message}]"),
                Some(report.clone()),
                Some(json!({"exit": ran.code, "stderr": ran.stderr})),
            ));
        }
    } else if is_error(report) || refused {
        out.push(difference(
            name,
            "write",
            Some(report.clone()),
            Some(json!({"exit": ran.code, "stdout": ran.stdout, "stderr": ran.stderr})),
        ));
    } else {
        let got: Value = serde_json::from_str(&ran.stdout).unwrap_or(Value::Null);
        out.extend(report_lines(name, report, &got));
    }
    let _ = std::fs::remove_file(&output);
    out
}

fn python_inputs(dir: &Path) -> Vec<Difference> {
    let mut out = Vec::new();
    let all = cases();
    let (mut asked, mut writes, mut scenario_count) = (0, 0, 0);
    // Questions on every part's Python analysis: no STEP needed.
    for c in &all {
        let (Some(analysis), Some(questions)) = (c.artifact("analysis"), c.artifact("questions"))
        else {
            continue;
        };
        if is_error(&analysis) {
            continue;
        }
        for label in ["none", "defaults"] {
            let set = &questions[label];
            if is_error(set) {
                continue;
            }
            asked += 1;
            out.extend(questions_against(
                dir,
                &c.name,
                &format!("questions.{label}"),
                &analysis,
                &set["answers"],
                &set["questions"],
                &set["open"],
            ));
        }
    }
    // Each file's base write, from Python's analyses.
    for c in all.iter().filter(|c| c.part().is_none()) {
        let Some(report) = c.artifact("write") else {
            continue;
        };
        if report.get("skipped").is_some() {
            continue;
        }
        let count = c.meta["parts"].as_u64().unwrap_or(1) as usize;
        let part_cases: Vec<String> = if count > 1 {
            (0..count).map(|k| format!("{}#{k}", c.name)).collect()
        } else {
            vec![c.name.clone()]
        };
        let found: Option<Vec<(String, Value, Value)>> = part_cases
            .iter()
            .map(|n| {
                let p = all.iter().find(|p| p.name == *n)?;
                Some((n.clone(), p.artifact("analysis")?, p.artifact("apply")?))
            })
            .collect();
        let Some(found) = found else { continue };
        if found.iter().any(|(_, a, _)| is_error(a)) {
            continue;
        }
        let Some(input) = c.input() else { continue };
        writes += 1;
        let analyses: Vec<(String, Value)> = found
            .iter()
            .map(|(n, a, _)| (n.clone(), a.clone()))
            .collect();
        let applied: Vec<Value> = found.iter().map(|(_, _, i)| i.clone()).collect();
        out.extend(write_against(
            dir,
            &c.name,
            &input,
            &analyses,
            &c.meta["answers"],
            true,
            &applied,
            Some(&report),
        ));
    }
    // Every scenario: its questions, and its intent (and report, when it writes with answers,
    // as the command line always does) through `write`.
    for sc in scenarios() {
        let Some(result) = &sc.result else { continue };
        let c = case(&sc.case);
        let Some(analysis) = c.artifact("analysis") else {
            continue;
        };
        let name = format!("scenarios/{}", sc.name);
        scenario_count += 1;
        out.extend(questions_against(
            dir,
            &name,
            "scenario",
            &analysis,
            &sc.answers,
            &result["questions"],
            &result["open"],
        ));
        let Some(input) = c.input() else { continue };
        let defaults = sc.spec.get("accept_defaults") != Some(&json!(false));
        let report = (sc.spec.get("write") == Some(&json!("with answers")))
            .then(|| result.get("write"))
            .flatten();
        out.extend(write_against(
            dir,
            &name,
            &input,
            &[(name.clone(), analysis)],
            &sc.answers,
            defaults,
            std::slice::from_ref(&result["apply"]),
            report,
        ));
    }
    out.extend(serve_replays(dir));
    println!(
        "python inputs: {asked} question sets, {writes} file writes, {scenario_count} scenarios"
    );
    out
}

/// The mesh summary fields both engines must agree on (the rest, triangles, areas, box and
/// edges, are properties of the triangulation, judged by the mesh sweep).
fn mesh_fields(mesh: &Value, from_summary: bool) -> Value {
    let face_count = if from_summary {
        mesh["face_count"].clone()
    } else {
        json!(mesh["faces"].as_array().map_or(0, Vec::len))
    };
    json!({
        "format": mesh["format"], "version": mesh["version"], "binding": mesh["binding"],
        "face_count": face_count, "missing": mesh["missing"],
    })
}

/// A reply without the keys named.
fn without(reply: &Value, path: &[&str], key: &str) -> Value {
    let mut out = reply.clone();
    let mut at = &mut out;
    for p in path {
        match at.get_mut(*p) {
            Some(next) => at = next,
            None => return out,
        }
    }
    if let Some(map) = at.as_object_mut() {
        map.remove(key);
    }
    out
}

/// Each synthetic part's serve transcript replayed through `specify-core serve`.
fn serve_replays(dir: &Path) -> Vec<Difference> {
    let mut out = Vec::new();
    let output = dir.join("served");
    std::fs::create_dir_all(&output).unwrap();
    for c in cases() {
        let Some(transcript) = c.artifact("serve") else {
            continue;
        };
        let Some(input) = c.input() else { continue };
        let (input_s, output_s) = (s(&input), s(&output));
        let to_rust = [
            ("{input}", input_s.as_str()),
            ("{output}", output_s.as_str()),
        ];
        let to_python = [
            (input_s.as_str(), "{input}"),
            (output_s.as_str(), "{output}"),
        ];
        let name = format!("{}@serve", c.name);
        let mut session = Session::start();
        let mut library = Server::default();
        for (i, entry) in transcript.as_array().unwrap().iter().enumerate() {
            let request = replaced_in(&entry["request"], &to_rust);
            let reply = session.line(&request.to_string()).expect("a reply");
            let mut reply = replaced_in(&reply, &to_python);
            if let Some(trace) = reply.pointer_mut("/error/trace") {
                *trace = json!("(omitted)");
            }
            let python = &entry["reply"];
            let op = request["op"].as_str().unwrap_or_default();
            let at = format!("serve[{i}].{op}");
            let tolerances = Tolerances::default();
            match op {
                "analyse" if python["ok"] == json!(true) => {
                    // The envelope, and the mesh by what both engines must agree on; the
                    // analysis is the Rust engine's (feature ids cascade) and checked by
                    // composition.
                    let envelope = |r: &Value| {
                        let keys: Vec<&String> = r["result"]
                            .as_object()
                            .map(|m| m.keys().collect())
                            .unwrap_or_default();
                        json!({"id": r["id"], "ok": r["ok"], "keys": keys})
                    };
                    out.extend(prefixed(
                        compare(&name, &envelope(python), &envelope(&reply), &tolerances),
                        &at,
                    ));
                    out.extend(prefixed(
                        compare(
                            &name,
                            &mesh_fields(&python["result"]["mesh"], true),
                            &mesh_fields(&reply["result"]["mesh"], false),
                            &tolerances,
                        ),
                        &format!("{at}.mesh"),
                    ));
                }
                "preview" if python["ok"] == json!(true) => {
                    let p = without(python, &["result", "intent"], "anchors");
                    let r = without(&reply, &["result", "intent"], "anchors");
                    out.extend(prefixed(compare(&name, &p, &r, &tolerances), &at));
                    // Which groups have an anchor, against Python; the anchors themselves,
                    // against the library on the same request.
                    let keys = |v: &Value| -> Value {
                        json!(
                            v.pointer("/result/intent/anchors")
                                .and_then(Value::as_object)
                                .map(|m| m.keys().cloned().collect::<Vec<_>>())
                        )
                    };
                    out.extend(prefixed(
                        compare(&name, &keys(python), &keys(&reply), &tolerances),
                        &format!("{at}.anchors"),
                    ));
                    let expected = replaced_in(&library.reply(&request), &to_python);
                    out.extend(exactly(
                        &name,
                        &format!("composition.{at}"),
                        &expected,
                        &reply,
                    ));
                }
                "write" if python["ok"] == json!(true) && reply["ok"] == json!(true) => {
                    let p = without(python, &["result"], "report");
                    let r = without(&reply, &["result"], "report");
                    out.extend(prefixed(compare(&name, &p, &r, &tolerances), &at));
                    out.extend(report_lines(
                        &name,
                        &python["result"]["report"],
                        &reply["result"]["report"],
                    ));
                    out.extend(same_text(
                        &name,
                        &format!("{at}.output"),
                        python["result"]["report"]["output"]
                            .as_str()
                            .unwrap_or_default(),
                        reply["result"]["report"]["output"]
                            .as_str()
                            .unwrap_or_default(),
                    ));
                }
                _ => out.extend(prefixed(compare(&name, python, &reply, &tolerances), &at)),
            }
        }
        out.extend(same_text(
            &name,
            "serve.exit",
            "0",
            &session.finish().to_string(),
        ));
    }
    let _ = std::fs::remove_dir_all(&output);
    out
}

// ---------------------------------------------------------------------------------------------
// 3. Exit codes, stdout and stderr against Python's executable
// ---------------------------------------------------------------------------------------------

fn command_lines(dir: &Path) -> Vec<Difference> {
    let oracle = read_json_gz(&common::fixtures().join("cli_oracle.json.gz"));
    let mut out = Vec::new();
    let runs = oracle["runs"].as_array().unwrap();
    for (i, r) in runs.iter().enumerate() {
        let name = format!("cli/{}", r["name"].as_str().unwrap());
        let tmp = dir.join(format!("run{i}"));
        std::fs::create_dir_all(&tmp).unwrap();
        for (file, spec) in r["inputs"].as_object().unwrap() {
            let text = if let Some(a) = spec.get("artifact") {
                let c = case(a[0].as_str().unwrap());
                c.artifact(a[1].as_str().unwrap()).unwrap().to_string()
            } else if let Some(v) = spec.get("json") {
                v.to_string()
            } else {
                spec["text"].as_str().unwrap().to_string()
            };
            std::fs::write(tmp.join(file), text).unwrap();
        }
        let tmp_s = s(&tmp);
        let args: Vec<String> = r["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a.as_str().unwrap().replace("{tmp}", &tmp_s))
            .collect();
        let ran = run(&args);
        let scrub = |t: &str| t.replace(&tmp_s, "{tmp}");
        let (stdout, stderr) = (scrub(&ran.stdout), scrub(&ran.stderr));
        out.extend(same_text(
            &name,
            "exit",
            &r["exit"].to_string(),
            &ran.code.to_string(),
        ));
        if r["stdout_noise"].as_u64().unwrap_or(0) > 0 {
            out.push(difference(
                &name,
                "stdout_noise",
                Some(r["stdout_noise"].clone()),
                Some(json!(0)),
            ));
        }
        let python_stdout = r["stdout"].as_str().unwrap();
        let parsed = (
            serde_json::from_str::<Value>(python_stdout),
            serde_json::from_str::<Value>(&stdout),
        );
        match parsed {
            (Ok(p), Ok(q)) if name.ends_with("/write") && r["exit"] == json!(0) => {
                // The report's lines are compared in check 2 (the same write from the same
                // inputs); here, that it is the report of this output.
                out.extend(prefixed(
                    compare(&name, &p["output"], &q["output"], &Tolerances::default()),
                    "stdout.output",
                ));
            }
            (Ok(p), Ok(q)) => {
                let diffs = prefixed(compare(&name, &p, &q, &Tolerances::default()), "stdout");
                if diffs.is_empty() {
                    out.extend(same_text(&name, "stdout.text", python_stdout, &stdout));
                }
                out.extend(diffs);
            }
            _ => out.extend(same_text(&name, "stdout", python_stdout, &stdout)),
        }
        out.extend(same_text(
            &name,
            "stderr",
            r["stderr"].as_str().unwrap(),
            &stderr,
        ));
        for (file, python) in r["written"].as_object().unwrap() {
            let rust = std::fs::read_to_string(tmp.join(file))
                .ok()
                .map(|t| scrub(&t));
            if python.as_str() != rust.as_deref() {
                out.push(difference(
                    &name,
                    &format!("written.{file}"),
                    Some(python.clone()),
                    rust.map(Value::String),
                ));
            }
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
    // The serve session.
    let tmp = dir.join("serve");
    std::fs::create_dir_all(&tmp).unwrap();
    let tmp_s = s(&tmp);
    let mut session = Session::start();
    let serve = &oracle["serve"];
    for (i, entry) in serve["lines"].as_array().unwrap().iter().enumerate() {
        let line = entry["line"].as_str().unwrap().replace("{tmp}", &tmp_s);
        let reply = session
            .line(&line)
            .map(|r| replaced_in(&r, &[(tmp_s.as_str(), "{tmp}")]));
        let reply = reply.map(|mut r| {
            if let Some(trace) = r.pointer_mut("/error/trace") {
                *trace = json!("(omitted)");
            }
            r
        });
        out.extend(prefixed(
            compare(
                "cli/serve",
                &entry["reply"],
                &reply.unwrap_or(Value::Null),
                &Tolerances::default(),
            ),
            &format!("lines[{i}]"),
        ));
    }
    out.extend(same_text(
        "cli/serve",
        "exit",
        &serve["exit"].to_string(),
        &session.finish().to_string(),
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    // The executables: specify-core's package scripts against this crate's binaries.
    let cargo =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let names = regex::Regex::new(r#"(?m)^\[\[bin\]\]\s*\nname = "([^"]+)""#).unwrap();
    let ours: BTreeSet<String> = names
        .captures_iter(&cargo)
        .map(|c| c[1].to_string())
        .collect();
    for program in oracle["programs"].as_array().unwrap() {
        let program = program.as_str().unwrap();
        if !ours.contains(program) {
            out.push(difference(
                "cli/programs",
                &format!("programs.{program}"),
                Some(json!(program)),
                None,
            ));
        }
    }
    // The analysis and intent files' text: Python's `json.dumps(indent=1)` against what the
    // command line writes for the same values (a float reaches the file through serde_json,
    // which has no non-finite number, then pyjson's indent-1 form).
    for (key, python) in oracle["json_text"].as_object().unwrap() {
        let value = match key.as_str() {
            "nan" => serde_json::to_value(f64::NAN).unwrap(),
            "infinity" => serde_json::to_value(f64::INFINITY).unwrap(),
            "minus_infinity" => serde_json::to_value(f64::NEG_INFINITY).unwrap(),
            "non_ascii" => json!("Ø6.6 H11, ±0.1, Ra 0.8 µm, M5×0.8, ⌀4.2"),
            other => panic!("json_text: unknown value {other}"),
        };
        out.extend(same_text(
            "cli/json-text",
            &format!("json_text.{key}"),
            python.as_str().unwrap(),
            &pyjson::dumps_indent1(&value),
        ));
    }
    println!(
        "command lines: {} runs and a serve session of {} lines",
        runs.len(),
        serve["lines"].as_array().unwrap().len()
    );
    out
}

// ---------------------------------------------------------------------------------------------
// The sweep
// ---------------------------------------------------------------------------------------------

/// `check_known("cli")`, but without the corpus (as in CI) the entries for corpus cases and
/// for scenarios on corpus cases are left out: their cases were skipped, so they cannot be
/// stale. With the corpus, every entry must match.
fn check_area(differences: &[Difference]) {
    let path = common::known_differences_file("cli");
    let entries = load_known(&path);
    let entries: Vec<_> = if corpus_dir().is_some() {
        entries
    } else {
        let on_corpus: Vec<String> = scenarios()
            .into_iter()
            .filter(|s| s.case.starts_with("corpus/"))
            .map(|s| format!("scenarios/{}", s.name))
            .collect();
        println!("no corpus: corpus entries of cli not checked");
        entries
            .into_iter()
            .filter(|e| !e.case.starts_with("corpus/") && !on_corpus.contains(&e.case))
            .collect()
    };
    check_known_with(&path.display().to_string(), &entries, differences);
}

#[test]
fn the_command_line_matches_the_oracle() {
    let dir = scratch("oracle");
    let mut differences = composition(&dir);
    differences.extend(explained(&AREAS, &python_inputs(&dir)));
    differences.extend(command_lines(&dir));
    let _ = std::fs::remove_dir_all(&dir);
    check_area(&differences);
}

// ---------------------------------------------------------------------------------------------
// Behaviour
// ---------------------------------------------------------------------------------------------

/// test_assembly.py::test_the_command_line_writes_a_pair_for_each_part
#[test]
fn the_command_line_writes_a_pair_for_each_part() {
    let dir = scratch("pairs");
    let assembly = part("assembly");
    let mut pairs = Vec::new();
    for (k, material) in [(0, "steel"), (1, "brass")] {
        let (a, q) = (
            dir.join(format!("a{k}.json")),
            dir.join(format!("q{k}.json")),
        );
        let ran = run(&[
            "analyse",
            &s(&assembly),
            "--part",
            &k.to_string(),
            "-o",
            &s(&a),
        ]);
        assert_eq!(ran.code, 0, "{}", ran.stderr);
        save(&q, &json!({"part.material": material}));
        pairs.extend([s(&a), s(&q)]);
    }
    let out = dir.join("cli.step");
    let mut args = vec!["write".to_string(), s(&assembly)];
    args.extend(pairs);
    args.extend(["-o".to_string(), s(&out), "--accept-defaults".to_string()]);
    let ran = run(&args);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let report: Value = serde_json::from_str(&ran.stdout).unwrap();
    let written = report["written"].as_array().unwrap();
    assert!(
        written
            .iter()
            .any(|l| l.as_str().unwrap().starts_with("pin: "))
    );
    assert!(written.iter().any(|l| l == "pin: material brass"));
    assert!(written.iter().any(|l| l == "plate: material steel"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Exit codes and messages: 0 done; 2 refused (incomplete, invalid answer, cannot write) or bad
/// arguments; 1 a write that could not be made (the destination left as it was) or a file that
/// cannot be read.
#[test]
fn exit_codes_say_what_happened() {
    let dir = scratch("exits");
    let plate = part("plate");
    let a = dir.join("a.json");
    assert_eq!(run(&["analyse", &s(&plate), "-o", &s(&a)]).code, 0);
    let q = dir.join("q.json");
    let steel = json!({"part.material": "steel"});

    save(&q, &json!({}));
    let ran = run(&[
        "write",
        &s(&plate),
        &s(&a),
        &s(&q),
        "-o",
        &s(&dir.join("o.step")),
        "--accept-defaults",
    ]);
    assert_eq!(
        (ran.code, ran.stderr.as_str()),
        (2, "incomplete: 1 question(s) unanswered: part.material\n")
    );

    save(&q, &json!({"part.material": "steel", "datum.A": [99999]}));
    let ran = run(&[
        "write",
        &s(&plate),
        &s(&a),
        &s(&q),
        "-o",
        &s(&dir.join("o.step")),
        "--accept-defaults",
    ]);
    assert_eq!(
        (ran.code, ran.stderr.as_str()),
        (2, "invalid answer: datum A: face 99999 does not exist\n")
    );

    save(&q, &steel);
    let ran = run(&[
        "write",
        &s(&part("pin")),
        &s(&a),
        &s(&q),
        "-o",
        &s(&dir.join("o.step")),
        "--accept-defaults",
    ]);
    assert_eq!(
        (ran.code, ran.stderr.as_str()),
        (2, "cannot write: intent was made for a different file\n")
    );

    let missing = dir.join("none").join("o.step");
    let ran = run(&[
        "write",
        &s(&plate),
        &s(&a),
        &s(&q),
        "-o",
        &s(&missing),
        "--accept-defaults",
    ]);
    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert!(
        ran.stderr
            .starts_with(&format!("write failed, {} left as it was: ", s(&missing))),
        "{}",
        ran.stderr
    );
    assert_eq!(ran.stdout, "");

    let ran = run(&["write", &s(&plate), &s(&a), "-o", &s(&dir.join("o.step"))]);
    assert_eq!(ran.code, 2);
    assert!(
        ran.stderr.ends_with(
            "specify-core: error: write takes an analysis and its answers for each part\n"
        )
    );

    let ran = run(&["analyse", &s(&part("assembly")), "-o", &s(&a)]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.stderr
            .contains("is an assembly of 2 parts; choose one (0: plate, 1: pin)")
    );

    let ran = run(&["analyse", &s(&plate)]);
    assert_eq!(ran.code, 2);
    assert_eq!(
        ran.stderr,
        "usage: specify-core analyse [-h] -o OUTPUT [--part PART] step\nspecify-core analyse: error: the following arguments are required: -o/--output\n"
    );

    let ran = run(&["parts", "-h"]);
    assert_eq!(ran.code, 0);
    assert!(
        ran.stdout
            .starts_with("usage: specify-core parts [-h] step\n")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Non-ASCII is escaped in the files (as Python's `json.dumps` writes them) and kept on stdout
/// (Python's `ensure_ascii=False`): the plate's interpret call-out has `×` and `⌀`, and the
/// questions' prompts and the intent's general tolerance have `±`.
/// argparse corners no captured case covers, each with the exit code and last stderr line
/// Python 3.14's argparse gives (checked by hand against the venv's `specify_core.cli`).
#[test]
fn arguments_are_parsed_as_argparse_parses_them() {
    let dir = scratch("argparse");
    let pin = "tests/fixtures/parts/pin.step";
    let mesh = s(&dir.join("m.json"));
    let cases: &[(&[&str], i32, &str)] = &[
        // A short option is its first two characters, not bytes (an en dash, a non-ASCII letter).
        (
            &["analyse", pin, "-\u{2013}output", "x.json"],
            2,
            "specify-core analyse: error: the following arguments are required: -o/--output",
        ),
        (
            &["analyse", "-\u{e9}"],
            2,
            "specify-core analyse: error: the following arguments are required: step, -o/--output",
        ),
        // A value given to a flag is quoted as Python's repr quotes it.
        (
            &["write", "a", "b", "c", "--accept-defaults=1", "-o", "x"],
            2,
            "specify-core write: error: argument --accept-defaults: ignored explicit argument '1'",
        ),
        // Positionals are matched a run at a time between options.
        (
            &["write", "S", "a", "b", "-o", "OUT", "c", "d"],
            2,
            "specify-core: error: unrecognized arguments: c d",
        ),
        (
            &["write", "S", "a", "-o", "OUT", "b"],
            2,
            "specify-core: error: unrecognized arguments: b",
        ),
        (
            &["write", "S", "-o", "OUT", "a", "b", "--", "c"],
            2,
            "specify-core: error: write takes an analysis and its answers for each part",
        ),
    ];
    for (args, code, last) in cases {
        let got = run(args);
        assert_eq!(got.code, *code, "{args:?}: {}", got.stderr);
        assert_eq!(got.stderr.lines().last(), Some(*last), "{args:?}");
    }
    // `-o=VALUE` is `-o VALUE`, as argparse splits a short option at `=`.
    let got = run(&["mesh", pin, &format!("-o={mesh}")]);
    assert_eq!(got.code, 0, "{}", got.stderr);
    assert!(dir.join("m.json").is_file());
}

#[test]
fn files_escape_non_ascii_and_stdout_keeps_it() {
    let dir = scratch("ascii");
    let plate = part("plate");
    let a = dir.join("a.json");
    assert_eq!(run(&["analyse", &s(&plate), "-o", &s(&a)]).code, 0);
    let ran = run(&["questions", &s(&a)]);
    assert!(!ran.stdout.is_ascii(), "questions have non-ASCII text");
    assert!(!ran.stdout.contains("\\u"));
    let parsed: Value = serde_json::from_str(&ran.stdout).unwrap();
    let qs = api::questions(
        &serde_json::from_str(&std::fs::read_to_string(&a).unwrap()).unwrap(),
        &Map::new(),
    )
    .unwrap();
    assert_eq!(
        parsed,
        Value::Array(qs.iter().map(|q| q.to_dict()).collect())
    );
    let q = dir.join("q.json");
    save(&q, &json!({"part.material": "steel"}));
    let intent = dir.join("i.json");
    let ran = run(&[
        "write",
        &s(&plate),
        &s(&a),
        &s(&q),
        "-o",
        &s(&dir.join("o.step")),
        "--accept-defaults",
        "--intent",
        &s(&intent),
    ]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let text = std::fs::read_to_string(&intent).unwrap();
    assert!(
        text.is_ascii(),
        "the intent file is ASCII, non-ASCII escaped"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two runs give identical stdout and files.
#[test]
fn runs_are_deterministic() {
    let dir = scratch("determinism");
    let once = |n: usize| -> Vec<(String, Vec<u8>)> {
        let mut seen = Vec::new();
        for (stem, k) in [("plate", None), ("stack", Some(1)), ("assembly", Some(0))] {
            let step = s(&part(stem));
            let tag = format!("{stem}{n}");
            let (a, m) = (
                dir.join(format!("a{tag}.json")),
                dir.join(format!("m{tag}.json")),
            );
            let mut part_args: Vec<String> = Vec::new();
            if let Some(k) = k {
                part_args = vec!["--part".into(), k.to_string()];
            }
            let mut args = vec!["analyse".to_string(), step.clone(), "-o".into(), s(&a)];
            args.extend(part_args.clone());
            seen.push(("analyse stdout".into(), run(&args).stdout.into_bytes()));
            seen.push(("analysis".into(), std::fs::read(&a).unwrap()));
            let mut args = vec!["mesh".to_string(), step.clone(), "-o".into(), s(&m)];
            args.extend(part_args);
            seen.push(("mesh stdout".into(), run(&args).stdout.into_bytes()));
            seen.push(("mesh".into(), std::fs::read(&m).unwrap()));
            seen.push((
                "questions".into(),
                run(&["questions", &s(&a)]).stdout.into_bytes(),
            ));
            if k.is_none() {
                let q = dir.join("q.json");
                save(&q, &json!({"part.material": "steel"}));
                let (o, i) = (
                    dir.join(format!("o{tag}.step")),
                    dir.join(format!("i{tag}.json")),
                );
                let ran = run(&[
                    "write",
                    &step,
                    &s(&a),
                    &s(&q),
                    "-o",
                    &s(&o),
                    "--accept-defaults",
                    "--intent",
                    &s(&i),
                ]);
                seen.push((
                    "report".into(),
                    ran.stdout.replace(&s(&o), "OUT").into_bytes(),
                ));
                seen.push(("written".into(), std::fs::read(&o).unwrap()));
                seen.push(("intent".into(), std::fs::read(&i).unwrap()));
            }
        }
        seen
    };
    let (first, second) = (once(1), once(2));
    assert_eq!(first.len(), second.len());
    for ((what, a), (_, b)) in first.iter().zip(&second) {
        assert!(a == b, "{what} differs between two runs");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
