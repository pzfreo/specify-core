//! The cross-engine check: what each engine reads of the other's files, so Draftwright Specify
//! can move a part between engines.
//!
//! - (a) `rust_written_files_are_what_the_writer_writes_today`: the committed Rust-written
//!   files (`tests/fixtures/cross_engine/<case>/rust.step.gz`, or `rust.refused.txt` for a
//!   case the example or the writer refuses) are still exactly what `examples/write_fixtures.rs` produces,
//!   byte for byte. A deliberate change is regenerated with that example (its doc comment says
//!   how), together with Python's read-back of the new files.
//! - (b) Python's read-back of each Rust-written file (`python_readback.json.gz`, captured by
//!   `tools/capture.py cross-engine`: specify-core's `read_existing`, `part_settings` and
//!   `resume.answers_for` per part, each in a process of its own) against the intent as
//!   written. The intent as written is the Rust engine's read-back of the same file, which the
//!   writer verified against the intent when it wrote it, with each fit given the limits its
//!   ISO 286 class states (the writer writes a fit as its class, and refuses an intent whose
//!   limits are not the class's). So every datum, size, location, tolerance, material, general
//!   tolerance and stored answer OpenCascade reads differently, or not at all, is a difference.
//!   Where Python does not resume the stored answers, the capture's resume evidence (the
//!   payload Python's pattern finds, and the faces Python fingerprints) says why, and the
//!   reason is part of the difference's path.
//! - (c) Each file Python wrote (`tests/fixtures/oracle/**/written.step.gz`) read and resumed
//!   by the Rust engine, against Python's own read-back of it (`readback.json.gz`).
//!
//! Differences in how the two readers read a file's own PMI (NIST's, in (b); and in (c) the
//! presentation of specify-core's own callouts) are the existing-PMI area's
//! (`known_differences/existing.json`) and are filtered through it. In (b) only the NIST files
//! have PMI of their own, so a synthetic file is not filtered: all OpenCascade reads of it is
//! what the Rust writer wrote. The rest must each
//! have a verdict in `known_differences/cross_engine.json`, checked once by
//! `the_engines_read_each_others_files`.

// The example's case list and write, and through it the oracle harness (`tests/common`).
#[path = "../examples/write_fixtures.rs"]
mod write_fixtures;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use specify_core::existing::{part_settings, read_existing};
use specify_core::features::describe_faces;
use specify_core::part::{LoadedPart, load};
use specify_core::pyfmt::round_half_even;
use specify_core::resume::{answers_for, fingerprint, stored};
use write_fixtures::common::{
    self, Difference, Tolerances, cases, check_known, compare, explained, is_error,
};

fn gunzip(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(&bytes[..])
        .read_to_end(&mut out)
        .unwrap_or_else(|e| panic!("{}: not gzip: {e}", path.display()));
    out
}

/// The committed Rust-written file of `case`, decompressed to a temporary file (by its
/// sha256, so concurrent tests share it); None when the writer refused the case.
fn rust_written(case: &str) -> Option<PathBuf> {
    let gz = write_fixtures::case_dir(case).join("rust.step.gz");
    gz.is_file()
        .then(|| common::decompressed(&gz, &common::sha256(&gunzip(&gz))))
}

// ---------------------------------------------------------------------------------------------
// (a) The committed files are today's writer's
// ---------------------------------------------------------------------------------------------

#[test]
fn rust_written_files_are_what_the_writer_writes_today() {
    let scratch = write_fixtures::scratch();
    let cases = write_fixtures::cases();
    assert!(!cases.is_empty(), "no synthetic or NIST oracle cases");
    let mut stale = Vec::new();
    for case in &cases {
        let dir = write_fixtures::case_dir(&case.name);
        let output = scratch.join(format!("{}.step", case.name.replace(['/', '#'], "_")));
        let (step, refused) = (dir.join("rust.step.gz"), dir.join("rust.refused.txt"));
        match (write_fixtures::write(case, &output), step.is_file()) {
            (Ok(bytes), true) if gunzip(&step) == bytes => {}
            (Ok(_), true) => stale.push(format!("{}: the writer's bytes changed", case.name)),
            (Ok(_), false) => stale.push(format!("{}: written now, not committed", case.name)),
            (Err(why), false)
                if std::fs::read_to_string(&refused).ok().as_deref()
                    == Some(format!("{why}\n").as_str()) => {}
            (Err(why), _) => stale.push(format!("{}: refused now: {why}", case.name)),
        }
        // Exactly one of the two, never both.
        if step.is_file() && refused.is_file() {
            stale.push(format!(
                "{}: both rust.step.gz and rust.refused.txt",
                case.name
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    // No committed case the example no longer writes.
    let names: Vec<&str> = cases.iter().map(|c| c.name.as_str()).collect();
    for dir in fixture_case_dirs(&write_fixtures::fixtures_dir()) {
        let name = relative_name(&dir);
        if !names.contains(&name.as_str()) {
            stale.push(format!("{name}: committed, but not a case of the example"));
        }
    }
    assert!(
        stale.is_empty(),
        "regenerate with `cargo run --release --example write_fixtures` and recapture \
         Python's read-back (examples/write_fixtures.rs says how):\n  {}",
        stale.join("\n  ")
    );
}

/// The case directories under `root`: those holding a `rust.step.gz` or `rust.refused.txt`.
fn fixture_case_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if !path.is_dir() {
            continue;
        }
        if path.join("rust.step.gz").is_file() || path.join("rust.refused.txt").is_file() {
            out.push(path.clone());
        }
        out.extend(fixture_case_dirs(&path));
    }
    out.sort();
    out
}

fn relative_name(dir: &Path) -> String {
    dir.strip_prefix(write_fixtures::fixtures_dir())
        .expect("under the fixtures")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

// ---------------------------------------------------------------------------------------------
// Read-backs, in the oracle's layout
// ---------------------------------------------------------------------------------------------

/// What the Rust engine reads back of `loaded` (`readback.json`'s layout for one part).
fn readback_of(loaded: &LoadedPart) -> Value {
    let existing = match read_existing(loaded) {
        Ok(e) => serde_json::to_value(e).unwrap(),
        Err(e) => json!({"error": "ExistingError", "message": e.to_string()}),
    };
    let part = match part_settings(loaded) {
        Ok(p) => Value::Object(p),
        Err(e) => json!({"error": "ExistingError", "message": e.to_string()}),
    };
    let resume = match answers_for(loaded) {
        Ok(Some(a)) => Value::Object(a),
        Ok(None) => Value::Null,
        Err(e) => json!({"error": "FeatureError", "message": e.to_string()}),
    };
    json!({"existing": existing, "part": part, "resume": resume})
}

/// Part `k` of the file at `path` of `count` parts, loaded as Python's `load(path, part=k)`.
fn load_part(path: &Path, k: usize, count: usize) -> Result<LoadedPart, String> {
    load(path, (count > 1).then_some(k)).map_err(|e| e.to_string())
}

/// Each existing-PMI item keyed by kind, type and faces (and its occurrence, for a repeat), as
/// the existing-PMI sweep keys them (`tests/existing.rs`), so the two lists pair item by item
/// and the reading differences that sweep explains are recognised here.
fn keyed(items: &Value) -> Value {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let out: Vec<Value> = items
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            let base = format!(
                "{}|{}|{}|{}",
                item["kind"].as_str().unwrap_or_default(),
                item["type"].as_str().unwrap_or_default(),
                item["faces"],
                item.get("faces2").map_or(String::new(), Value::to_string)
            );
            let n = seen.entry(base.clone()).or_default();
            *n += 1;
            let mut item = item.clone();
            item.as_object_mut()
                .unwrap()
                .insert("key".into(), json!(format!("{base}#{n}")));
            item
        })
        .collect();
    Value::Array(out)
}

/// The two read-backs of one part compared: `existing` paired by key, `part` and `resume` as
/// values. The case is the oracle case's name, the path unprefixed, so the existing-PMI area's
/// entries can be applied before the caller prefixes it.
fn compare_readback(name: &str, python: &Value, rust: &Value) -> Vec<Difference> {
    let tolerances = Tolerances::default()
        .align("existing", "key")
        .with("existing*", 1e-9, 1e-9);
    let view = |v: &Value| {
        let existing = if v["existing"].is_array() {
            keyed(&v["existing"])
        } else {
            v["existing"].clone()
        };
        json!({"existing": existing, "part": v["part"], "resume": v["resume"]})
    };
    compare(name, &view(python), &view(rust), &tolerances)
}

/// The ISO 286 limits each fit of `intent` means, by its sorted faces.
fn fits(intent: &Value) -> Vec<(Vec<u64>, f64, f64)> {
    let mut out = Vec::new();
    for r in intent["requirements"].as_array().into_iter().flatten() {
        let (Some(fit), Some(nominal)) = (r["fit"].as_str(), r["nominal"].as_f64()) else {
            continue;
        };
        if r["kind"] != "size" || fit.is_empty() {
            continue;
        }
        if let Ok((upper, lower)) = specify_core::standards::fit_limits(fit, nominal) {
            out.push((sorted_faces(&r["faces"]), upper, lower));
        }
    }
    out
}

fn sorted_faces(faces: &Value) -> Vec<u64> {
    let mut out: Vec<u64> = faces
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .collect();
    out.sort_unstable();
    out
}

/// A Rust read-back with each size that reads back as a class alone (no limits, on the faces
/// of one of `fits`) given the limits its class states: the intent as written.
fn with_class_limits(mut rust: Value, fits: &[(Vec<u64>, f64, f64)]) -> Value {
    for item in rust["existing"].as_array_mut().into_iter().flatten() {
        if item["kind"] != "dimension"
            || !item["type"].as_str().unwrap_or("").starts_with("Size_")
            || !item["upper"].is_null()
            || !item["lower"].is_null()
        {
            continue;
        }
        let faces = sorted_faces(&item["faces"]);
        if let Some((_, upper, lower)) = fits.iter().find(|(f, _, _)| *f == faces) {
            item["upper"] = json!(upper);
            item["lower"] = json!(lower);
        }
    }
    rust
}

fn prefixed(diffs: Vec<Difference>, at: &str) -> Vec<Difference> {
    diffs
        .into_iter()
        .map(|mut d| {
            d.path = if d.path.is_empty() || d.path.starts_with('[') {
                format!("{at}{}", d.path)
            } else {
                format!("{at}.{}", d.path)
            };
            d
        })
        .collect()
}

fn difference(case: &str, path: String, python: Value, rust: Value) -> Difference {
    Difference {
        case: case.to_owned(),
        path,
        python: Some(python),
        rust: Some(rust),
    }
}

// ---------------------------------------------------------------------------------------------
// (b) Python reading the Rust-written files
// ---------------------------------------------------------------------------------------------

/// The differences between Python's read-back of each Rust-written file and the intent as
/// written, and the resume tally (`resumed`, by outcome). Where OpenCascade crashed on the
/// file, the capture's probe (`python_readback_without_symbols.json.gz`: the same file without
/// the datum symbols' relationship, upstream need U15) is compared too, under its own path, to
/// show what OpenCascade reads of the semantic PMI apart from the crash.
fn python_reads_rust(resumed: &mut BTreeMap<String, usize>) -> Vec<Difference> {
    let mut out = Vec::new();
    for case in write_fixtures::cases() {
        let Some(step) = rust_written(&case.name) else {
            // The writer refused it (checked by (a)): nothing crosses, which needs a verdict too.
            let refused = write_fixtures::case_dir(&case.name).join("rust.refused.txt");
            let why = std::fs::read_to_string(&refused).unwrap_or_default();
            let python = common::case(&case.name)
                .artifact("write")
                .unwrap_or(Value::Null);
            out.push(difference(
                &case.name,
                "rust_written[refused]".into(),
                python,
                json!(why.trim_end()),
            ));
            continue;
        };
        let dir = write_fixtures::case_dir(&case.name);
        let python_file = dir.join("python_readback.json.gz");
        assert!(
            python_file.is_file(),
            "{}: no python_readback.json.gz; run tools/capture.py cross-engine",
            case.name
        );
        let python = common::read_json_gz(&python_file);
        let meta = common::read_json_gz(&dir.join("python_meta.json.gz"));
        let sha = common::sha256(&std::fs::read(&step).unwrap());
        assert_eq!(
            meta["rust_step_sha256"].as_str(),
            Some(sha.as_str()),
            "{}: Python's read-back was captured from other bytes; recapture it",
            case.name
        );
        let count = case.intents.len();
        let rust: Vec<Value> = (0..count)
            .map(|k| match load_part(&step, k, count) {
                Ok(l) => with_class_limits(readback_of(&l), &fits(&case.intents[k])),
                Err(e) => json!({"error": "LoadError", "message": e}),
            })
            .collect();
        let read = WrittenFile {
            case: &case.name,
            step: &step,
            rust: &rust,
        };
        let probe = dir.join("python_readback_without_symbols.json.gz");
        if let Value::Array(_) = python {
            assert!(
                !probe.exists(),
                "{}: a probe of a file Python read",
                case.name
            );
            let diffs = read.compare(&python, &meta["resume_evidence"], Some(resumed));
            out.extend(prefixed(diffs, "python_readback"));
            continue;
        }
        // OpenCascade could not read the file at all (a crash or a refusal): one difference,
        // the reason in its path, against everything the intent wrote.
        let why = python["message"].as_str().unwrap_or("").to_owned();
        *resumed.entry("file not read".into()).or_default() += count;
        out.push(difference(
            &case.name,
            format!("python_readback[{why}]"),
            python.clone(),
            json!(rust),
        ));
        if probe.is_file() {
            let python = common::read_json_gz(&probe);
            let diffs = read.compare(&python, &meta["probe_resume_evidence"], None);
            out.extend(prefixed(diffs, "python_readback_without_symbols"));
        }
    }
    out
}

/// One Rust-written file and the intent as written in it (the Rust read-back of each part).
struct WrittenFile<'a> {
    case: &'a str,
    step: &'a Path,
    rust: &'a [Value],
}

impl WrittenFile<'_> {
    /// Python's read-back of the file (`readback.json`'s layout) against the intent as
    /// written, part by part, with why Python did not resume where it did not (`evidence`,
    /// per part). A NIST file's are filtered through the existing-PMI area (the file's own
    /// PMI); a synthetic file has none, so its every difference stands. Paths are unprefixed.
    fn compare(
        &self,
        python: &Value,
        evidence: &Value,
        resumed: Option<&mut BTreeMap<String, usize>>,
    ) -> Vec<Difference> {
        let count = self.rust.len();
        let Value::Array(back) = python else {
            let why = python["message"].as_str().unwrap_or("").to_owned();
            return vec![difference(
                self.case,
                format!("[{why}]"),
                python.clone(),
                json!(self.rust),
            )];
        };
        if back.len() != count {
            return vec![difference(
                self.case,
                "parts".into(),
                json!(back.len()),
                json!(count),
            )];
        }
        let mut tally: BTreeMap<String, usize> = BTreeMap::new();
        let mut out = Vec::new();
        let evidence = evidence.as_array().cloned().unwrap_or_default();
        for (k, (python, rust)) in back.iter().zip(self.rust).enumerate() {
            let name = if count > 1 {
                format!("{}#{k}", self.case)
            } else {
                self.case.to_owned()
            };
            let mut diffs = compare_readback(&name, python, rust);
            for d in &mut diffs {
                if d.path == "resume" && d.python == Some(Value::Null) {
                    let why = python_resume_failure(self.step, k, count, evidence.get(k));
                    d.path = format!("resume[python did not resume: {why}]");
                }
            }
            let outcome = match (&python["resume"], &rust["resume"]) {
                (p, _) if is_error(p) => format!("python failed: {}", p["message"]),
                (Value::Null, Value::Null) => "neither engine resumed".into(),
                (Value::Null, _) => "python did not resume".into(),
                (p, r) if p == r => "resumed, same answers".into(),
                _ => "resumed, different answers".into(),
            };
            *tally.entry(outcome).or_default() += 1;
            out.extend(diffs);
        }
        if let Some(resumed) = resumed {
            for (outcome, n) in tally {
                *resumed.entry(outcome).or_default() += n;
            }
        }
        if self.case.starts_with("nist/") {
            explained(&["existing"], &out)
        } else {
            out
        }
    }
}

/// Why Python's `answers_for` gave None for part `k` of the Rust-written `step`, from the
/// capture's evidence (what Python's pattern found stored, its face count and the faces it
/// fingerprints) checked against this engine's own reading of the file.
fn python_resume_failure(step: &Path, k: usize, count: usize, evidence: Option<&Value>) -> String {
    let Some(evidence) = evidence.filter(|e| !is_error(e)) else {
        return "no evidence captured".into();
    };
    let Ok(loaded) = load_part(step, k, count) else {
        return "the Rust engine cannot load the part".into();
    };
    let python_stored = &evidence["stored"];
    if python_stored.is_null() {
        return "Python's pattern finds no answers stored".into();
    }
    // What Python found must be what this engine stored (and reads back).
    let ours = stored(&loaded.bytes).remove(&k).map(Value::Object);
    if ours.as_ref() != Some(python_stored) {
        return "Python found a payload other than the one stored".into();
    }
    if python_stored["face_count"] != evidence["face_count"] {
        return "face count differs".into();
    }
    if python_stored["faces"] != json!(fingerprint(&loaded).unwrap_or_default()) {
        return "the stored fingerprint is not this engine's".into();
    }
    faces_gap(&loaded, evidence)
}

/// Why the Rust engine's `answers_for` gave None for part `k` of the Python-written `step`,
/// against what Python fingerprints in it (`python_written_evidence.json.gz`).
fn rust_resume_failure(loaded: &LoadedPart, k: usize, evidence: Option<&Value>) -> String {
    let Some(evidence) = evidence.filter(|e| !is_error(e)) else {
        return "no evidence captured".into();
    };
    let Some(ours) = stored(&loaded.bytes).remove(&k).map(Value::Object) else {
        return "the Rust engine finds no answers stored".into();
    };
    if ours != evidence["stored"] {
        return "the Rust engine found a payload other than Python's".into();
    }
    if ours["face_count"] != json!(loaded.binding.face_count) {
        return "face count differs".into();
    }
    // Python resumes its own file: the stored fingerprint is the hash of the faces captured.
    if ours["faces"] != evidence["fingerprint"] {
        return "the stored fingerprint is not Python's".into();
    }
    faces_gap(loaded, evidence)
}

/// How the faces Python fingerprints (`evidence.faces`: kind and centroid rounded to 3, in
/// order) differ from this engine's, which `resume::fingerprint` hashes. A zero whose sign
/// differs is equal by value but not in Python's JSON text (`-0.0`), and so in the hash.
fn faces_gap(loaded: &LoadedPart, evidence: &Value) -> String {
    let rust: Vec<Value> = describe_faces(loaded)
        .map(|faces| {
            faces
                .iter()
                .map(|f| {
                    let c: Vec<f64> = f.centroid.iter().map(|&c| round_half_even(c, 3)).collect();
                    json!([f.kind, c])
                })
                .collect()
        })
        .unwrap_or_default();
    let python = evidence["faces"].as_array().cloned().unwrap_or_default();
    if python.len() != rust.len() {
        return "face lists differ in length".into();
    }
    let (mut by_value, mut by_sign) = (0, 0);
    for (p, r) in python.iter().zip(&rust) {
        if p[0] != r[0] {
            by_value += 1;
            continue;
        }
        let pc = p[1].as_array().cloned().unwrap_or_default();
        let rc = r[1].as_array().cloned().unwrap_or_default();
        for (a, b) in pc.iter().zip(&rc) {
            let (a, b) = (
                a.as_f64().unwrap_or(f64::NAN),
                b.as_f64().unwrap_or(f64::NAN),
            );
            if a != b {
                by_value += 1;
            } else if a.is_sign_negative() != b.is_sign_negative() {
                by_sign += 1;
            }
        }
    }
    match (by_value, by_sign) {
        (0, 0) => "same faces, yet the fingerprints differ".into(),
        (0, _) => "fingerprint differs only in signs of zero".into(),
        _ => "fingerprint differs: faces differ in kind or rounded centroid".into(),
    }
}

// ---------------------------------------------------------------------------------------------
// (c) The Rust engine reading the Python-written files
// ---------------------------------------------------------------------------------------------

fn rust_reads_python(resumed: &mut BTreeMap<String, usize>) -> Vec<Difference> {
    let mut out = Vec::new();
    for c in cases() {
        if c.part().is_some() {
            continue;
        }
        let (Some(written), Some(Value::Array(back))) = (c.written_step(), c.artifact("readback"))
        else {
            continue;
        };
        let evidence = common::read_json_gz(
            &write_fixtures::case_dir(&c.name).join("python_written_evidence.json.gz"),
        );
        let sha = common::sha256(&std::fs::read(&written).unwrap());
        assert_eq!(
            evidence["written_step_sha256"].as_str(),
            Some(sha.as_str()),
            "{}: the resume evidence was captured from other bytes; recapture it",
            c.name
        );
        let evidence = evidence["resume_evidence"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for (k, python) in back.iter().enumerate() {
            let loaded = load_part(&written, k, back.len());
            let rust = match &loaded {
                Ok(l) => readback_of(l),
                Err(e) => json!({"error": "LoadError", "message": e}),
            };
            let name = if back.len() > 1 {
                format!("{}#{k}", c.name)
            } else {
                c.name.clone()
            };
            let outcome = match (&python["resume"], &rust["resume"]) {
                (_, r) if is_error(r) => "rust failed".to_owned(),
                (Value::Null, Value::Null) => "neither engine resumed".into(),
                (_, Value::Null) => "rust did not resume".into(),
                (p, r) if p == r => "resumed, same answers".into(),
                _ => "resumed, different answers".into(),
            };
            *resumed.entry(outcome).or_default() += 1;
            let mut diffs = compare_readback(&name, python, &rust);
            for d in &mut diffs {
                if let (true, Ok(l)) = (d.path == "resume" && d.rust == Some(Value::Null), &loaded)
                {
                    let why = rust_resume_failure(l, k, evidence.get(k));
                    d.path = format!("resume[rust did not resume: {why}]");
                }
            }
            out.extend(diffs);
        }
    }
    prefixed(explained(&["existing"], &out), "python_written")
}

#[test]
fn the_engines_read_each_others_files() {
    let (mut python_resumed, mut rust_resumed) = (BTreeMap::new(), BTreeMap::new());
    let mut differences = python_reads_rust(&mut python_resumed);
    differences.extend(rust_reads_python(&mut rust_resumed));
    println!("Python resuming Rust-written parts: {python_resumed:?}");
    println!("Rust resuming Python-written parts: {rust_resumed:?}");
    check_known("cross_engine", &differences);
}
