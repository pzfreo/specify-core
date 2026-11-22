//! The in-memory API (`part::load_bytes`, `writer::write_bytes`): a file held as bytes loads,
//! analyses, meshes and writes exactly as the same file on disk does. The parity check is the
//! editor's `crates/intent/examples/verify_pmi.rs` (draftwright-editor, thin-slice), whose
//! byte adapter these replace: a fresh write and a resumed write of the plate, each compared
//! byte for byte and report for report with the file writer.

mod common;

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use specify_core::api;
use specify_core::part::{self, LoadError};
use specify_core::writer::{self, WriteError};

fn fixture(name: &str) -> PathBuf {
    common::fixtures()
        .join("parts")
        .join(format!("{name}.step"))
}

/// A fresh directory for one test.
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-bytes-api-tests")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// verify_pmi.rs's answers: a fit on the plate's hole set, the part's notes and a finish on
/// face 0.
fn answers(fit: &str) -> Map<String, Value> {
    json!({
        "hole.function:holes:9": fit,
        "part.surface_finish": "Ra 1.6",
        "part.coating": "Anodise black",
        "part.edges": "Deburr only",
        "extra.1": {"faces": [0], "kind": "finish", "value": "0.8"},
    })
    .as_object()
    .cloned()
    .unwrap()
}

/// verify_pmi.rs: `input` written with `fit` by `write_bytes` and by `write` gives the same
/// bytes and the same `written`, `not_written` and `warnings`; also what the editor's session
/// takes from the loaded part (its binding, analysis and mesh) is the same from memory.
fn same_from_memory_and_disk(input: &Path, fit: &str, output: &Path) -> Vec<u8> {
    let bytes = std::fs::read(input).unwrap();
    let answers = answers(fit);
    let in_memory = part::load_bytes("part.step", bytes).unwrap();
    let native = part::load(input, None).unwrap();
    assert_eq!(in_memory.binding, native.binding);
    assert_eq!(in_memory.path, PathBuf::from("part.step"));
    let analysis = api::analyse(&in_memory).unwrap();
    assert_eq!(analysis, api::analyse(&native).unwrap());
    assert_eq!(api::mesh(&in_memory), api::mesh(&native));

    let intent = api::preview(&analysis, &answers).unwrap();
    let (memory_bytes, memory_report) =
        writer::write_bytes(&in_memory, &intent, Some(&answers)).unwrap();
    let native_report = writer::write(&native, &intent, output, Some(&answers)).unwrap();
    assert_eq!(
        memory_bytes,
        std::fs::read(output).unwrap(),
        "write_bytes differs from write ({fit})"
    );
    let (memory_report, native_report) = (memory_report.to_dict(), native_report.to_dict());
    for key in ["written", "not_written", "warnings"] {
        assert_eq!(
            memory_report[key], native_report[key],
            "the reports differ on {key} ({fit})"
        );
    }
    assert_eq!(memory_report["output"], json!("part.step"));
    assert!(
        memory_report["written"]
            .as_array()
            .is_some_and(|w| !w.is_empty()),
        "something is written ({fit})"
    );
    memory_bytes
}

/// A fresh write from memory is the file writer's, byte for byte, with the same report.
#[test]
fn bytes_in_bytes_out_match_the_file_writer_fresh() {
    let dir = temp("parity-fresh");
    same_from_memory_and_disk(&fixture("plate"), "fit:H7", &dir.join("native-0.step"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A fit changed on the written file is identical through both edit-list entry points.
#[test]
fn bytes_in_bytes_out_match_the_file_writer_resumed() {
    use specify_core::{characteristics, edits, existing};
    use writer::apply::{apply_bytes, apply_to};

    let dir = temp("parity-resumed");
    let mut input = fixture("plate");
    let mut number = None;
    for (index, class) in ["H7", "H8"].into_iter().enumerate() {
        let output = dir.join(format!("native-{index}.step"));
        let list = if index == 0 {
            json!({"fill": answers("fit:H7")})
        } else {
            json!({"change": {"hole.function:holes:9": {"to": "fit:H8", "expect": "⌀8 H7"}}})
        };
        let bytes = std::fs::read(&input).unwrap();
        let in_memory = part::load_bytes("part.step", bytes.clone()).unwrap();
        let native = part::load(&input, None).unwrap();
        assert_eq!(in_memory.path, PathBuf::from("part.step"));
        assert_eq!(in_memory.binding, native.binding);
        assert_eq!(
            api::analyse(&in_memory).unwrap(),
            api::analyse(&native).unwrap()
        );
        assert_eq!(api::mesh(&in_memory), api::mesh(&native));
        let memory = apply_bytes("part.step", bytes, None, &list.to_string()).unwrap();
        let native = apply_to(&native, &edits::parse_value(list).unwrap(), &output).unwrap();
        assert_eq!(memory.bytes, std::fs::read(&output).unwrap());
        assert_eq!(memory.report.output, PathBuf::from("part.step"));
        assert_eq!(native.report.output, output);
        let (memory_report, native_report) = (memory.to_dict(), native.to_dict());
        for key in ["written", "not_written", "warnings", "retired"] {
            assert_eq!(memory_report[key], native_report[key], "{key} ({class})");
        }
        assert!(!memory.report.written.is_empty(), "{class}");
        let reopened = part::load_bytes("written.step", memory.bytes).unwrap();
        let fit = existing::read_existing(&reopened)
            .unwrap()
            .into_iter()
            .find(|item| item.faces == [9] && item.fields.contains_key("fit"))
            .unwrap();
        assert_eq!(fit.fields["fit"], class);
        let fit_number = fit.number.unwrap();
        assert!(
            characteristics::read(&reopened)
                .unwrap()
                .iter()
                .any(|item| item.number == fit_number)
        );
        assert_eq!(fit_number, *number.get_or_insert(fit_number));
        assert!(
            api::analyse(&reopened).unwrap()["resume"]["answers"]["hole.function:holes:9"]
                .is_null()
        );
        input = output;
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// A file of several parts is refused from memory as `load` refuses it without a part, the
/// name standing for the path.
#[test]
fn load_bytes_refuses_an_assembly_as_load_does() {
    let path = fixture("a_file_with_two_parts_must_say_which");
    let bytes = std::fs::read(&path).unwrap();
    let Err(memory) = part::load_bytes("two.step", bytes) else {
        panic!("an assembly loads from memory");
    };
    let Err(native) = part::load(&path, None) else {
        panic!("an assembly loads");
    };
    let (
        LoadError::Assembly { path: p, names },
        LoadError::Assembly {
            names: expected, ..
        },
    ) = (&memory, &native)
    else {
        panic!("not refused as an assembly: {memory} / {native}");
    };
    assert_eq!(p, &PathBuf::from("two.step"));
    assert_eq!(names, expected);
    assert_eq!(
        memory.to_string(),
        native
            .to_string()
            .replace(&path.display().to_string(), "two.step")
    );
}

/// Bytes that are gzip or not STEP are refused as unreadable, as from a file.
#[test]
fn load_bytes_refuses_unreadable_bytes() {
    let err = part::load_bytes("x.step.gz", vec![0x1f, 0x8b, 8, 0])
        .unwrap_err()
        .to_string();
    assert!(
        err.starts_with("cannot read STEP file x.step.gz") && err.contains("gzip"),
        "{err}"
    );
    let err = part::load_bytes("x.step", b"not a step file".to_vec())
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("cannot read STEP file x.step"), "{err}");
}

/// write_bytes writes a part of an assembly as write_parts_bytes writes it alone (#2: it took a
/// file of one part only), the file's other parts in view, and refuses an intent made for
/// other bytes (the binding check `write` makes).
#[test]
fn write_bytes_writes_a_part_of_an_assembly_and_refuses_a_foreign_intent() {
    let assembly = part::load(&fixture("assembly"), Some(0)).unwrap();
    let analysis = api::analyse(&assembly).unwrap();
    let intent = api::preview(&analysis, &Map::new()).unwrap();
    let answers = Map::new();
    let (bytes, report) = writer::write_bytes(&assembly, &intent, Some(&answers)).unwrap();
    assert_eq!(
        (bytes.clone(), report),
        writer::write_parts_bytes(&[(&assembly, &intent, Some(&answers))]).unwrap()
    );
    let back = part::File::of_bytes(Path::new("written.step"), bytes).unwrap();
    assert_eq!(
        back.parts(),
        part::File::read(&fixture("assembly")).unwrap().parts()
    );
    // A part of an assembly is still written with the answers it came from.
    match writer::write_bytes(&assembly, &intent, None) {
        Err(WriteError::Invalid(why)) => assert_eq!(
            why,
            "a part of an assembly is written with the answers it came from"
        ),
        other => panic!("a part of an assembly written without answers: {other:?}"),
    }

    let plate = part::load_bytes("plate.step", std::fs::read(fixture("plate")).unwrap()).unwrap();
    match writer::write_bytes(&plate, &intent, None) {
        Err(WriteError::Invalid(why)) => assert_eq!(why, "intent was made for a different file"),
        other => panic!("a foreign intent written in memory: {other:?}"),
    }
}
