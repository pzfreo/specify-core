//! The command line (`specify-pmi`, the built binary) and `serve`, against the library and the
//! Python oracle.
//!
//! The oracle sweep, [`the_command_line_matches_the_oracle`], is three checks (docs/design.md,
//! "Feature ids cascade"), and every difference they find must have a verdict in
//! `known_differences/cli.json`:
//!
//! 1. **Composition**, on the named wrapper cases in `file_cases`: the binary's `analyse`, `mesh`,
//!    `questions` and `write` outputs equal the library's own `api` results on the same inputs,
//!    exactly (the analysis file byte for byte). This is where Rust-produced analyses are
//!    checked: not against Python's, because feature ids differ by design (U2) and every
//!    question id, answer key, intent requirement and report line embedding one would differ.
//!    In a composition difference, `python` holds the library's result and `rust` the binary's.
//! 2. **Python's inputs**: given Python's analysis and the oracle's answers (selected cases'
//!    questions and scenarios), the binary's open questions are compared with Python's. Each
//!    former whole-intent write has an exact not-applicable entry. Each
//!    synthetic part's serve transcript is replayed reply by reply through `specify-pmi serve`
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
    is_error, load_known, read_json_gz, scenarios, without_threads_beyond_python,
};
use serde_json::{Map, Value, json};
use specify_pmi::api;
use specify_pmi::pyjson;
use specify_pmi::rules;
use specify_pmi::serve::Server;

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
    let out = Command::new(env!("CARGO_BIN_EXE_specify-pmi"))
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
        .join("specify-pmi-cli-tests")
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

fn write_listed_defaults(input: &Path, name: &str) -> (PathBuf, Value, Value) {
    let dir = scratch(name);
    let analysis = dir.join("analysis.json");
    let answers = dir.join("answers.json");
    let output = dir.join("out.step");
    let analysed = run(&["analyse", &s(input), "-o", &s(&analysis)]);
    assert_eq!(analysed.code, 0, "{}", analysed.stderr);
    let listed = run(&["questions", &s(&analysis)]);
    assert_eq!(listed.code, 0, "{}", listed.stderr);
    let listed: Vec<Value> = serde_json::from_str(&listed.stdout).unwrap();
    assert!(
        listed
            .iter()
            .any(|q| q["id"].as_str().unwrap().starts_with("diameter.runout:"))
    );
    let given = listed
        .into_iter()
        .map(|q| {
            let id = q["id"].as_str().unwrap();
            let answer = if id == "part.material" {
                json!("Steel")
            } else {
                assert!(!q["default"].is_null(), "{id} has no default");
                q["default"].clone()
            };
            (id.to_string(), answer)
        })
        .collect::<Map<String, Value>>();
    let analysis: Value = serde_json::from_slice(&std::fs::read(analysis).unwrap()).unwrap();
    let intent = api::apply(&analysis, &given, false).unwrap().to_dict();
    let edits = json!({"fill": given});
    save(&answers, &edits);
    let written = run(&["write", &s(input), &s(&answers), "-o", &s(&output)]);
    assert_eq!(written.code, 0, "{}", written.stderr);
    (
        output,
        intent,
        serde_json::from_str(&written.stdout).unwrap(),
    )
}

#[test]
fn grm05_write_accepts_every_listed_default() {
    let Some(corpus) = corpus_dir() else { return };
    write_listed_defaults(
        &corpus.join("gramel/GRM-05_depth_lock_bolt.step"),
        "grm05-defaults",
    );
}

const GRM05_LENGTHS: [(&str, u64, f64, &str); 3] = [
    ("length:planes:2-3", 3, 14.0, "14 ±0.2 from B"),
    ("length:planes:2-5", 5, 5.0, "5 ±0.1 from B"),
    ("length:planes:2-8", 8, 18.0, "18 ±0.2 from B"),
];

struct Grm05 {
    input: PathBuf,
    dir: PathBuf,
    analysis: Value,
}

impl Grm05 {
    fn load(name: &str) -> Option<Self> {
        let input = corpus_dir()?.join("gramel/GRM-05_depth_lock_bolt.step");
        let dir = scratch(name);
        let analysis = api::analyse_file(&input, None).unwrap();
        save(&dir.join("analysis.json"), &analysis);
        Some(Self {
            input,
            dir,
            analysis,
        })
    }

    fn other_answers(&self) -> Map<String, Value> {
        let answers: Map<_, _> = api::questions(&self.analysis, &Map::new())
            .unwrap()
            .into_iter()
            .filter(|q| !GRM05_LENGTHS.iter().any(|(id, _, _, _)| q.id == *id))
            .map(|q| {
                let value = if q.id == "part.material" {
                    json!("Steel")
                } else {
                    q.default.unwrap()
                };
                (q.id, value)
            })
            .collect();
        assert_eq!(answers.len(), 17);
        answers
    }

    fn run_write(&self, answers: Map<String, Value>, bulk: bool) -> Run {
        let given = self.dir.join("answers.json");
        let edits = if bulk {
            common::approve_edits(&self.analysis, &answers)
        } else {
            json!({"fill": answers})
        };
        save(&given, &edits);
        let output = self.dir.join("out.step");
        run(&["write", &s(&self.input), &s(&given), "-o", &s(&output)])
    }

    fn write(&self, answers: Map<String, Value>, bulk: bool) -> Value {
        let result = self.run_write(answers, bulk);
        assert_eq!(result.code, 0, "{}", result.stderr);
        api::analyse_file(&self.dir.join("out.step"), None).unwrap()
    }
}

fn grm05_row<'a>(analysis: &'a Value, id: &str) -> &'a Value {
    analysis["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap_or_else(|| panic!("missing {id}"))
}

fn grm05_written(analysis: &Value, face: u64, nominal: f64) -> bool {
    analysis["existing"].as_array().unwrap().iter().any(|item| {
        item["type"] == "Location_LinearDistance"
            && item["faces"] == json!([2])
            && item["faces2"] == json!([face])
            && item["value"].as_f64() == Some(nominal)
    })
}

#[test]
fn grm05_has_three_suggested_length_gaps_before_save() {
    let Some(part) = Grm05::load("grm05-length-gaps") else {
        return;
    };
    let lengths: Vec<_> = part.analysis["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kind"] == "length")
        .collect();
    assert_eq!(lengths.len(), 3);
    let result = run(&["questions", &s(&part.dir.join("analysis.json")), "--open"]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let open: Vec<Value> = serde_json::from_str(&result.stdout).unwrap();
    for (id, face, _, text) in GRM05_LENGTHS {
        let row = grm05_row(&part.analysis, id);
        assert_eq!(row["state"], "gap", "{id}");
        assert_eq!(row["open"], true, "{id} writes PMI");
        assert!(
            open.iter().any(|q| q["id"] == id),
            "{id} absent from --open"
        );
        assert_eq!(row["suggestion"], text, "{id}");
        assert_eq!(row["faces"], json!([face]), "{id}");
    }
}

#[test]
fn grm05_write_lists_three_unanswered_length_gaps_and_leaves_them_open() {
    let Some(part) = Grm05::load("grm05-no-lengths-approved") else {
        return;
    };
    let answers = part.other_answers();
    // #228 writes the answered rows and reports every remaining open gap.
    let result = part.run_write(answers, false);
    assert_eq!(result.code, 0, "{}", result.stderr);
    let report: Value = serde_json::from_str(&result.stdout).unwrap();
    let reopened = api::analyse_file(&part.dir.join("out.step"), None).unwrap();
    assert_eq!(report["missing"], json!(GRM05_LENGTHS.map(|(id, ..)| id)));
    for (id, face, nominal, _) in GRM05_LENGTHS {
        assert!(
            !grm05_written(&reopened, face, nominal),
            "{id} was written without approval"
        );
        let row = grm05_row(&reopened, id);
        assert_eq!(row["state"], "gap", "{id}");
        assert_eq!(row["open"], true, "{id}");
    }
}

#[test]
fn grm05_accepting_suggestions_writes_all_three_lengths() {
    let Some(part) = Grm05::load("grm05-lengths-approved") else {
        return;
    };
    let reopened = part.write(object(&json!({"part.material": "Steel"})), true);
    for (id, face, nominal, _) in GRM05_LENGTHS {
        assert!(grm05_written(&reopened, face, nominal), "{id}");
        assert_eq!(grm05_row(&reopened, id)["state"], "stated", "{id}");
    }
}

#[test]
fn grm05_none_for_one_length_writes_the_other_two_and_survives_reopening() {
    let Some(part) = Grm05::load("grm05-length-none") else {
        return;
    };
    let none = GRM05_LENGTHS[0].0;
    let reopened = part.write(
        object(&json!({"part.material": "Steel", none: "none"})),
        true,
    );
    for (id, face, nominal, _) in GRM05_LENGTHS {
        assert_eq!(grm05_written(&reopened, face, nominal), id != none, "{id}");
        let row = grm05_row(&reopened, id);
        if id == none {
            assert_eq!(row["state"], "decided_none");
            assert_eq!(row["value"], "none");
            assert_eq!(reopened["resume"]["answers"][id], "none");
        } else {
            assert_eq!(row["state"], "stated", "{id}");
        }
    }
}

#[test]
fn grm05_reopening_a_file_with_one_length_shows_it_as_stated() {
    let Some(part) = Grm05::load("grm05-one-length") else {
        return;
    };
    let mut answers = part.other_answers();
    let (approved, _, _, text) = GRM05_LENGTHS[0];
    answers.insert(approved.into(), json!(text));
    let reopened = part.write(answers, false);
    let next = part.dir.join("out.step");
    for (id, face, nominal, _) in GRM05_LENGTHS {
        assert_eq!(
            grm05_written(&reopened, face, nominal),
            id == approved,
            "{id}"
        );
        assert_eq!(
            grm05_row(&reopened, id)["state"],
            if id == approved { "stated" } else { "gap" },
            "{id}"
        );
    }
    let stated = grm05_row(&reopened, approved);
    assert_eq!(stated["requirements"][0]["faces"], json!([2, 3]));

    // The next approval uses the same path the panel uses when it keeps one suggestion.
    let loaded = specify_pmi::part::load(&next, None).unwrap();
    let (second, face, nominal, text) = GRM05_LENGTHS[1];
    let bytes = common::apply_edits(&loaded, json!({"fill": {second: text}})).bytes;
    let updated =
        api::analyse(&specify_pmi::part::load_bytes("updated.step", bytes).unwrap()).unwrap();
    assert!(grm05_written(&updated, face, nominal), "{second}");
    assert!(
        grm05_written(&updated, GRM05_LENGTHS[0].1, GRM05_LENGTHS[0].2),
        "{approved}"
    );
    assert!(!grm05_written(
        &updated,
        GRM05_LENGTHS[2].1,
        GRM05_LENGTHS[2].2
    ));

    // An isolated approval carries its displayed deviation even if the note is still a gap.
    let original = specify_pmi::part::load(&part.input, None).unwrap();
    let bytes = common::apply_edits(&original, json!({"fill": {second: text}})).bytes;
    let isolated =
        api::analyse(&specify_pmi::part::load_bytes("isolated.step", bytes).unwrap()).unwrap();
    assert!(grm05_written(&isolated, face, nominal), "{second}");
    assert_eq!(
        grm05_row(&isolated, second)["requirements"][0]["text"],
        "5 ±0.1"
    );
    assert!(isolated["part"]["general_tolerance"].is_null());
}

#[test]
fn spindle_write_accepts_every_listed_default() {
    write_listed_defaults(&common::fixtures().join("spindle.step"), "spindle-defaults");
}

#[test]
fn spindle_writes_approved_locations_and_sizes() {
    let input = common::fixtures().join("spindle.step");
    let (output, intent, report) = write_listed_defaults(&input, "spindle-general");
    // Four standalone locations and three size approvals. The imported
    // general tolerance governs the approved sizes, including the cross-hole.
    assert_eq!(intent["general"].as_array().unwrap().len(), 7);
    assert_eq!(
        report["not_written"],
        json!([
            "general size 15 on turned_steps:6 faces [6]: the faces establish a datum and the writer cannot mark a datum feature of size",
            "answer diameter.fit:turned_steps:6 ('general'): approved, but nothing of its kind was written on its feature (general size on a datum's own faces); stored as decided"
        ])
    );
    let reopened = api::analyse_file(&output, None).unwrap();
    for (face, nominal) in [(2, 10.0), (7, 4.0)] {
        assert!(
            reopened["existing"].as_array().unwrap().iter().any(|item| {
                item["type"] == "Size_Diameter"
                    && item["faces"] == json!([face])
                    && item["value"] == nominal
                    && item["origin"] == "general_tolerance"
            }),
            "missing diameter {nominal} on face {face}"
        );
    }
    assert_eq!(
        reopened["resume"]["answers"]["_specify"]["no_pmi"],
        json!(["diameter.fit:turned_steps:6"])
    );
    assert!(
        reopened["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["open"] == false)
    );
    for (id, face, value) in [
        ("length:planes:8-10", 8, 12.0),
        ("length:planes:4-10", 4, 34.0),
        ("length:planes:0-10", 0, 52.0),
        ("location:holes:10-7", 7, 23.0),
    ] {
        assert!(
            intent["general"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["question"] == id)
        );
        assert!(
            reopened["existing"].as_array().unwrap().iter().any(|item| {
                item["type"] == "Location_LinearDistance"
                    && item["faces"] == json!([10])
                    && item["faces2"] == json!([face])
                    && item["value"].as_f64() == Some(value)
            }),
            "{id}"
        );
    }
    assert!(
        !intent["general"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["nominal"] == 24.0)
    );
    // The imported position tolerance is untouched, down to the original entity bytes.
    let before = std::fs::read_to_string(input).unwrap();
    let after = std::fs::read_to_string(output).unwrap();
    let position = before
        .lines()
        .find(|line| line.contains("POSITION_TOLERANCE"))
        .unwrap();
    assert!(after.lines().any(|line| line == position), "{position}");
}

/// `-o -` streams the JSON to stdout, byte for byte the file `-o file` writes, and moves the
/// summary to stderr, so the output can be piped into another program (a CAM bridge, `jq`).
#[test]
fn a_dash_output_streams_the_json_to_stdout() {
    let dir = scratch("dash-output");
    let input = s(&part("plate"));
    for command in ["analyse", "mesh"] {
        let file = dir.join(format!("{command}.json"));
        let to_file = run(&[command, "-o", &s(&file), &input]);
        assert_eq!(to_file.code, 0, "{}", to_file.stderr);
        let streamed = run(&[command, "-o", "-", &input]);
        assert_eq!(streamed.code, 0, "{}", streamed.stderr);
        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(streamed.stdout.trim_end(), written.trim_end(), "{command}");
        assert!(streamed.stderr.contains(to_file.stdout.trim()), "{command}");
        let parsed: Value = serde_json::from_str(&streamed.stdout).unwrap();
        assert!(parsed.is_object(), "{command}");
    }
    assert!(!Path::new(env!("CARGO_MANIFEST_DIR")).join("-").exists());
}

#[test]
fn freecad_product_type_upgrades_and_written_pmi_reads_back() {
    freecad_write_roundtrip("hex-standoff-m3");
}

#[test]
fn freecad_independent_diameters_on_a_composite_datum_keep_their_supports() {
    freecad_write_roundtrip("bearing-lm10uu");
}

fn freecad_write_roundtrip(name: &str) {
    use haecceity::{p21::Document, pmi, step::read_part_definitions};

    let dir = scratch(&format!("freecad-product-type-{name}"));
    let fixture = common::fixtures().join(format!("freecad/{name}.step.gz"));
    let loaded = specify_pmi::part::load(&fixture, None).unwrap();
    let input = dir.join(format!("{name}.step"));
    std::fs::write(&input, loaded.bytes.as_slice()).unwrap();
    let analysis = dir.join("a.json");
    let answers = dir.join("answers.json");
    let output = dir.join("out.step");
    let analysed = run(&["analyse", "-o", &s(&analysis), &s(&input)]);
    assert_eq!(analysed.code, 0, "{}", analysed.stderr);
    let analysed = serde_json::from_slice(&std::fs::read(&analysis).unwrap()).unwrap();
    let given = object(&json!({"part.material":"6061-T6"}));
    save(&answers, &common::approve_edits(&analysed, &given));
    let written = run(&["write", &s(&input), &s(&answers), "-o", &s(&output)]);
    assert_eq!(written.code, 0, "{}", written.stderr);
    let report: Value = serde_json::from_str(&written.stdout).unwrap();
    assert!(
        report["written"]
            .as_array()
            .unwrap()
            .contains(&json!("material 6061-T6"))
    );

    let bytes = std::fs::read(&output).unwrap();
    let before = Document::parse(loaded.bytes.as_ref().clone()).unwrap();
    let after = Document::parse(bytes.clone()).unwrap();
    assert!(before.file_schema().unwrap()[0].contains("AUTOMOTIVE_DESIGN"));
    assert!(after.file_schema().unwrap()[0].contains("AP242"));
    let mut rewritten = 0;
    for id in before.ids() {
        let original = &before.bytes()[before.span(id).unwrap()];
        let actual = &after.bytes()[after.span(id).unwrap()];
        let original = std::str::from_utf8(original).unwrap();
        if original.contains("PRODUCT_TYPE(") {
            rewritten += 1;
            assert_eq!(
                actual,
                original
                    .replace("PRODUCT_TYPE(", "PRODUCT_RELATED_PRODUCT_CATEGORY(")
                    .as_bytes()
            );
        } else {
            assert_eq!(actual, original.as_bytes(), "original entity #{id}");
        }
    }
    assert_eq!(rewritten, 1);
    assert_eq!(std::fs::read(&input).unwrap(), loaded.bytes.as_slice());
    let violations = haecceity::express::validate_document(after.graph());
    assert!(violations.is_empty(), "{violations:?}");
    let violations = haecceity::express_rules::check_all(&after);
    assert!(violations.is_empty(), "{violations:?}");
    let defs = read_part_definitions(&bytes).unwrap();
    let read = pmi::read(&after, &defs).unwrap();
    assert_eq!(read.parts[0].material.as_ref().unwrap().id, "6061-T6");
    assert!(!read.parts[0].dimensions.is_empty());
    assert!(!read.parts[0].datums.is_empty());
    if name == "bearing-lm10uu" {
        let lands: Vec<_> = read.parts[0]
            .dimensions
            .iter()
            .filter(|d| {
                matches!(
                    d.kind,
                    pmi::DimensionKind::Size {
                        kind: pmi::SizeKind::Diameter,
                        ..
                    }
                ) && d.nominal.as_ref().is_some_and(
                    |v| matches!(&v.quantity, pmi::Quantity::Length(l) if l.mm() == 19.0),
                )
            })
            .map(|d| {
                let pmi::DimensionKind::Size { feature, .. } = d.kind else {
                    unreachable!()
                };
                let pmi::Feature::Items(items) = &read.parts[0].features[feature.0] else {
                    panic!("expected a face-supported land");
                };
                items
                    .iter()
                    .map(|a| match a {
                        pmi::Anchor::Face(face) => face.0,
                        _ => panic!("expected a face anchor"),
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(
            lands,
            [vec![8], vec![4], vec![0]],
            "three independently sized lands"
        );
    }
    let reopened = api::analyse_file(&output, None).unwrap();
    // ADR P11: the material is the file's (stated), not a stored answer.
    assert_eq!(reopened["part"]["material"], "6061-T6");
    let rewritten = dir.join("rewritten.step");
    // Replaying an empty list keeps the saved requirements and their entity bytes.
    save(&answers, &json!({}));
    let resaved = run(&["write", &s(&output), &s(&answers), "-o", &s(&rewritten)]);
    assert_eq!(resaved.code, 0, "{}", resaved.stderr);
    assert_eq!(std::fs::read(rewritten).unwrap(), bytes, "unchanged resave");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn full_thread_depth_uses_the_existing_questions_and_answers_cli() {
    let dir = scratch("full-thread");
    let a = api::analyse_file(&part("GRM-02_shaft"), None).unwrap();
    let a_path = dir.join("analysis.json");
    save(&a_path, &a);
    let asked = run(&["questions", &s(&a_path)]);
    assert_eq!(asked.code, 0, "{}", asked.stderr);
    let qs: Value = serde_json::from_str(&asked.stdout).unwrap();
    let q = qs
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"].as_str().unwrap().starts_with("hole.full_thread:"))
        .unwrap();
    assert_eq!(q["kind"], "text");
    assert_eq!(q["default"], "2");
    let id = q["id"].as_str().unwrap();
    let given = object(&json!({"part.material": "steel", id: "1.8"}));
    let answers_path = dir.join("edits.json");
    save(&answers_path, &common::approve_edits(&a, &given));
    let out = dir.join("edited.step");
    let written = run(&[
        "write",
        &s(&part("GRM-02_shaft")),
        &s(&answers_path),
        "-o",
        &s(&out),
    ]);
    assert_eq!(written.code, 0, "{}", written.stderr);
    // ADR P11: the depth is the file's thread PMI, not a stored answer.
    let text = std::fs::read_to_string(&out)
        .unwrap()
        .replace(['\r', '\n'], "");
    assert!(
        regex::Regex::new(
            r"LENGTH_MEASURE\(1\.8\),#\d+\)REPRESENTATION_ITEM\('minimum full thread'\)"
        )
        .unwrap()
        .is_match(&text)
    );
    let reopened = api::analyse_file(&out, None).unwrap();
    assert!(reopened["resume"]["answers"].get(id).is_none());
}

#[test]
fn alpha7_tap_survives_cli_resave_without_a_depth_answer() {
    use haecceity::{p21::Document, pmi, step::read_part_definitions};
    let dir = scratch("alpha7-tap");
    let loaded = specify_pmi::part::load(
        &common::fixtures().join("thread-depth-alpha7.step.gz"),
        None,
    )
    .unwrap();
    let input = dir.join("alpha7.step");
    std::fs::write(&input, loaded.bytes.as_slice()).unwrap();
    let a = api::analyse_file(&input, None).unwrap();
    let answers = dir.join("edits.json");
    save(&answers, &common::approve_edits(&a, &Map::new()));
    let output = dir.join("resaved.step");
    let ran = run(&["write", &s(&input), &s(&answers), "-o", &s(&output)]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let bytes = std::fs::read(output).unwrap();
    let defs = read_part_definitions(&bytes).unwrap();
    let doc = Document::parse(bytes).unwrap();
    let read = pmi::read(&doc, &defs).unwrap();
    assert_eq!(
        read.parts[0].threads.len(),
        2,
        "accepted defaults also tap the exact Ø2.5 drill"
    );
    assert!(read.parts[0].attributes.iter().flat_map(|a| &a.items).any(|(n,v)| n == "minimum full thread" && matches!(v, pmi::AttributeValue::Measure(m) if matches!(&m.quantity, pmi::Quantity::Length(l) if l.mm() == 2.0))));
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
        context: None,
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

/// A comparison of the binary's `rust` with the oracle's `python`, once the threads Rust's
/// diameter questions offer beyond Python's are set aside (stream S3;
/// `common::without_threads_beyond_python`): every other option and choice must equal Python's.
fn against_python(
    case: &str,
    python: &Value,
    rust: &Value,
    tolerances: &Tolerances,
) -> Vec<Difference> {
    compare(
        case,
        python,
        &without_threads_beyond_python(rust),
        tolerances,
    )
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
fn part_case(file: &str, k: Option<usize>) -> String {
    match k {
        Some(k) => format!("{file}#{k}"),
        None => file.to_string(),
    }
}

/// Wrapper contracts, rather than a second geometry corpus sweep. These cases cover plain
/// and gzipped inputs, one/two/three parts, explicit part selection, placed geometry,
/// through/blind holes and patterns, and foreign PMI. CLI errors/refusals also have
/// direct coverage in exit_codes_say_what_happened and the captured command lines.
/// Selected corpus geometry remains in features/mesh, Python-intent writes and read-back in
/// write_oracle, and questions/defaults/intents in rules_oracle. `python_inputs` below still
/// checks every selected CLI question set and every scenario. Base corpus writes are the
/// writer sweep's responsibility; SPECIFY_CORPUS_ALL=1 also replays them through the CLI.
fn file_cases() -> Vec<(String, PathBuf, Vec<Option<usize>>)> {
    const WRAPPER_CASES: &[&str] = &[
        "parts/plate",
        "parts/pin",
        "parts/wrapped",
        "parts/assembly",
        "parts/stack",
        "parts/a_file_with_two_parts_must_say_which",
        "parts/a_placed_part_is_meshed_in_the_frame_the_analysis_uses",
        "nist/nist_ctc_01_asme1_ap242",
    ];
    let all = cases();
    WRAPPER_CASES
        .iter()
        .map(|name| {
            all.iter()
                .find(|c| c.name == *name)
                .unwrap_or_else(|| panic!("missing CLI wrapper case {name}"))
                .clone()
        })
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
    let started = std::time::Instant::now();
    let mut out = Vec::new();
    let base = json!({"part.material": "steel"});
    let answers_path = dir.join("base.json");
    save(&answers_path, &base);
    let mut compared = 0;
    for (file, input, parts) in file_cases() {
        let single = parts.len() == 1 && parts[0].is_none();
        let mut analysed = 0;
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
            let given = object(&base);
            if api::apply(&analysis, &given, true).is_ok() {
                let loaded = specify_pmi::part::load(&input, k).unwrap();
                let stored = api::stored_with_defaults(&loaded, &analysis, &given).unwrap();
                let storable = specify_pmi::feature_table::storable;
                assert_eq!(
                    storable(&stored),
                    storable(&common::accept_all::stored(
                        &input,
                        loaded.binding.part,
                        &analysis,
                        &given
                    )),
                    "{file} part {}",
                    loaded.binding.part
                );
            }
            analysed += 1;
            if single {
                let list = if analysis["part"]["material"].is_null() {
                    json!({"fill": base})
                } else {
                    json!({})
                };
                let edits = dir.join("composition-edits.json");
                save(&edits, &list);
                let cli_out = dir.join("cli.step");
                let ran = run(&["write", &s(&input), &s(&edits), "-o", &s(&cli_out)]);
                assert_eq!(ran.code, 0, "{file}: {}", ran.stderr);
                let loaded = specify_pmi::part::load(&input, None).unwrap();
                let list = specify_pmi::edits::parse_value(list).unwrap();
                let applied = specify_pmi::writer::apply::apply(&loaded, &list).unwrap();
                let mut expected = applied.to_dict();
                expected["output"] = json!(s(&cli_out));
                let got: Value = serde_json::from_str(&ran.stdout).unwrap();
                let reopened = api::analyse_file(&cli_out, None).unwrap();
                let missing: Vec<_> = reopened["table"]["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|row| row["open"] == true)
                    .map(|row| &row["id"])
                    .collect();
                if !missing.is_empty() {
                    expected["missing"] = json!(missing);
                }
                out.extend(exactly(&name, "composition.write.report", &expected, &got));
                assert_eq!(std::fs::read(&cli_out).unwrap(), applied.bytes, "{file}");
                std::fs::remove_file(&cli_out).unwrap();
            }
        }
        if analysed == parts.len() {
            compared += 1;
        }
    }
    println!(
        "composition: {compared} files analysed, meshed, asked and written by both in {:?}",
        started.elapsed()
    );
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
        against_python(name, questions, &got, &Tolerances::default()),
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
        against_python(name, open, &Value::Array(ids), &Tolerances::default()),
        &format!("{at}.open"),
    ));
    out
}

/// The command line's stderr for Python's refusal `failure` of an apply.
fn removed_write(case: &str, old: Value) -> Difference {
    difference(case, "whole_intent_write", Some(old), None)
}

fn python_inputs(dir: &Path) -> Vec<Difference> {
    let started = std::time::Instant::now();
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
    // Wrapper writes on committed fixtures (including assemblies and foreign PMI).
    // Corpus writes repeat write_oracle's reports and rules_oracle's intents; leave that
    // geometry work to the library sweeps unless the full local sweep was requested.
    for c in all
        .iter()
        .filter(|c| c.part().is_none() && (c.kind() != "corpus" || common::all_corpus()))
    {
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
        if c.input().is_none() {
            continue;
        }
        writes += 1;
        let status: Vec<_> = found
            .iter()
            .map(|(name, _, apply)| json!({"part": name, "error": apply.get("error")}))
            .collect();
        out.push(removed_write(
            &c.name,
            json!({"apply": status, "write_error": report.get("error")}),
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
        if c.input().is_some() {
            out.push(removed_write(
                &name,
                json!({
                    "accept_defaults": sc.spec.get("accept_defaults") != Some(&json!(false)),
                    "apply_error": result["apply"].get("error"),
                    "write": sc.spec.get("write"),
                }),
            ));
        }
    }
    out.extend(serve_replays(dir));
    println!(
        "python inputs: {asked} question sets, {writes} file writes, {scenario_count} scenarios in {:?}",
        started.elapsed()
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

/// Each synthetic part's serve transcript replayed through `specify-pmi serve`.
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
                        against_python(&name, &envelope(python), &envelope(&reply), &tolerances),
                        &at,
                    ));
                    out.extend(prefixed(
                        against_python(
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
                    out.extend(prefixed(against_python(&name, &p, &r, &tolerances), &at));
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
                        against_python(&name, &keys(python), &keys(&reply), &tolerances),
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
                    out.extend(prefixed(against_python(&name, &p, &r, &tolerances), &at));
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
                _ => out.extend(prefixed(
                    against_python(&name, python, &reply, &tolerances),
                    &at,
                )),
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
        let whole_intent =
            args.first().is_some_and(|a| a == "write") && !name.starts_with("cli/usage/");
        if whole_intent {
            assert_eq!(ran.code, 2, "{name}: {stderr}");
            assert!(
                stderr.contains("whole-intent write was removed"),
                "{name}: {stderr}"
            );
            assert!(stdout.is_empty(), "{name}: {stdout}");
            out.push(removed_write(
                &name,
                json!({"argv": r["argv"], "exit": r["exit"]}),
            ));
            std::fs::remove_dir_all(&tmp).unwrap();
            continue;
        }
        // Python specify-core is retired and the command line is parsed by clap, so help and
        // usage errors are clap's text, not argparse's: the captures' stdout and stderr differ
        // (explained by `cli/usage/*` in known_differences/cli.json). What matters is checked
        // here instead: the exit code (compared with Python's below, as for every run), a usage
        // error's message on stderr naming the argument at fault with nothing on stdout, and
        // help on stdout.
        if let Some(rest) = name.strip_prefix("cli/usage/") {
            usage_or_help(rest, r["exit"].as_i64().unwrap(), &stdout, &stderr);
        }

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
            (Ok(p), Ok(q)) => {
                let diffs = prefixed(
                    against_python(&name, &p, &q, &Tolerances::default()),
                    "stdout",
                );
                // The text too, unless Rust's stdout holds threads beyond Python's (which the
                // value comparison has set aside).
                if diffs.is_empty() && without_threads_beyond_python(&q) == q {
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
            against_python(
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
    // The executables: specify-pmi's package scripts against this crate's binaries.
    let cargo =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let names = regex::Regex::new(r#"(?m)^\[\[bin\]\]\s*\nname = "([^"]+)""#).unwrap();
    let ours: BTreeSet<String> = names
        .captures_iter(&cargo)
        .map(|c| c[1].to_string())
        .collect();
    for program in oracle["programs"].as_array().unwrap() {
        // The oracle's `specify-core` executable is this crate's `specify-pmi`.
        let program = match program.as_str().unwrap() {
            "specify-core" => "specify-pmi",
            other => other,
        };
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

/// Stale checks cover the CLI operations exercised: selected questions, fixture base writes,
/// and scenarios. Full corpus base-write history remains active with SPECIFY_CORPUS_ALL=1.
/// Without corpus inputs, unavailable corpus operations are also left out.
fn check_area(differences: &[Difference]) {
    let path = common::known_differences_file("cli");
    let entries: Vec<_> = load_known(&path)
        .into_iter()
        .filter(|e| {
            if exact_default_array(e) {
                return false;
            }
            common::all_corpus()
                || !e.case.starts_with("corpus/")
                || (!e.path.starts_with("write") && !e.path.starts_with("apply") && e.path != "whole_intent_write"
                // Mixed question/write verdicts still explain every observed difference.
                // Their exact selected question values are independently snapshot-checked.
                && (e.path != "*" || differences.iter().any(|d| e.matches(d))))
        })
        .collect();
    let mut entries: Vec<_> = if corpus_dir().is_some() {
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
            .filter(|e| {
                (!e.case.starts_with("corpus/") && !on_corpus.contains(&e.case))
                    || differences.iter().any(|d| e.matches(d))
            })
            .collect()
    };
    // A broad pre-existing comparison cannot satisfy an exact retired-command case.
    entries.sort_by_key(|entry| entry.path != "whole_intent_write");
    check_known_with(&path.display().to_string(), &entries, differences);
}

/// The 0.5.5 review pins complete captured default arrays. Check those
/// before round 6's field-level comparison projects out known rule departures.
fn exact_default_array(entry: &common::KnownDifference) -> bool {
    entry.path == "questions.defaults.questions" && entry.exact.is_some()
}

fn check_default_arrays(differences: &[Difference]) {
    let entries: Vec<_> = load_known(&common::known_differences_file("cli"))
        .into_iter()
        .filter(exact_default_array)
        .filter(|entry| common::case_selected(&entry.case))
        .collect();
    let cases: BTreeSet<_> = entries.iter().map(|entry| entry.case.as_str()).collect();
    let actual: Vec<_> = differences
        .iter()
        .filter(|d| {
            cases.contains(d.case.as_str()) && d.exact().0 == "questions.defaults.questions"
        })
        .cloned()
        .collect();
    check_known_with("cli_arrays.json", &entries, &actual);
}

/// CLI prefixes artifact names onto paths, whereas rules_oracle puts them in case names.
/// Inherit only exact reviewed old/new values, not the broader verdict scopes. Composition
/// still compares the CLI with the library independently, without filtering any difference.
fn without_reviewed_rule_departures(differences: Vec<Difference>) -> Vec<Difference> {
    let reviewed = common::read_json_gz(&common::fixtures().join("rule_defaults_oracle.json.gz"));
    let key = |case: &str, path: &str, python: &Value, rust: &Value| {
        format!("{case}\0{}\0{python}\0{rust}", path.replace(".[", "["))
    };
    let mut known = std::collections::HashSet::new();
    for d in reviewed.as_array().unwrap() {
        let (case, artifact) = d["case"].as_str().unwrap().rsplit_once('/').unwrap();
        for prefix in [format!("{artifact}."), format!("scenario.{artifact}.")] {
            known.insert(key(
                case,
                &format!("{prefix}{}", d["path"].as_str().unwrap()),
                &d["python"],
                &d["rust"],
            ));
        }
    }
    // Python's accepted-default answers preserve its old datum overrides. That keeps
    // different question membership from rules_oracle's newly accepted defaults. Pin these
    // additional CLI variants exactly, including each field's old and new value.
    let variants = json!([
        [
            "corpus/cadgenbench/flanged_spool_132",
            "questions.defaults.questions[15].faces[0]",
            69,
            34
        ],
        [
            "corpus/cadgenbench/flanged_spool_132",
            "questions.defaults.questions[15].faces[1]",
            null,
            89
        ],
        [
            "corpus/cadgenbench/flanged_spool_132",
            "questions.defaults.questions[15].default[0]",
            69,
            34
        ],
        [
            "corpus/cadgenbench/flanged_spool_132",
            "questions.defaults.questions[15].default[1]",
            null,
            89
        ],
        [
            "corpus/cadgenbench/flanged_spool_132",
            "questions.defaults.questions[16].faces[0]",
            55,
            69
        ],
        [
            "corpus/cadgenbench/flanged_spool_132",
            "questions.defaults.questions[16].default[0]",
            55,
            69
        ],
        [
            "corpus/gramel/string_post",
            "questions.defaults.questions[15].faces[0]",
            11,
            0
        ],
        [
            "corpus/gramel/string_post",
            "questions.defaults.questions[15].default[0]",
            11,
            0
        ],
        [
            "corpus/gramel/string_post",
            "questions.defaults.questions[16].faces[0]",
            null,
            8
        ],
        [
            "corpus/gramel/string_post",
            "questions.defaults.questions[16].default[0]",
            null,
            8
        ],
        [
            "parts/stack#2",
            "questions.defaults.open[1]",
            null,
            "diameter.runout:turned_steps:3"
        ]
    ]);
    for v in variants.as_array().unwrap() {
        known.insert(key(
            v[0].as_str().unwrap(),
            v[1].as_str().unwrap(),
            &v[2],
            &v[3],
        ));
    }
    differences
        .into_iter()
        .filter(|d| !known.contains(&key(&d.case, &d.path, &json!(d.python), &json!(d.rust))))
        .collect()
}

/// Keep CLI/serve envelopes of measurement origins, ordered notes, planner reports (including
/// general-dimension omissions under P11), combined classes, and standalone dimension rows
/// (general dimensions are rows (Paul, 2026-11-21)). Keep these under the
/// library rules' exact before/after guard.
/// The snapshot retains its original datum-origin filename. Older reviewed CLI differences
/// retain their existing verdicts; composition never enters this snapshot.
fn check_datum_origin_departures(differences: &[Difference]) {
    const PATTERN: &str = "measurement datum defaults in CLI and serve envelopes";
    let prior: Vec<_> = load_known(&common::known_differences_file("cli"))
        .into_iter()
        .filter(|e| {
            e.exact.is_none()
                && e.pattern != PATTERN
                && e.pattern != "Shared planner report item in serve (#75)"
                && e.pattern != "Exact tap-drill defaults and dependent question order"
                && e.pattern != "Combined ISO 2768 class choices in CLI and serve"
                && e.pattern != "Absolute slash bounds are accepted by CLI answers"
                && e.pattern != "Open questions are gaps: write writes what is answered (#228)"
                && e.pattern != "general dimensions are rows (Paul, 2026-11-21)"
        })
        .collect();
    let ties: Vec<_> = load_known(&common::known_differences_file("cli"))
        .into_iter()
        .filter(|entry| entry.pattern == PATTERN && entry.verdict == "equivalent")
        .collect();
    let mut departures: Vec<Value> = differences
        .iter()
        .filter(|d| !prior.iter().any(|e| e.matches(d)))
        .map(|d| {
            let mut row =
                json!({"case": d.case, "path": d.path, "python": d.python, "rust": d.rust});
            if ties.iter().any(|entry| entry.matches(d)) {
                row["verdict"] = json!("equivalent");
            }
            row
        })
        .collect();
    let on_corpus: BTreeSet<String> = scenarios()
        .into_iter()
        .filter(|s| s.case.starts_with("corpus/"))
        .map(|s| format!("scenarios/{}", s.name))
        .collect();
    let available = |v: &Value| {
        let case = v["case"].as_str().unwrap();
        let path = v["path"].as_str().unwrap();
        if !common::case_selected(case) {
            return false;
        }
        if !common::all_corpus()
            && case.starts_with("corpus/")
            && (path.starts_with("write") || path.starts_with("apply"))
        {
            return false;
        }
        // Questions use captured analyses and are checked even without STEP inputs.
        corpus_dir().is_some()
            || path.starts_with("questions.")
            || path.starts_with("scenario.")
            || (!case.starts_with("corpus/") && !on_corpus.contains(case))
    };
    let order = |items: &mut Vec<Value>| {
        items.sort_by(|a, b| {
            (a["case"].as_str(), a["path"].as_str()).cmp(&(b["case"].as_str(), b["path"].as_str()))
        })
    };
    order(&mut departures);
    let path = common::fixtures().join("cli_datum_origins.json.gz");
    if std::env::var_os("UPDATE_CLI_DATUM_ORIGINS").is_some() {
        let mut merged = departures.clone();
        if path.is_file() {
            let old = read_json_gz(&path);
            let mut reasons = std::collections::HashMap::new();
            for mut row in old.as_array().unwrap().iter().cloned() {
                if !available(&row) {
                    merged.push(row);
                } else if let Some(reason) = row.as_object_mut().unwrap().shift_remove("reason") {
                    reasons.insert(row.to_string(), reason);
                }
            }
            for row in &mut merged {
                if let Some(reason) = reasons.remove(&row.to_string()) {
                    row["reason"] = reason;
                }
            }
        }
        order(&mut merged);
        let file = std::fs::File::create(&path).unwrap();
        let mut gz = flate2::GzBuilder::new()
            .mtime(0)
            .write(file, flate2::Compression::best());
        gz.write_all(&serde_json::to_vec_pretty(&merged).unwrap())
            .unwrap();
        gz.finish().unwrap();
    }
    let mut expected: Vec<_> = read_json_gz(&path)
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| available(v))
        .cloned()
        .collect();
    // Explanations are fixture metadata; compare all observation fields literally.
    for row in &mut expected {
        if let Some(reason) = row.as_object_mut().unwrap().shift_remove("reason") {
            assert!(
                reason.as_str().is_some_and(|text| !text.trim().is_empty()),
                "an oracle reason must be nonempty: {row}"
            );
        }
    }
    order(&mut expected);
    assert_eq!(
        expected, departures,
        "review CLI datum-origin oracle departures"
    );
}

/// The separate exact snapshot compares the complete question and its order.
/// The ledger records the addition by question ID, and its open-list membership.
fn field_departures(differences: Vec<Difference>) -> Vec<Difference> {
    let mut out = Vec::new();
    let mut added = std::collections::BTreeMap::<_, Vec<Value>>::new();
    let mut opened = std::collections::BTreeMap::<_, Map<String, Value>>::new();
    let measurement =
        |id: &str| id.starts_with("length:planes:") || id.starts_with("location:holes:");
    for mut d in differences {
        if let Some((list, index)) = d.path.rsplit_once('[')
            && index
                .strip_suffix(']')
                .is_some_and(|i| i.bytes().all(|b| b.is_ascii_digit()))
            && let Some(value) = &d.rust
        {
            let list = list.trim_end_matches('.');
            if list.ends_with(".questions")
                && d.python.is_none()
                && let Some(id) = value
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| measurement(id))
            {
                added
                    .entry((d.case, list.to_string()))
                    .or_default()
                    .push(json!(id));
                continue;
            }
            if list.ends_with(".open")
                && let Some(id) = value.as_str().filter(|id| measurement(id))
            {
                opened
                    .entry((d.case.clone(), list.to_string()))
                    .or_default()
                    .insert(id.into(), json!(true));
                if d.python.is_none() {
                    continue;
                }
                // The Python question at this index still needs its own verdict.
                d.rust = None;
            }
        }
        out.push(d);
    }
    for ((case, list), ids) in added {
        out.push(difference(
            &case,
            &format!("{list}.measurement_count"),
            Some(json!(0)),
            Some(json!(ids.len())),
        ));
        out.push(difference(
            &case,
            &format!("{list}.measurement_ids"),
            Some(json!([])),
            Some(json!(ids)),
        ));
    }
    out.extend(opened.into_iter().map(|((case, list), flags)| {
        difference(
            &case,
            &format!("{list}.measurement_flags"),
            Some(json!({})),
            Some(json!(flags)),
        )
    }));
    out
}

#[test]
fn the_command_line_matches_the_oracle() {
    let dir = scratch("oracle");
    let composition = composition(&dir);
    assert!(
        composition.is_empty(),
        "library/executable differences: {composition:?}"
    );
    let (retired, active): (Vec<_>, Vec<_>) = python_inputs(&dir)
        .into_iter()
        .partition(|difference| difference.path == "whole_intent_write");
    check_default_arrays(&active);
    let mut differences = explained(&AREAS, &without_reviewed_rule_departures(active));
    differences.extend(retired);
    differences.extend(command_lines(&dir));
    let _ = std::fs::remove_dir_all(&dir);
    let differences_for_ledger = field_departures(differences.clone());
    if let Some(dir) = std::env::var_os("SPECIFY_DUMP") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rows: Vec<_> = differences_for_ledger
            .iter()
            .map(|d| json!({"case":d.case,"path":d.path,"python":d.python,"rust":d.rust}))
            .collect();
        std::fs::write(
            dir.join("cli.json"),
            serde_json::to_vec_pretty(&rows).unwrap(),
        )
        .unwrap();
    }
    check_datum_origin_departures(&differences);
    check_area(&differences_for_ledger);
}

// ---------------------------------------------------------------------------------------------
// Behaviour
// ---------------------------------------------------------------------------------------------

/// Two explicit writes select the parts in turn and keep the other part's requirements.
#[test]
fn the_command_line_writes_each_selected_part() {
    let dir = scratch("parts-write");
    let mut input = part("assembly");
    for (k, material) in [(0, "steel"), (1, "brass")] {
        let edits = dir.join(format!("edits-{k}.json"));
        let out = dir.join(format!("part-{k}.step"));
        save(&edits, &json!({"fill":{"part.material": material}}));
        let ran = run(&[
            "write",
            &s(&input),
            &s(&edits),
            "--part",
            &k.to_string(),
            "-o",
            &s(&out),
        ]);
        assert_eq!(ran.code, 0, "{}", ran.stderr);
        let report: Value = serde_json::from_str(&ran.stdout).unwrap();
        assert!(report["written"].as_array().unwrap().iter().any(|l| {
            l.as_str()
                .unwrap()
                .ends_with(&format!("material {material}"))
        }));
        input = out;
    }
    for (k, material) in [(0, "steel"), (1, "brass")] {
        assert_eq!(
            api::analyse_file(&input, Some(k)).unwrap()["table"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["id"] == "part.material")
                .unwrap()["value"],
            material
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Exit codes and messages: 0 done (questions left open listed in `missing`, #228); 2 refused
/// (a carried answer not applied, invalid answer, cannot write) or bad
/// arguments; 1 a write that could not be made (the destination left as it was) or a file that
/// cannot be read.
#[test]
fn exit_codes_say_what_happened() {
    let dir = scratch("exits");
    let plate = part("plate");
    let a = dir.join("a.json");
    assert_eq!(run(&["analyse", &s(&plate), "-o", &s(&a)]).code, 0);
    let q = dir.join("q.json");
    let output = dir.join("out.step");
    save(&q, &json!({}));
    let ran = run(&["write", &s(&plate), &s(&q), "-o", &s(&output)]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let report: Value = serde_json::from_str(&ran.stdout).unwrap();
    assert!(
        report["missing"]
            .as_array()
            .unwrap()
            .contains(&json!("part.material"))
    );

    save(
        &q,
        &json!({"fill": {"part.material": "steel", "datum.A": [99999]}}),
    );
    let ran = run(&["write", &s(&plate), &s(&q), "-o", &s(&output)]);
    assert_eq!(ran.code, 2, "{}", ran.stderr);
    assert!(
        ran.stderr.contains("face 99999 does not exist"),
        "{}",
        ran.stderr
    );

    // Ids in edits address the selected file, and an id from another file can be unknown.
    save(&q, &json!({"fill": {"hole.function:holes:12": "fit:H7"}}));
    let ran = run(&["write", &s(&part("pin")), &s(&q), "-o", &s(&output)]);
    assert_eq!(ran.code, 2, "{}", ran.stderr);
    assert!(
        ran.stderr.contains("hole.function:holes:12"),
        "{}",
        ran.stderr
    );

    save(&q, &json!({"fill": {"part.material": "steel"}}));
    let missing = dir.join("none").join("o.step");
    let ran = run(&["write", &s(&plate), &s(&q), "-o", &s(&missing)]);
    assert_eq!(ran.code, 1, "{}", ran.stderr);
    assert!(
        ran.stderr
            .starts_with(&format!("write failed, {} left as it was: ", s(&missing))),
        "{}",
        ran.stderr
    );
    assert_eq!(ran.stdout, "");

    // One JSON after STEP is an edit list (proposal specify-0.3.0, use case 7): an analysis
    // given alone is refused as one, every unknown field named, and nothing written.
    // (Its own output: the write above, with every default taken, made o.step.)
    let ran = run(&[
        "write",
        &s(&plate),
        &s(&a),
        "-o",
        &s(&dir.join("edits.step")),
    ]);
    assert_eq!(ran.code, 2);
    assert!(!dir.join("edits.step").exists());
    assert!(
        ran.stderr.starts_with(
            "cannot write: nothing was written. An edit list is applied all or nothing"
        ) && ran.stderr.contains("- schema: unknown field"),
        "{}",
        ran.stderr
    );

    let ran = run(&["analyse", &s(&part("assembly")), "-o", &s(&a)]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.stderr
            .contains("is an assembly of 2 parts; choose one (0: plate, 1: pin)")
    );

    let ran = run(&["analyse", &s(&plate)]);
    assert_eq!(ran.code, 2);
    assert_eq!(ran.stdout, "");
    assert!(
        ran.stderr.starts_with(
            "error: the following required arguments were not provided:\n  --output <OUTPUT>\n"
        ),
        "{}",
        ran.stderr
    );

    let ran = run(&["parts", "-h"]);
    assert_eq!(ran.code, 0);
    assert!(
        ran.stdout
            .starts_with("list an assembly's parts\n\nUsage: specify-pmi parts <STEP>\n"),
        "{}",
        ran.stdout
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a usage capture (`cli/usage/NN args`) must show under clap: exit 2 with stdout empty
/// and stderr naming what is missing, unknown or invalid; or help (exit 0) on stdout.
fn usage_or_help(case: &str, exit: i64, stdout: &str, stderr: &str) {
    let named: &[&str] = match &case[..2] {
        // No command: the commands are listed.
        "00" => &["Usage: specify-pmi <COMMAND>", "parts", "write"],
        "01" => &["unrecognized subcommand 'bogus'"],
        "02" | "06" => &["required arguments were not provided", "--output", "<STEP>"],
        "03" => &["required arguments were not provided", "--output"],
        "04" => &["a value is required for '--output <OUTPUT>'"],
        "05" => &["invalid value 'q' for '--part <PART>'"],
        "07" => &["required arguments were not provided", "<STEP>"],
        "08" => &["required arguments were not provided", "<ANALYSIS>"],
        "09" => &["required arguments were not provided", "<EDITS.json>"],
        // One JSON after STEP is an edit list (proposal specify-0.3.0, use case 7): no longer a
        // usage error, so the missing file is what is reported.
        "10" => &["a.json: No such file"],
        "11" => &["whole-intent write was removed"],
        "12" => &["unexpected argument 'extra'"],
        "13" => &["unexpected argument 'z'"],
        "14" => &["unexpected argument '--bogus'"],
        "15" => &["a value is required for '--intent <INTENT>'"],
        _ => &[],
    };
    if exit == 2 {
        assert_eq!(stdout, "", "{case}: a usage error prints nothing on stdout");
        assert!(
            !named.is_empty(),
            "{case}: no expectation for this usage error"
        );
        for text in named {
            assert!(stderr.contains(text), "{case}: {text:?} not in {stderr}");
        }
    } else {
        assert_eq!((exit, stderr), (0, ""), "{case}");
        let command = case[3..].split(' ').find(|a| !a.starts_with('-'));
        let usage = match command {
            Some(c) => format!("Usage: specify-pmi {c}"),
            None => "Usage: specify-pmi <COMMAND>".to_string(),
        };
        assert!(stdout.contains(&usage), "{case}: {stdout}");
        assert!(stdout.contains("-h, --help"), "{case}: {stdout}");
    }
}

/// Parsing corners no captured case covers, each with the exit code and a text stderr must
/// hold. These pinned argparse's wording until the port to clap (Python specify-core is
/// retired; help and usage errors are clap's): the usage errors keep exit 2 and name the
/// argument at fault.
#[test]
fn arguments_are_parsed_as_clap_parses_them() {
    let dir = scratch("argparse");
    let pin = "tests/fixtures/parts/pin.step";
    let mesh = s(&dir.join("m.json"));
    let cases: &[(&[&str], i32, &str)] = &[
        // Unknown options with non-ASCII characters (an en dash, a non-ASCII letter).
        (
            &["analyse", pin, "-\u{2013}output", "x.json"],
            2,
            "error: unexpected argument '-\u{2013}' found",
        ),
        (
            &["analyse", "-\u{e9}"],
            2,
            "error: unexpected argument '-\u{e9}' found",
        ),
        // A flag takes no value.
        (
            &["write", "a", "b", "c", "--accept-defaults=1", "-o", "x"],
            2,
            "error: unexpected value '1' for '--accept-defaults' found; no more were expected",
        ),
        // Pairs given after `--` are still counted: an odd number is refused.
        (
            &["write", "S", "-o", "OUT", "a", "b", "--", "c"],
            2,
            "whole-intent write was removed",
        ),
        // Positionals continue after an option (argparse left `c d`, and below `b`,
        // unrecognised here, a run at a time): the pairs are `a b c d` (and `a b`), so the
        // first analysis is read and found missing.
        (
            &["write", "S", "a", "b", "-o", "OUT", "c", "d"],
            2,
            "whole-intent write was removed",
        ),
        (
            &["write", "S", "a", "-o", "OUT", "b"],
            2,
            "whole-intent write was removed",
        ),
        // A value that looks like a negative number is a value, for every option and
        // positional (as on main, where these reached the files and failed reading them);
        // other `-` arguments stay unknown options.
        (
            &["diff", "A", "B", "-o", "D", "--title", "-5"],
            1,
            "error: cannot read STEP file A",
        ),
        (
            &["analyse", "X", "-o", "-1"],
            1,
            "error: cannot read STEP file X",
        ),
        (
            &["analyse", "X", "--output", "-1"],
            1,
            "error: cannot read STEP file X",
        ),
        (&["parts", "-1"], 1, "error: cannot read STEP file -1"),
        (
            &["write", "S", "-1", "-2", "-o", "O"],
            2,
            "whole-intent write was removed",
        ),
        (
            &["analyse", "X", "-x"],
            2,
            "error: unexpected argument '-x' found",
        ),
        // The last value of a repeated option wins, as argparse's `store` does.
        (
            &["analyse", pin, "-o", "x", "-o", "y", "--part", "z"],
            2,
            "error: invalid value 'z' for '--part <PART>'",
        ),
    ];
    for (args, code, text) in cases {
        let got = run(args);
        assert_eq!(got.code, *code, "{args:?}: {}", got.stderr);
        assert!(got.stderr.contains(text), "{args:?}: {}", got.stderr);
    }
    // `-o=VALUE` is `-o VALUE`, a long option may be shortened to a prefix only it has, and a
    // repeated option's last value is the one used.
    let got = run(&["mesh", pin, &format!("-o={mesh}")]);
    assert_eq!(got.code, 0, "{}", got.stderr);
    assert!(dir.join("m.json").is_file());
    let first = s(&dir.join("first.json"));
    let last = s(&dir.join("last.json"));
    let got = run(&[
        "mesh",
        pin,
        "--out",
        &first,
        "--pa",
        "0",
        &format!("--output={last}"),
    ]);
    assert_eq!(got.code, 0, "{}", got.stderr);
    assert!(dir.join("last.json").is_file() && !dir.join("first.json").exists());
    // `--part` takes a negative number, refused when the file is read (exit 1, as Python).
    let got = run(&["mesh", pin, "-o", &mesh, "--part", "-1"]);
    assert_eq!(got.code, 1, "{}", got.stderr);
    assert!(got.stderr.contains("there is no part -1"), "{}", got.stderr);
}

/// `-h` gives help on stdout (exit 0) for every command, and `completions` a script that
/// names the program for each shell.
#[test]
fn every_command_has_help_and_completions_are_printed() {
    for command in [
        "parts",
        "analyse",
        "serve",
        "mesh",
        "questions",
        "write",
        "diff",
        "open",
        "bom",
        "completions",
    ] {
        for flag in ["-h", "--help"] {
            let got = run(&[command, flag]);
            assert_eq!((got.code, got.stderr.as_str()), (0, ""), "{command} {flag}");
            assert!(
                got.stdout
                    .contains(&format!("Usage: specify-pmi {command}")),
                "{command} {flag}: {}",
                got.stdout
            );
        }
    }
    let got = run(&["-h"]);
    assert_eq!(got.code, 0);
    for command in ["parts", "write", "diff", "open", "bom", "completions"] {
        assert!(
            got.stdout.contains(&format!("  {command} ")),
            "{}",
            got.stdout
        );
    }
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let got = run(&["completions", shell]);
        assert_eq!((got.code, got.stderr.as_str()), (0, ""), "{shell}");
        assert!(got.stdout.contains("specify-pmi"), "{shell}");
        assert!(
            got.stdout.contains("--check") || got.stdout.contains("check"),
            "{shell}"
        );
    }
    // Every subcommand bash selects has its case in the script (clap_complete's names for them
    // disagree for a program name with a `-`; src/cli.rs mends them).
    let bash = run(&["completions", "bash"]).stdout;
    let selected: BTreeSet<&str> = bash
        .lines()
        .filter_map(|l| l.trim().strip_prefix("cmd=\""))
        .filter_map(|l| l.strip_suffix('"'))
        .filter(|cmd| !cmd.is_empty())
        .collect();
    assert!(selected.len() > 10, "{selected:?}");
    for cmd in selected {
        assert!(
            bash.contains(&format!("\n        {cmd})\n")),
            "no case for {cmd}"
        );
    }
    let got = run(&["completions", "tcsh"]);
    assert_eq!(got.code, 2);
    assert!(
        got.stderr.contains("invalid value 'tcsh'"),
        "{}",
        got.stderr
    );
}

/// Non-ASCII is escaped in the files (as Python's `json.dumps` writes them) and kept on stdout
/// (Python's `ensure_ascii=False`): the plate's interpret call-out has `×` and `⌀`, and the
/// questions' prompts and the intent's general tolerance have `±`.
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
    save(&q, &json!({"fill":{"part.material":"6061 Al ± anodized"}}));
    let out = dir.join("o.step");
    let ran = run(&["write", &s(&plate), &s(&q), "-o", &s(&out)]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(ran.stdout.contains("6061 Al ± anodized"));
    let analysed = dir.join("written.json");
    assert_eq!(run(&["analyse", &s(&out), "-o", &s(&analysed)]).code, 0);
    let text = std::fs::read_to_string(analysed).unwrap();
    assert!(text.is_ascii() && text.contains("\\u00b1"), "{text}");
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
                let analysis = serde_json::from_slice(&std::fs::read(&a).unwrap()).unwrap();
                save(
                    &q,
                    &common::approve_edits(&analysis, &object(&json!({"part.material":"steel"}))),
                );
                let o = dir.join(format!("o{tag}.step"));
                let ran = run(&["write", &step, &s(&q), "-o", &s(&o)]);
                assert_eq!(ran.code, 0, "{}", ran.stderr);
                seen.push((
                    "report".into(),
                    ran.stdout.replace(&s(&o), "OUT").into_bytes(),
                ));
                seen.push(("written".into(), std::fs::read(&o).unwrap()));
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

// Workflow regression: a groove splits the authored bottom datum into two faces.
fn split_plate(which: &str) -> PathBuf {
    common::fixtures().join(format!("revisions/plate_split.{which}.step.gz"))
}

fn split_answers() -> Value {
    json!({"part.material":"6061-T6", "datum.A":[4], "datum.B":[1], "datum.C":[0]})
}

/// The split plate's answers file for *new* (written to `dir/changes/answers.json`), with
/// *new*'s face evidence: on the old plate itself, as saved; on the new plate, as an earlier
/// `diff` carried it, its bottom datum dropped. (Diff no longer writes answers: ADR P11.)
fn split_diff(dir: &Path, new: &Path) -> Value {
    let loaded = specify_pmi::part::load(new, None).unwrap();
    let evidence = specify_pmi::resume::evidence(&loaded).unwrap();
    let mut answers = split_answers();
    if new == split_plate("new") {
        answers = json!({"part.material": "6061-T6", "_specify": {
            "evidence": evidence,
            "unresolved": {"datum.A": SPLIT_DROPPED},
        }});
    } else {
        answers["_specify"] = json!({"evidence": evidence, "unresolved": {}});
    }
    std::fs::create_dir_all(dir.join("changes")).unwrap();
    save(&dir.join("changes/answers.json"), &answers);
    answers
}

/// Why the split plate's datum A could not be carried.
const SPLIT_DROPPED: &str = "face 4 has no match in the new part";

fn split_write(dir: &Path, step: &Path, answers: &Path) -> Run {
    run(&[
        "write",
        &s(step),
        &s(answers),
        "-o",
        &s(&dir.join("out.step")),
    ])
}

#[test]
fn dropped_carried_datum_cannot_be_bulk_defaulted_and_explicit_answer_clears_it() {
    let dir = scratch("dropped-carried-datum");
    let new = split_plate("new");
    let mut answers = split_diff(&dir, &new);
    assert!(answers.get("datum.A").is_none());
    let reason = SPLIT_DROPPED;
    let analysis = dir.join("questions.analysis.json");
    save(&analysis, &api::analyse_file(&new, None).unwrap());
    let asked = run(&[
        "questions",
        &s(&analysis),
        "--answers",
        &s(&dir.join("changes/answers.json")),
    ]);
    assert_eq!(asked.code, 0, "{}", asked.stderr);
    let questions: Value = serde_json::from_str(&asked.stdout).unwrap();
    let q = questions
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"] == "datum.A")
        .unwrap();
    assert_eq!(q["default"], json!(null));
    assert_eq!(q["attention"], "required");
    assert_eq!(
        q["reason"],
        format!("carried answer could not be applied: {reason}")
    );
    let done = split_write(&dir, &new, &dir.join("changes/answers.json"));
    assert_eq!(done.code, 2, "{}", done.stderr);
    assert!(
        done.stderr.contains(&format!(
            "incomplete: datum.A (carried answer could not be applied: {reason})"
        )),
        "{}",
        done.stderr
    );
    assert!(!dir.join("out.step").exists());
    answers["datum.A"] = json!([4, 8]);
    save(&dir.join("resolved.json"), &answers);
    let done = split_write(&dir, &new, &dir.join("resolved.json"));
    assert_eq!(done.code, 0, "{}", done.stderr);
    let analysis = api::analyse_file(&dir.join("out.step"), None).unwrap();
    let datum = analysis["existing"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "datum" && item["type"] == "A")
        .unwrap();
    assert_eq!(datum["faces"], json!([4, 8]));
}

#[test]
fn specify_written_answers_verify_unchanged_geometry_and_refuse_stale_indices() {
    let dir = scratch("stale-answer-evidence");
    let old = split_plate("old");
    split_diff(&dir, &old);
    let answers = dir.join("changes/answers.json");
    let done = split_write(&dir, &old, &answers);
    assert_eq!(done.code, 0, "{}", done.stderr);
    std::fs::remove_file(dir.join("out.step")).unwrap();
    let done = split_write(&dir, &split_plate("new"), &answers);
    assert_eq!(done.code, 2, "{}", done.stderr);
    assert!(done.stderr.contains("stale answers"), "{}", done.stderr);
    assert!(done.stderr.contains(&s(&answers)), "{}", done.stderr);
    assert!(done.stderr.contains("another revision"), "{}", done.stderr);
    assert!(!dir.join("out.step").exists());
}

#[test]
fn plain_maps_have_no_geometry_history_and_remain_compatible() {
    let dir = scratch("plain-answer-map");
    save(&dir.join("plain.json"), &split_answers());
    for which in ["old", "new"] {
        let done = split_write(&dir, &split_plate(which), &dir.join("plain.json"));
        // The same map could be newly handwritten for either part: no provenance to check.
        assert_eq!(done.code, 0, "{}", done.stderr);
    }
}

#[test]
fn mesh_max_deviation_mm_is_measured_and_validated() {
    let input = common::fixtures().join("parts/mesh_sphere.step");
    let run = |limit: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_specify-pmi"))
            .arg("mesh")
            .arg(&input)
            .args(["-o", "-", "--max-deviation-mm", limit])
            .output()
            .unwrap()
    };
    let output = run("0.01");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mesh: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(mesh["max_deviation_mm"], 0.01);
    assert!(mesh["achieved_deviation_mm"].as_f64().unwrap() <= 0.01);
    for invalid in ["0", "-1", "NaN", "inf", "-inf"] {
        let output = run(invalid);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("positive finite"));
        assert!(output.stdout.is_empty());
    }
}

/// #228: `write` writes what is answered while questions are open, and lists them in
/// `missing` (ADR P11: gaps are not a refusal). The half-shell of the issue, its general
/// tolerance and material answered, its holes, datums and edges still open.
#[test]
fn issue_228_write_writes_what_is_answered_and_lists_the_gaps() {
    let dir = scratch("issue228");
    let step = common::fixtures().join("issue228-half-shell.step.gz");
    let q = dir.join("q.json");
    save(
        &q,
        &json!({"fill": {
            "part.material": "6061 Al",
            "part.coating": "Anodize blue",
            "part.general_tolerance": ".XX ±.01, .XXX ±.005, fractions ±1/64, angles ±1°",
        }}),
    );
    let output = dir.join("o.step");
    let ran = run(&["write", &s(&step), &s(&q), "-o", &s(&output)]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let report: Value = serde_json::from_str(&ran.stdout).unwrap();
    let written: Vec<&str> = report["written"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect();
    assert!(written.contains(&"material 6061 Al"), "{written:?}");
    assert!(
        written.contains(&"general tolerance .XX ±.01 · .XXX ±.005 · ±1/64 · angles ±1°"),
        "{written:?}"
    );
    let missing = report["missing"].as_array().unwrap();
    assert!(missing.contains(&json!("datum.A")), "{missing:?}");
    assert!(
        missing.contains(&json!("hole.function:holes:96.129")),
        "{missing:?}"
    );
    assert!(
        missing.contains(&json!("part.surface_finish")),
        "{missing:?}"
    );
    let reopened = api::analyse_file(&output, None).unwrap();
    assert_eq!(reopened["part"]["material"], "6061 Al");
    assert!(
        reopened["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "part.surface_finish" && row["state"] == "gap")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// ADR P11: `write` never replaces a general tolerance the file states. A file stating ISO
/// 2768-f, an analysis without the feature table (an older one, so the rules do not hold the
/// answer back), and ISO 2768-c answered: the writer refuses, and nothing is written.
#[test]
fn write_refuses_a_different_general_tolerance_than_the_file_states() {
    let dir = scratch("stated-general");
    let q = dir.join("edits.json");
    save(
        &q,
        &json!({"fill": {"part.material": "steel", "part.general_tolerance": "ISO 2768-f"}}),
    );
    let first = dir.join("f.step");
    let ran = run(&["write", &s(&part("plate")), &s(&q), "-o", &s(&first)]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    save(&q, &json!({"fill":{"part.general_tolerance":"ISO 2768-c"}}));
    let second = dir.join("c.step");
    let ran = run(&["write", &s(&first), &s(&q), "-o", &s(&second)]);
    assert_eq!(ran.code, 2, "{}", ran.stdout);
    assert!(
        ran.stderr
            .contains("the file already states \"ISO 2768-f\" for part.general_tolerance"),
        "{}",
        ran.stderr
    );
    assert!(!second.exists());
    assert_eq!(
        api::analyse_file(&first, None).unwrap()["part"]["general_tolerance"],
        "ISO 2768-f"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The `specify-core` alias (all of 0.3.x): a copy of `specify-pmi` named `specify-core`, as the
/// release archives ship, gives the same output plus one notice on stderr and nothing more on
/// stdout. (The crate has no second binary: `cargo install` would refuse it beside an installed
/// `specify-core` crate; cargo users get the alias from the 0.3.1 `specify-core` forwarder.)
#[test]
fn specify_core_alias_prints_a_notice_on_stderr_only() {
    let notice = "specify-core is now specify-pmi; this alias will be removed in 0.4\n";
    let pmi = Command::new(env!("CARGO_BIN_EXE_specify-pmi"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(pmi.status.success() && pmi.stderr.is_empty());
    let dir = scratch("alias-copy");
    let copy = dir.join(format!("specify-core{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(env!("CARGO_BIN_EXE_specify-pmi"), &copy).unwrap();
    {
        let exe = copy;
        let out = Command::new(&exe).arg("--help").output().unwrap();
        assert!(out.status.success(), "{exe:?}");
        assert_eq!(out.stdout, pmi.stdout, "{exe:?}");
        assert_eq!(String::from_utf8_lossy(&out.stderr), notice, "{exe:?}");
        let bad = Command::new(&exe).arg("no-such-command").output().unwrap();
        let pmi_bad = Command::new(env!("CARGO_BIN_EXE_specify-pmi"))
            .arg("no-such-command")
            .output()
            .unwrap();
        assert_eq!(bad.status.code(), pmi_bad.status.code(), "{exe:?}");
        assert!(bad.stdout.is_empty() && String::from_utf8_lossy(&bad.stderr).starts_with(notice));
    }
}
