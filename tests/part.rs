//! The part model (`src/part.rs`, port of `load.py`): parts, bindings, face numbering and the
//! U1 stand-in, checked against specify-core's own outputs.
//!
//! Ports `tests/test_load.py`'s behavioural tests and `tests/test_assembly.py`'s parts listing.
//! Expected values come from Python: written below as literals (with the command that produced
//! them) for the committed synthetic parts, and compared live for corpus and NIST files when
//! specify-core's venv is present (skipped otherwise, as are files that are absent).

use std::path::{Path, PathBuf};
use std::process::Command;

use haecceity::Part;
use haecceity::geom::Surface;
use haecceity::step::Placement;
use serde_json::{Value, json};
use specify_core::part::{self, Binding, LOADER_VERSION, LoadedPart};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/parts")
        .join(name)
}

const SYNTHETIC: [&str; 5] = [
    "assembly.step",
    "pin.step",
    "plate.step",
    "stack.step",
    "wrapped.step",
];

fn corpus_root() -> PathBuf {
    std::env::var_os("QUIDDITY_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users/paul/repos/quiddity/tests/corpus"))
}

/// A corpus file, if the corpus has it.
fn corpus(relative: &str) -> Option<PathBuf> {
    let path = corpus_root().join(relative);
    path.is_file().then_some(path)
}

const NIST_AP242: &str = "/Users/paul/repos/dw-examples/steps/nist";
const PYTHON: &str = "/Users/paul/repos/specify-core/.venv/bin/python";

/// One file and part from each of five corpus subdirectories (and a sixth), and the corpus's
/// NIST geometry files: the files the live face-by-face comparison runs on.
const CORPUS_SAMPLE: [&str; 16] = [
    "cadgenbench/flanged_spool_132.step",
    "gramel/GRM-03_thumbwheel_drive_screw.step",
    "mfcadpp/1000.step",
    "mfcadpp_holdout/118.step",
    "ttt_inputs/sm-hanger.step",
    "cadgenbench_inputs/cgb203.step",
    "nist/nist_ctc_01_asme1_rd.stp",
    "nist/nist_ctc_02_asme1_rc.stp",
    "nist/nist_ctc_03_asme1_rc.stp",
    "nist/nist_ctc_04_asme1_rd.stp",
    "nist/nist_ctc_05_asme1_rd.stp",
    "nist/nist_ftc_06_asme1_rd.stp",
    "nist/nist_ftc_07_asme1_rd.stp",
    "nist/nist_ftc_08_asme1_rc.stp",
    "nist/nist_ftc_09_asme1_rd.stp",
    "nist/nist_ftc_10_asme1_rb.stp",
];

/// Python's `describe_faces` kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    P,
    C,
    K,
    T,
    O,
}
use Kind::{C, K, O, P};

impl Kind {
    fn of(surface: &Surface) -> Kind {
        match surface {
            Surface::Plane { .. } => P,
            Surface::Cylinder { .. } => C,
            Surface::Cone { .. } => K,
            Surface::Torus { .. } => Kind::T,
            _ => O,
        }
    }

    fn python(name: &str) -> Kind {
        match name {
            "plane" => P,
            "cylinder" => C,
            "cone" => K,
            "torus" => Kind::T,
            _ => O,
        }
    }
}

/// A face's kind and area centroid, as Python's `describe_faces` gives them.
fn face(part: &Part, index: usize) -> (Kind, [f64; 3]) {
    let centroid = part
        .face_moments(index)
        .unwrap_or_else(|| panic!("face {index} has no moments"))
        .centroid;
    (Kind::of(&part.faces[index].surface), centroid)
}

/// Python rounds centroids to 6 decimals; within half that unit plus noise is the same point.
fn same_point(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= 1e-6)
}

/// Face-by-face mismatches between a loaded part and Python's faces: `(index, why)`.
fn mismatches(loaded: &LoadedPart, python: &[(Kind, [f64; 3])]) -> Vec<String> {
    let mut out = Vec::new();
    if loaded.part.faces.len() != python.len() {
        out.push(format!(
            "{} faces, Python {}",
            loaded.part.faces.len(),
            python.len()
        ));
    }
    for (i, &(kind, centroid)) in python.iter().enumerate().take(loaded.part.faces.len()) {
        let (k, c) = face(&loaded.part, i);
        if k != kind || !same_point(c, centroid) {
            out.push(format!(
                "face {i}: {k:?} {c:?}, Python {kind:?} {centroid:?}"
            ));
        }
    }
    out
}

/// Whether face `i` is Python's face `i`, where the centroids are not equal to Python's 6
/// decimals: the faces are the same kind at every index, and Python's face nearest to each face's
/// centroid, among faces of its kind, is the face at the same index. Returns the indices where
/// that fails, and the largest centroid distance from Python's face at the same index.
fn numbering_mismatches(loaded: &LoadedPart, python: &[(Kind, [f64; 3])]) -> (Vec<String>, f64) {
    let dist = |a: [f64; 3], b: [f64; 3]| {
        a.iter()
            .zip(&b)
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f64>()
            .sqrt()
    };
    let mut bad = Vec::new();
    if loaded.part.faces.len() != python.len() {
        bad.push(format!(
            "{} faces, Python {}",
            loaded.part.faces.len(),
            python.len()
        ));
        return (bad, f64::NAN);
    }
    let mut worst = 0.0_f64;
    for (i, &(kind, centroid)) in python.iter().enumerate() {
        let (k, c) = face(&loaded.part, i);
        let own = dist(c, centroid);
        worst = worst.max(own);
        if k != kind {
            bad.push(format!("face {i}: {k:?}, Python {kind:?}"));
            continue;
        }
        if let Some((j, d)) = python
            .iter()
            .enumerate()
            .filter(|(j, (pk, _))| *j != i && *pk == k)
            .map(|(j, (_, pc))| (j, dist(c, *pc)))
            .find(|&(_, d)| d < own)
        {
            bad.push(format!(
                "face {i}: nearer Python face {j} ({d:e}) than face {i} ({own:e})"
            ));
        }
    }
    (bad, worst)
}

// --- the synthetic parts, against literals from Python ---------------------------------------

/// A part as `parts` lists it: name, faces, instances, placements.
type Listing<'a> = (&'a str, usize, usize, Vec<Placement>);
/// A part's expected name, face count and placements.
type Expected = (&'static str, usize, Vec<Placement>);
/// Python's faces of one part of a file.
type PythonFaces = (&'static str, usize, &'static [(Kind, [f64; 3])]);

/// `specify-core parts tests/fixtures/parts/<file>` with specify-core's venv, and each part's
/// `load_all(path)[k].binding.to_dict()`.
#[test]
fn parts_and_bindings_are_pythons() {
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let at = |x: f64, z: f64| [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, z]];
    let expected: [(&str, &str, &[Expected]); 5] = [
        (
            "assembly.step",
            "8bcf7a6500f889884b09edecaa73a95953a41fe77d5c85785382eff8589cd716",
            &[
                ("plate", 13, vec![identity]),
                ("pin", 7, vec![at(-30.0, 5.0), at(30.0, 5.0)]),
            ],
        ),
        (
            "pin.step",
            "5315022e025b5167c01b059df0d3a0e4e402bc808ab45b19372e2db40f81868c",
            &[("COMPOUND", 7, vec![identity])],
        ),
        (
            "plate.step",
            "0336b397faae8d94757b1fb0c4a1eca152428559298c6da66c9134851667781e",
            &[("COMPOUND", 14, vec![identity])],
        ),
        (
            "stack.step",
            "1da640ef610f52637a8b898c744b44ac84b6d1051960f81437bc06a3448eaf38",
            &[
                ("base", 11, vec![identity]),
                ("cover", 11, vec![at(0.0, 8.0)]),
                ("pin", 5, vec![at(0.0, -5.0)]),
            ],
        ),
        (
            // Python names a part held alone by an assembly after the assembly.
            "wrapped.step",
            "dd7cc5ee17252e00b875b80b22e15e2c91b5e71e3604aecf140aad72e012f5d5",
            &[("assembly", 7, vec![identity])],
        ),
    ];
    for (file, sha, want) in expected {
        let path = fixture(file);
        let listed = part::parts(&path).unwrap();
        let got: Vec<Listing> = listed
            .iter()
            .map(|p| (p.name.as_str(), p.faces, p.instances, p.placements.clone()))
            .collect();
        let want_listed: Vec<Listing> = want
            .iter()
            .map(|(n, f, p)| (*n, *f, p.len(), p.clone()))
            .collect();
        assert_eq!(got, want_listed, "{file}");
        assert!(listed.iter().enumerate().all(|(i, p)| p.part == i));

        let loaded = part::load_all(&path).unwrap();
        for (k, lp) in loaded.iter().enumerate() {
            assert_eq!(
                lp.binding,
                Binding {
                    source_sha256: sha.into(),
                    face_count: want[k].1,
                    loader_version: 2,
                    part: k,
                },
                "{file} part {k}"
            );
            assert_eq!(lp.part_count, want.len());
            assert_eq!(lp.name, want[k].0);
            assert_eq!(lp.placements, want[k].2, "{file} part {k}");
        }
    }
}

/// The parts listing as JSON, in Python's key order (`api.parts` before it adds `written`), with
/// the product's id after the name (decision D1; not in Python's listing).
#[test]
fn a_parts_listing_serialises_as_pythons() {
    let listed = part::parts(&fixture("assembly.step")).unwrap();
    let json = serde_json::to_string(&listed[1]).unwrap();
    assert_eq!(
        json,
        r#"{"part":1,"name":"pin","product_id":"pin","faces":7,"instances":2,"placements":[[[1.0,0.0,0.0,-30.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,5.0]],[[1.0,0.0,0.0,30.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,5.0]]]}"#
    );
    let binding = part::load(&fixture("plate.step"), None).unwrap().binding;
    assert_eq!(
        serde_json::to_string(&binding).unwrap(),
        r#"{"source_sha256":"0336b397faae8d94757b1fb0c4a1eca152428559298c6da66c9134851667781e","face_count":14,"loader_version":2,"part":0}"#
    );
    assert_eq!(LOADER_VERSION, 2);
}

/// Python's faces, from `[(f.kind, f.centroid) for f in describe_faces(lp)]` for each `lp` of
/// `load_all(path)` with specify-core's venv (centroids rounded to 6 decimals as Python does).
#[rustfmt::skip]
const PYTHON_FACES: &[PythonFaces] = &[
    ("assembly.step", 0, &[
        (P, [-50.0, -0.0, -0.0]), (P, [0.0, -30.0, -0.0]), (P, [-0.107847, -0.0, 5.0]),
        (P, [0.0, 30.0, -0.0]), (P, [-0.107847, -0.0, -5.0]), (P, [50.0, -0.0, -0.0]),
        (C, [-30.0, -15.0, -0.0]), (C, [0.0, -15.0, -0.0]), (C, [30.0, -15.0, -0.0]),
        (C, [-30.0, 15.0, -0.0]), (C, [0.0, 15.0, -0.0]), (C, [45.0, -0.0, -0.0]),
        (C, [30.0, 15.0, -0.0]),
    ]),
    ("assembly.step", 1, &[
        (C, [0.0, 0.0, 22.0]), (P, [0.0, 0.0, 32.0]), (P, [-0.0, -0.0, 12.0]),
        (C, [0.0, -0.0, 28.0]), (C, [0.0, 0.0, 6.0]), (P, [-0.0, 0.0, 24.0]), (P, [0.0, 0.0, 0.0]),
    ]),
    ("pin.step", 0, &[
        (C, [0.0, 0.0, 22.0]), (P, [0.0, 0.0, 32.0]), (P, [-0.0, -0.0, 12.0]),
        (C, [0.0, -0.0, 28.0]), (C, [0.0, 0.0, 6.0]), (P, [-0.0, 0.0, 24.0]), (P, [0.0, 0.0, 0.0]),
    ]),
    ("plate.step", 0, &[
        (P, [-50.0, -0.0, -0.0]), (P, [0.0, -30.0, -0.0]), (P, [-0.108793, -0.0, 5.0]),
        (P, [0.0, 30.0, -0.0]), (P, [-0.108793, -0.0, -5.0]), (P, [50.0, -0.0, -0.0]),
        (C, [-30.0, -15.0, -0.0]), (C, [0.0, -15.0, -0.0]), (C, [30.0, -15.0, -0.0]),
        (C, [0.0, 0.0, -0.0]), (C, [-30.0, 15.0, -0.0]), (C, [0.0, 15.0, -0.0]),
        (C, [45.0, -0.0, -0.0]), (C, [30.0, 15.0, -0.0]),
    ]),
    ("stack.step", 0, &[
        (P, [-30.0, -0.0, -0.0]), (P, [-0.0, -20.0, -0.0]), (P, [-0.0, -0.0, 5.0]),
        (P, [-0.0, 20.0, -0.0]), (P, [-0.0, -0.0, -5.0]), (P, [30.0, -0.0, -0.0]),
        (C, [-20.0, -10.0, -0.0]), (C, [20.0, -10.0, -0.0]), (C, [-20.0, 10.0, -0.0]),
        (C, [0.0, 0.0, -0.0]), (C, [20.0, 10.0, -0.0]),
    ]),
    ("stack.step", 1, &[
        (P, [-30.0, 0.0, 0.0]), (P, [0.0, -20.0, 0.0]), (P, [-0.0, 0.0, 3.0]),
        (P, [0.0, 20.0, 0.0]), (P, [-0.0, 0.0, -3.0]), (P, [30.0, 0.0, 0.0]),
        (C, [-20.0, -10.0, -0.0]), (C, [20.0, -10.0, -0.0]), (C, [-20.0, 10.0, -0.0]),
        (C, [0.0, 0.0, 0.0]), (C, [20.0, 10.0, -0.0]),
    ]),
    ("stack.step", 2, &[
        (C, [0.0, 0.0, 17.5]), (P, [0.0, 0.0, 19.0]), (P, [0.0, 0.0, 16.0]), (C, [0.0, 0.0, 8.0]),
        (P, [0.0, 0.0, 0.0]),
    ]),
    ("wrapped.step", 0, &[
        (P, [-50.0, -0.0, -0.0]), (P, [0.0, -30.0, -0.0]), (P, [0.0, -0.0, 5.0]),
        (P, [0.0, 30.0, -0.0]), (P, [0.0, -0.0, -5.0]), (P, [50.0, -0.0, -0.0]),
        (C, [0.0, 0.0, -0.0]),
    ]),
];

#[test]
fn every_synthetic_face_is_pythons_face_at_the_same_index() {
    let mut checked = 0;
    for file in SYNTHETIC {
        let loaded = part::load_all(&fixture(file)).unwrap();
        let want: Vec<_> = PYTHON_FACES.iter().filter(|(f, ..)| *f == file).collect();
        assert_eq!(loaded.len(), want.len(), "{file}");
        for (lp, (_, k, faces)) in loaded.iter().zip(want) {
            assert_eq!(lp.binding.part, *k);
            let bad = mismatches(lp, faces);
            assert!(bad.is_empty(), "{file} part {k}: {bad:#?}");
            checked += faces.len();
        }
    }
    assert_eq!(checked, 75);
}

// --- test_load.py ---------------------------------------------------------------------------

/// test_face_indices_are_stable_across_processes: the same bytes number faces the same way
/// every time, here two loads in one process (nothing in the load depends on the process).
#[test]
fn loading_twice_gives_the_same_faces_bindings_and_unresolved_lists() {
    let mut files: Vec<PathBuf> = SYNTHETIC.iter().map(|f| fixture(f)).collect();
    files.extend(corpus("cadgenbench/flanged_spool_132.step"));
    files.extend(corpus("nist/nist_ctc_02_asme1_rc.stp"));
    for path in files {
        let signature = |parts: &[LoadedPart]| -> Vec<String> {
            parts
                .iter()
                .map(|lp| {
                    let faces: Vec<String> = (0..lp.part.faces.len())
                        .map(|i| {
                            let (k, c) = face(&lp.part, i);
                            let bits = c.map(f64::to_bits);
                            format!("{i}:{k:?}:{bits:?}:{}", lp.face_entity(i))
                        })
                        .collect();
                    format!(
                        "{:?}|{}|{:?}|{:?}|{}",
                        lp.binding,
                        faces.join(","),
                        lp.unresolved_faces,
                        lp.unresolved_edges,
                        lp.rebuilt
                    )
                })
                .collect()
        };
        let a = signature(&part::load_all(&path).unwrap());
        let b = signature(&part::load_all(&path).unwrap());
        assert_eq!(a, b, "{}", path.display());
    }
}

/// test_binding_identifies_the_file.
#[test]
fn the_binding_identifies_the_file() {
    let a = part::load(&fixture("plate.step"), None).unwrap();
    let b = part::load(&fixture("pin.step"), None).unwrap();
    assert_ne!(a.binding.source_sha256, b.binding.source_sha256);
    assert_eq!(a.binding.face_count, a.part.faces.len());
    assert_eq!(a.binding.face_count, a.definition.faces.len());
}

/// test_index_of_round_trips, in haecceity's terms: face `i` is the face read from
/// `face_entity(i)`, and the definition finds `i` again from it.
#[test]
fn each_face_is_read_from_its_entity() {
    for file in SYNTHETIC {
        for lp in part::load_all(&fixture(file)).unwrap() {
            for i in 0..lp.binding.face_count {
                assert_eq!(lp.definition.face_index(lp.face_entity(i)), Some(i));
                assert_eq!(lp.part.face_source(i).unwrap().entity, lp.face_entity(i));
            }
        }
    }
}

/// test_an_assembly_holding_one_solid_is_numbered_on_the_solid.
#[test]
fn an_assembly_holding_one_solid_is_numbered_on_the_solid() {
    let lp = part::load(&fixture("wrapped.step"), None).unwrap();
    assert_eq!(lp.binding.face_count, 7);
    assert_eq!(lp.part.solids.len(), 1);
    assert_eq!(lp.part.solids[0].faces, (0..7).collect::<Vec<_>>());
    assert!(
        !lp.rebuilt,
        "a part placed once at the origin is read_step's own Part"
    );
}

/// test_nist_ctc_03_loads_through_its_assembly.
#[test]
fn nist_ctc_03_loads_through_its_assembly() {
    let Some(path) = corpus("nist/nist_ctc_03_asme1_rc.stp") else {
        eprintln!("skipped: corpus not present");
        return;
    };
    let lp = part::load(&path, None).unwrap();
    assert_eq!(lp.binding.face_count, 139);
    assert!(!lp.rebuilt);
}

/// test_a_file_with_two_parts_must_say_which, on the committed two-part assembly.
#[test]
fn a_file_with_two_parts_must_say_which() {
    let path = fixture("assembly.step");
    let err = part::load(&path, None).unwrap_err().to_string();
    assert_eq!(
        err,
        format!(
            "{} is an assembly of 2 parts; choose one (0: plate, 1: pin)",
            path.display()
        )
    );
    assert_eq!(part::load(&path, Some(1)).unwrap().binding.part, 1);
    let err = part::load(&path, Some(2)).unwrap_err().to_string();
    assert_eq!(
        err,
        format!("{} has 2 part(s); there is no part 2", path.display())
    );
}

#[test]
fn unreadable_files_are_refused() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let missing = dir.join("no-such-file.step");
    let err = part::load(&missing, None).unwrap_err().to_string();
    assert!(
        err.starts_with(&format!("cannot read STEP file {}", missing.display())),
        "{err}"
    );

    // A gzipped file is read decompressed (tests/gzip.rs), no longer refused.
    let junk = dir.join("junk.step");
    std::fs::write(&junk, "not a STEP file").unwrap();
    let err = part::load(&junk, None).unwrap_err().to_string();
    assert!(err.starts_with("cannot read STEP file"), "{err}");
}

// --- test_assembly.py ------------------------------------------------------------------------

/// test_each_part_is_listed_once_with_where_its_instances_are (the `written` flag is the API's).
#[test]
fn each_part_is_listed_once_with_where_its_instances_are() {
    let listed = part::parts(&fixture("assembly.step")).unwrap();
    let [plate, pin] = listed.as_slice() else {
        panic!("{listed:?}")
    };
    assert_eq!(
        (plate.name.as_str(), plate.faces, plate.instances),
        ("plate", 13, 1)
    );
    assert_eq!((pin.name.as_str(), pin.faces, pin.instances), ("pin", 7, 2));
    let translations: Vec<Vec<f64>> = pin
        .placements
        .iter()
        .map(|p| p.iter().map(|row| row[3]).collect())
        .collect();
    assert_eq!(translations, [[-30.0, 0.0, 5.0], [30.0, 0.0, 5.0]]);
}

// --- haecceity's own Part of every part, and its unresolved lists ----------------------------

/// A file of one part placed once at the origin is haecceity's own `Part`, not rebuilt.
#[test]
fn a_single_part_file_uses_read_steps_part() {
    for file in ["plate.step", "pin.step", "wrapped.step"] {
        let path = fixture(file);
        let lp = part::load(&path, None).unwrap();
        let whole = haecceity::read_step(&std::fs::read(&path).unwrap()).unwrap();
        assert!(!lp.rebuilt, "{file}");
        assert_eq!(lp.part.faces.len(), whole.faces.len());
        assert_eq!(lp.part.edges.len(), whole.edges.len());
        assert_eq!(lp.unresolved_faces, whole.unresolved_faces());
        assert_eq!(lp.unresolved_edges, whole.unresolved_edges());
        assert_eq!(lp.degraded(), None);
    }
}

/// A part of an assembly, or a part placed more than once, is haecceity's own `Part`
/// (`StepFile::part`, upstream need U1, met): its unresolved lists are its own `Part`'s and are
/// read_step's restricted to the part's first instance and re-indexed (faces by the `#N` they
/// were read from, edges likewise), and its topology is the instance's: the same faces, edges and
/// solids, by source. (Before U1 was met these parts were rebuilt by a stand-in; the same checks
/// held of it.)
fn check_own(path: &Path, lp: &LoadedPart, whole: &Part) {
    assert!(!lp.rebuilt, "{}", path.display());
    assert_eq!(lp.unresolved_faces, lp.part.unresolved_faces());
    assert_eq!(lp.unresolved_edges, lp.part.unresolved_edges());
    for i in 0..lp.binding.face_count {
        assert_eq!(lp.part.face_source(i).unwrap().entity, lp.face_entity(i));
    }
    let instances = &lp.definition.placements[0].instances;
    let ours =
        |s: Option<haecceity::brep::Source>| s.is_some_and(|s| instances.contains(&s.instance));
    // Faces: the instance's, in the definition's order.
    let face_of: std::collections::HashMap<u64, usize> = (0..whole.faces.len())
        .filter(|&f| ours(whole.face_source(f)))
        .map(|f| (whole.face_source(f).unwrap().entity, f))
        .collect();
    assert_eq!(face_of.len(), lp.part.faces.len(), "{}", path.display());
    let mut want_faces: Vec<usize> = whole
        .unresolved_faces()
        .iter()
        .filter(|&&f| ours(whole.face_source(f)))
        .map(|&f| {
            let entity = whole.face_source(f).unwrap().entity;
            lp.definition.face_index(entity).unwrap()
        })
        .collect();
    want_faces.sort_unstable();
    assert_eq!(lp.unresolved_faces, want_faces, "{}", path.display());
    // Edges: map the part's edges to the whole read's through the faces' loops.
    let mut edge_map = std::collections::HashMap::new();
    for (i, &entity) in lp.definition.faces.iter().enumerate() {
        let old = &whole.faces[face_of[&entity]];
        let new = &lp.part.faces[i];
        assert_eq!(old.loops.len(), new.loops.len());
        for (lo, ln) in old.loops.iter().zip(&new.loops) {
            assert_eq!(lo.edges.len(), ln.edges.len());
            for (&(eo, d0), &(en, d1)) in lo.edges.iter().zip(&ln.edges) {
                assert_eq!(d0, d1);
                assert_eq!(*edge_map.entry(eo).or_insert(en), en);
            }
        }
    }
    assert_eq!(edge_map.len(), lp.part.edges.len());
    let mut want_edges: Vec<usize> = whole
        .unresolved_edges()
        .iter()
        .filter(|&&e| ours(whole.edge_source(e)))
        .map(|e| edge_map[e])
        .collect();
    want_edges.sort_unstable();
    assert_eq!(lp.unresolved_edges, want_edges, "{}", path.display());
    let solids = whole
        .solids
        .iter()
        .enumerate()
        .filter(|(s, _)| instances.contains(s))
        .count();
    assert_eq!(lp.part.solids.len(), solids);
    assert_eq!(lp.degraded(), None);
}

#[test]
fn parts_of_assemblies_carry_their_instances_unresolved_lists() {
    let mut checked = 0;
    for file in SYNTHETIC {
        let path = fixture(file);
        let whole = haecceity::read_step(&std::fs::read(&path).unwrap()).unwrap();
        let parts = part::load_all(&path).unwrap();
        for lp in &parts {
            if parts.len() > 1 || lp.placements.len() > 1 {
                check_own(&path, lp, &whole);
                checked += 1;
            }
        }
    }
    // assembly.step: plate and pin; stack.step: base, cover, pin.
    assert_eq!(checked, 5);
}

/// Every corpus file (a gzipped one decompressed first, as the oracle harness does): loads or
/// is refused with a reason; no part is rebuilt or degraded; a part whose file holds more than
/// one instance of it or other parts beside it carries its first instance's unresolved lists
/// (`check_own`). Which parts have unresolved faces or edges is printed.
#[test]
fn corpus_parts_and_their_unresolved_faces() {
    let root = corpus_root();
    if !root.is_dir() {
        eprintln!("skipped: corpus not present");
        return;
    }
    let mut files = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.to_string_lossy();
            if path.is_dir() {
                stack.push(path);
            } else if [".step", ".stp", ".step.gz", ".stp.gz"]
                .iter()
                .any(|e| name.ends_with(e))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("part-corpus");
    std::fs::create_dir_all(&scratch).unwrap();
    let (mut loaded, mut refused) = (0, 0);
    for file in &files {
        let name = file.strip_prefix(&root).unwrap().display().to_string();
        let path = if name.ends_with(".gz") {
            let mut text = Vec::new();
            std::io::Read::read_to_end(
                &mut flate2::read::GzDecoder::new(std::fs::File::open(file).unwrap()),
                &mut text,
            )
            .unwrap();
            let out = scratch.join(name.replace('/', "_").trim_end_matches(".gz"));
            std::fs::write(&out, text).unwrap();
            out
        } else {
            file.clone()
        };
        let parts = match part::load_all(&path) {
            Ok(parts) => parts,
            Err(e) => {
                eprintln!("refused  {name}: {e}");
                refused += 1;
                continue;
            }
        };
        loaded += 1;
        let assembled = |lp: &LoadedPart| parts.len() > 1 || lp.placements.len() > 1;
        let whole = parts
            .iter()
            .any(assembled)
            .then(|| haecceity::read_step(&std::fs::read(&path).unwrap()).unwrap());
        for lp in &parts {
            assert!(!lp.rebuilt, "{name} part {}", lp.binding.part);
            match &whole {
                Some(whole) if assembled(lp) => check_own(&path, lp, whole),
                _ => assert!(lp.part.face_source(0).is_some() && lp.degraded().is_none()),
            }
            if !lp.unresolved_faces.is_empty() || !lp.unresolved_edges.is_empty() {
                eprintln!(
                    "unresolved {name} part {}: faces {:?} edges {:?} rebuilt {} degraded {}",
                    lp.binding.part,
                    lp.unresolved_faces,
                    lp.unresolved_edges,
                    lp.rebuilt,
                    lp.degraded().is_some()
                );
            }
        }
    }
    eprintln!("corpus: {loaded} loaded, {refused} refused");
    assert!(loaded > 0);
}

/// A part of an assembly with an unresolved face keeps haecceity's record of it in its own
/// `Part`, so it is not degraded (before U1 was met, the stand-in rebuilt it without the record
/// and reported it degraded); the other part of the same file has none. assembly.step with the
/// pin's drill floor (#1248, the pin's face 5) written on an `OFFSET_SURFACE`, which haecceity
/// does not model (as its own `unresolved_surface.step`).
#[test]
fn a_part_with_an_unresolved_face_keeps_haecceitys_record() {
    let text = std::fs::read_to_string(fixture("assembly.step")).unwrap();
    let plane = "#1181 = PLANE('',#1182);";
    assert!(text.contains(plane));
    let text = text.replace(
        plane,
        "#1181 = OFFSET_SURFACE('',#99999,0.,.F.);\n#99999 = PLANE('',#1182);",
    );
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("assembly-unresolved.step");
    std::fs::write(&path, text).unwrap();
    let whole = haecceity::read_step(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        whole.unresolved_faces().len(),
        2,
        "both instances of the pin"
    );

    let pin = part::load(&path, Some(1)).unwrap();
    assert_eq!(pin.face_entity(5), 1248);
    assert_eq!(pin.unresolved_faces, [5]);
    // The Part itself has the record, which recognition and solid validity read.
    assert_eq!(pin.part.unresolved_faces(), [5]);
    check_own(&path, &pin, &whole);

    let plate = part::load(&path, Some(0)).unwrap();
    assert!(plate.unresolved_faces.is_empty());
    check_own(&path, &plate, &whole);
}

// --- live comparison with Python on corpus and NIST files -----------------------------------

/// Python's `parts`, bindings and faces for `paths`, from specify-core's venv; `None` when the
/// venv is absent.
fn python_parts(paths: &[PathBuf]) -> Option<serde_json::Map<String, Value>> {
    if !Path::new(PYTHON).is_file() || paths.is_empty() {
        return None;
    }
    const SCRIPT: &str = r#"
import json, sys
from specify_core.load import load_all, parts
from specify_core.features import describe_faces
out = {}
for path in sys.argv[1:]:
    try:
        loaded = load_all(path)
        out[path] = {"parts": parts(path), "bindings": [l.binding.to_dict() for l in loaded],
                     "faces": [[[f.kind, list(f.centroid)] for f in describe_faces(l)] for l in loaded]}
    except Exception as e:
        out[path] = {"error": f"{type(e).__name__}: {e}"}
json.dump(out, sys.stdout)
"#;
    let output = Command::new(PYTHON)
        .arg("-c")
        .arg(SCRIPT)
        .args(paths)
        .output()
        .expect("run specify-core's venv");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    match serde_json::from_slice(&output.stdout).unwrap() {
        Value::Object(map) => Some(map),
        other => panic!("{other}"),
    }
}

/// The files where OpenCascade's XCAF makes a part of each shape representation of one product
/// (the solid, and each surface model beside it): NIST CTC-02, 04 and 05 (but not CTC-05's AP242
/// edition, which has no surface model). haecceity has one part per product definition, holding
/// the solid's faces and then the surface models'. On these files the one Rust part's faces are
/// Python's parts' faces in turn.
fn xcaf_splits(file: &str) -> bool {
    ["nist_ctc_02", "nist_ctc_04", "nist_ctc_05"]
        .iter()
        .any(|n| file.contains(n) && !file.contains("ctc_05_asme1_ap242"))
}

/// Where XCAF makes those components, Python names a part by OpenCascade's label for its shape,
/// `SOLID`; this port names it by its product. Any other name must be Python's.
fn same_name(rust: &Value, python: &Value) -> bool {
    rust == python || python == "SOLID"
}

/// Face by face against Python on five corpus subdirectories' files, the corpus's NIST geometry
/// files and the NIST AP242 editions: the same kind at every index and the same face by centroid
/// (see `numbering_mismatches`); the parts listing and bindings Python's, but for the differences
/// `xcaf_splits` and `same_name` describe. Centroids that differ from Python's beyond its 6
/// decimals are printed (their verdicts belong to the features stream, which owns faces).
#[test]
fn corpus_and_nist_faces_are_pythons_at_every_index() {
    let mut paths: Vec<PathBuf> = CORPUS_SAMPLE.iter().filter_map(|f| corpus(f)).collect();
    for k in 1..=5 {
        let path = PathBuf::from(format!("{NIST_AP242}/nist_ctc_0{k}_asme1_ap242.stp"));
        if path.is_file() {
            paths.push(path);
        }
    }
    let Some(python) = python_parts(&paths) else {
        eprintln!("skipped: specify-core's venv or the files are not present");
        return;
    };
    let mut report = Vec::new();
    for path in &paths {
        let key = path.to_str().unwrap();
        let py = &python[key];
        assert!(py.get("error").is_none(), "{key}: {py}");
        let rust = part::load_all(path).unwrap();
        let listed = part::parts(path).unwrap();
        let py_parts = py["parts"].as_array().unwrap();
        let py_faces: Vec<Vec<(Kind, [f64; 3])>> = py["faces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|faces| {
                faces
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| {
                        let c = f[1].as_array().unwrap();
                        (
                            Kind::python(f[0].as_str().unwrap()),
                            [0, 1, 2].map(|i| c[i].as_f64().unwrap()),
                        )
                    })
                    .collect()
            })
            .collect();
        let sha = py["bindings"][0]["source_sha256"].as_str().unwrap();
        assert!(rust.iter().all(|lp| lp.binding.source_sha256 == sha));
        let compare = |lp: &LoadedPart, faces: &[(Kind, [f64; 3])], report: &mut Vec<String>| {
            let (bad, worst) = numbering_mismatches(lp, faces);
            assert!(bad.is_empty(), "{key} part {}: {bad:#?}", lp.binding.part);
            let off = mismatches(lp, faces).len();
            if off > 0 {
                report.push(format!(
                    "{key} part {}: {off} of {} centroids beyond 1e-6 of Python's, at most {worst:.3e}",
                    lp.binding.part,
                    faces.len()
                ));
            }
        };
        if xcaf_splits(key) {
            assert_eq!(rust.len(), 1, "{key}");
            compare(&rust[0], &py_faces.concat(), &mut report);
            report.push(format!(
                "{key}: one part of {} faces; Python {} parts {:?}",
                rust[0].binding.face_count,
                py_parts.len(),
                py_parts
                    .iter()
                    .map(|p| (p["name"].as_str().unwrap(), p["faces"].as_u64().unwrap()))
                    .collect::<Vec<_>>()
            ));
            continue;
        }
        assert_eq!(rust.len(), py_parts.len(), "{key}");
        for (k, lp) in rust.iter().enumerate() {
            compare(lp, &py_faces[k], &mut report);
            assert_eq!(
                serde_json::to_value(&lp.binding).unwrap(),
                py["bindings"][k],
                "{key} part {k}"
            );
            let mut want = py_parts[k].clone();
            let mut got = serde_json::to_value(&listed[k]).unwrap();
            // The product's id is reported beside the name (decision D1), not by Python.
            let product_id = got.as_object_mut().unwrap().remove("product_id");
            assert_eq!(
                product_id,
                Some(json!(lp.definition.product_id)),
                "{key} part {k}"
            );
            assert!(same_name(&got["name"], &want["name"]), "{key} part {k}");
            if got["name"] != want["name"] {
                report.push(format!(
                    "{key} part {k}: name {}, Python {}",
                    got["name"], want["name"]
                ));
                want["name"] = got["name"].clone();
            }
            assert_eq!(got, want, "{key} part {k}");
        }
    }
    for line in &report {
        eprintln!("differs: {line}");
    }
}
