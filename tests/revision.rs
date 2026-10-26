//! Revisions: answers carried from one revision of a part to the next through quiddity's
//! correspondence, and what changed.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value, json};
use specify_core::part::{LoadedPart, load};
use specify_core::revision::{Revised, diff, picture};

fn pair(case: &str) -> (LoadedPart, LoadedPart) {
    let path = |which: &str| -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/revisions/{case}.{which}.step.gz"))
    };
    (
        load(&path("old"), None).unwrap(),
        load(&path("new"), None).unwrap(),
    )
}

fn answers(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn revised(case: &str, given: Value) -> Revised {
    let (old, new) = pair(case);
    diff(&old, &new, &answers(given)).unwrap()
}

#[test]
fn removing_a_hole_renames_the_answers_of_the_holes_after_it() {
    // Old holes on faces 6 (Ø5), 7 (Ø7, removed), 8 (Ø8) and 9 (Ø6); the new part's faces
    // after 7 renumber.
    let r = revised(
        "hole_removed",
        json!({
            "part.material": "6061-T6",
            "hole.function:holes:6": "tapped:M6",
            "hole.function:holes:7": "fit:H7",
            "hole.function:holes:8": "clearance",
            "hole.function:holes:9": "general",
            "datum.A": [4],
        }),
    );
    assert_eq!(r.verdict, "decide");
    let unresolved: Vec<_> = r
        .unresolved
        .iter()
        .map(|u| (u.id.as_str(), u.class.as_str()))
        .collect();
    assert_eq!(unresolved, [("hole.function:holes:7", "orphaned")]);
    assert_eq!(r.answers["part.material"], "6061-T6");
    assert_eq!(r.answers["hole.function:holes:6"], "tapped:M6");
    assert_eq!(r.answers["datum.A"], json!([4]));
    let (old, new) = pair("hole_removed");
    let diameter = |p: &LoadedPart, id: &str| -> f64 {
        let a = specify_core::api::analyse(p).unwrap();
        a["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == id)
            .unwrap()["record"]["diameter"]
            .as_f64()
            .unwrap()
    };
    // Each carried answer is on the hole of the same size as before.
    for (before, after) in &r.migrated {
        let (b, a) = (
            before.split_once(':').unwrap().1,
            after.split_once(':').unwrap().1,
        );
        assert_eq!(diameter(&old, b), diameter(&new, a), "{before} → {after}");
    }
    assert_eq!(r.migrated.len(), 2, "the Ø8 and Ø6 holes renumber");
    assert!(
        r.features
            .iter()
            .any(|f| f.class == "orphaned" && f.old.as_deref() == Some("holes:7"))
    );
}

#[test]
fn a_moved_hole_keeps_its_answer_for_checking() {
    let r = revised(
        "hole_moved",
        json!({ "hole.function:holes:8": "clearance", "hole.function:holes:6": "general" }),
    );
    assert_eq!(r.verdict, "ok");
    assert!(r.unresolved.is_empty() && r.migrated.is_empty());
    let checked: Vec<_> = r.check.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(checked, ["hole.function:holes:8"]);
    assert!(r.check[0].changes.contains_key("position"));
    assert!(
        !r.gone.is_empty(),
        "the moved hole's old faces are drawn as a ghost"
    );
}

#[test]
fn an_added_hole_asks_its_questions() {
    let r = revised("hole_added", json!({ "part.material": "6061-T6" }));
    assert_eq!(r.verdict, "decide");
    assert!(
        r.features
            .iter()
            .any(|f| f.class == "new" && f.family == "holes")
    );
    assert!(
        r.questions
            .iter()
            .any(|q| q.id.starts_with("hole.function:")),
        "{:?}",
        r.questions
    );
}

#[test]
fn symmetric_holes_are_asked_not_guessed() {
    let (old, _) = pair("plate_thickened");
    let analysis = specify_core::api::analyse(&old).unwrap();
    let holes: Vec<String> = analysis["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["family"] == "holes")
        .map(|f| f["id"].as_str().unwrap().to_string())
        .collect();
    let given: Map<String, Value> = holes
        .iter()
        .map(|h| (format!("hole.function:{h}"), json!("clearance")))
        .collect();
    let r = revised("plate_thickened", Value::Object(given));
    assert!(
        r.unresolved
            .iter()
            .any(|u| u.class == "ambiguous" && u.candidates.len() > 1)
    );
    assert!(r.unresolved.iter().all(|u| u.class == "ambiguous"));
    assert_eq!(r.verdict, "decide");
}

#[test]
fn a_turned_part_carries_every_answer() {
    let r = revised(
        "whole_part_moved",
        json!({ "hole.function:holes:8": "clearance", "datum.A": [4] }),
    );
    assert_eq!(r.verdict, "ok", "{}", r.summary);
    assert!(r.features.is_empty());
    assert_eq!(r.answers.len(), 2);
}

#[test]
fn an_answer_the_old_part_never_had_is_stale() {
    let r = revised(
        "hole_moved",
        json!({ "hole.function:holes:42": "clearance" }),
    );
    assert_eq!(r.unresolved[0].class, "stale");
}

#[test]
fn the_picture_is_a_png() {
    let (old, new) = pair("boss_resized");
    let r = diff(&old, &new, &Map::new()).unwrap();
    let png = picture(&old, &new, &r, 300);
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    assert!(
        height <= 2 * 300 + 2,
        "no taller than twice the width: {height}"
    );
    assert!(
        r.faces.values().any(|c| *c == "changed"),
        "the boss is coloured"
    );
}

#[test]
fn the_command_line_writes_the_answers_report_and_picture() {
    let dir = std::env::temp_dir().join(format!("specify-core-revision-{}", std::process::id()));
    let fixture = |which: &str| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/revisions/hole_removed.{which}.step.gz"
        ))
    };
    std::fs::create_dir_all(&dir).unwrap();
    let answers = dir.join("answers.in.json");
    std::fs::write(
        &answers,
        json!({ "hole.function:holes:7": "fit:H7", "hole.function:holes:9": "general" })
            .to_string(),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("diff")
        .args([fixture("old"), fixture("new")])
        .arg("--answers")
        .arg(&answers)
        .arg("-o")
        .arg(&dir)
        .args(["--title", "Plate"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(printed["verdict"], "decide");
    let carried: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("answers.json")).unwrap()).unwrap();
    assert_eq!(carried, json!({ "hole.function:holes:8": "general" }));
    let md = std::fs::read_to_string(dir.join("report.md")).unwrap();
    assert!(md.starts_with("### Plate: Specify\n\n⚠️"), "{md}");
    assert!(
        md.contains(
            "- What is the Ø7 hole for? **fit:H7** (`hole.function:holes:7`): the Ø7 × 10 \
             through hole was removed"
        ),
        "{md}"
    );
    assert!(
        md.contains(
            "- Ø7 × 10 through hole removed: on the −Z side, at X 30, Y 30 from the −X, −Y corner"
        ),
        "{md}"
    );
    assert!(dir.join("model.png").is_file() && dir.join("report.json").is_file());
}

#[test]
fn the_report_says_what_changed_in_words() {
    let md = |case: &str| revised(case, json!({})).markdown("Plate", None);
    let thickened = md("plate_thickened");
    // Alike holes changed alike read as one line, with where they are.
    assert!(
        thickened.contains(
            "- 3× Ø4 × 10 through holes, through the part along Z, at X 30, Y 30; X 50, Y 30; \
             X 70, Y 30 from the −X, −Y corner: depth 10 → 12"
        ),
        "{thickened}"
    );
    assert!(
        thickened.contains("still there, but alike, so the old and new can't be paired one to one"),
        "{thickened}"
    );
    // The whole part, before and after.
    for row in [
        "| Volume | 58,494 mm³ | 70,193 mm³ | +11,699 mm³ (+20%) |",
        "| Bounding box | 100 × 60 × 10 mm | 100 × 60 × 12 mm | Z +2 |",
        "| Faces | 15 | 15 | unchanged |",
    ] {
        assert!(thickened.contains(row), "{thickened}");
    }
    let moved = md("hole_moved");
    assert!(
        moved.contains("✅ 1 hole moved. There are no answers to carry"),
        "{moved}"
    );
    assert!(
        moved.contains(
            "- Ø6 × 10 through hole, on the −Z side, at X 40, Y 45 from the −X, −Y corner: \
             moved X −15"
        ),
        "{moved}"
    );
    let resized = md("boss_resized");
    assert!(
        resized.contains(
            "- Ø20 × 8 boss, on the +Z side, at X 70, Y 40 from the −X, −Y corner: \
             diameter 20 → 26"
        ),
        "{resized}"
    );
}

#[test]
fn a_datum_s_settings_stay_open_with_their_datum() {
    // Face 7 is the removed hole's bore.
    let r = revised(
        "hole_removed",
        json!({ "datum.A": [7], "datum.A.flatness": "0.01", "datum.B": [4] }),
    );
    let unresolved: Vec<_> = r
        .unresolved
        .iter()
        .map(|u| (u.id.as_str(), u.class.as_str()))
        .collect();
    assert_eq!(
        unresolved,
        [("datum.A", "orphaned"), ("datum.A.flatness", "orphaned")]
    );
    assert_eq!(r.unresolved[1].reason, "its datum datum.A is unresolved");
    assert_eq!(r.answers.keys().collect::<Vec<_>>(), ["datum.B"]);
}

#[test]
fn a_part_the_old_file_lacks_is_named() {
    let fixture = |which: &str| {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/revisions/hole_moved.{which}.step.gz"
        ))
    };
    let out = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("diff")
        .args([fixture("old"), fixture("new")])
        .args(["--part", "1", "-o"])
        .arg(std::env::temp_dir().join("specify-core-revision-part"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("hole_moved.old.step.gz has 1 part(s); there is no part 1"),
        "{err}"
    );
}

/// *fixture* (a gzipped STEP) unpacked to *to*, with *edit* applied to its text.
fn unpacked(fixture: &str, to: &Path, edit: impl Fn(String) -> String) -> PathBuf {
    use std::io::Read;
    let mut text = String::new();
    flate2::read::GzDecoder::new(
        std::fs::File::open(Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture)).unwrap(),
    )
    .read_to_string(&mut text)
    .unwrap();
    std::fs::write(to, edit(text)).unwrap();
    to.to_path_buf()
}

#[test]
fn a_changed_cad_tolerance_is_reported() {
    let dir = std::env::temp_dir().join(format!("specify-core-diff-cad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let nist = "tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz";
    let old = unpacked(nist, &dir.join("old.stp"), |t| t);
    // Flatness.1 (#57) measures #82: 0.2 tightened to 0.1.
    let new = unpacked(nist, &dir.join("new.stp"), |t| {
        let at = t.find("#82=(").unwrap();
        let (head, tail) = t.split_at(at);
        format!(
            "{head}{}",
            tail.replacen("LENGTH_MEASURE(0.2)", "LENGTH_MEASURE(0.1)", 1)
        )
    });
    let (old, new) = (load(&old, None).unwrap(), load(&new, None).unwrap());
    let same = diff(&old, &old, &Map::new()).unwrap();
    assert!(same.pmi.existing.is_empty());
    assert_eq!(same.pmi.existing_count, [21, 21]);
    let r = diff(&old, &new, &Map::new()).unwrap();
    assert_eq!(r.pmi.existing.len(), 1, "{:?}", r.pmi.existing);
    let c = &r.pmi.existing[0];
    assert_eq!(c.class, "changed");
    assert_eq!(
        (c.old.as_deref(), c.new.as_deref()),
        (Some("flatness 0.2"), Some("flatness 0.1"))
    );
    let md = r.markdown("CTC 01", None);
    assert!(md.contains(": flatness 0.2 → 0.1"), "{md}");
    assert!(
        md.starts_with("### CTC 01: Specify\n\n✅ 1 change to the PMI."),
        "{md}"
    );
}

#[test]
fn specify_pmi_in_the_files_is_read_and_compared() {
    let dir = std::env::temp_dir().join(format!("specify-core-diff-pmi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cli = || Command::new(env!("CARGO_BIN_EXE_specify-core"));
    let fixture = "tests/fixtures/revisions/hole_removed";
    // Each revision written with Specify's PMI: the Ø8 hole (face 8, face 7 on the new part)
    // general before, fit:H8 after.
    let written = |which: &str, answers: Value| {
        let step = unpacked(
            &format!("{fixture}.{which}.step.gz"),
            &dir.join(format!("{which}.step")),
            |t| t,
        );
        let (analysis, given) = (
            dir.join(format!("{which}.json")),
            dir.join(format!("{which}.answers.json")),
        );
        std::fs::write(&given, answers.to_string()).unwrap();
        assert!(
            cli()
                .arg("analyse")
                .arg(&step)
                .arg("-o")
                .arg(&analysis)
                .status()
                .unwrap()
                .success()
        );
        let out = dir.join(format!("{which}.pmi.step"));
        assert!(
            cli()
                .arg("write")
                .args([&step, &analysis, &given])
                .arg("-o")
                .arg(&out)
                .arg("--accept-defaults")
                .status()
                .unwrap()
                .success()
        );
        load(&out, None).unwrap()
    };
    let old = written(
        "old",
        json!({ "part.material": "6061-T6", "hole.function:holes:8": "general" }),
    );
    let bare_path = dir.join("bare.step");
    let bare = load(
        &unpacked(&format!("{fixture}.new.step.gz"), &bare_path, |t| t),
        None,
    )
    .unwrap();
    // A new file without PMI: the old file's answers carry, to be written.
    let r = diff(&old, &bare, &Map::new()).unwrap();
    assert_eq!(r.pmi.answers_from, "stored");
    assert!(!r.pmi.new_stored);
    assert_eq!(r.answers["hole.function:holes:7"], "general");
    let md = r.markdown("Plate", None);
    assert!(
        md.contains("read from the PMI Specify wrote in the old file"),
        "{md}"
    );
    assert!(md.contains("The new file has no Specify PMI yet"), "{md}");
    // A new file with its own: the difference from the answers carried.
    let new = written(
        "new",
        json!({ "part.material": "6061-T6", "hole.function:holes:7": "fit:H8" }),
    );
    let r = diff(&old, &new, &Map::new()).unwrap();
    assert!(r.pmi.new_stored);
    let changed: Vec<_> = r
        .pmi
        .stored
        .iter()
        .filter(|c| c.id == "hole.function:holes:7")
        .collect();
    assert_eq!(changed.len(), 1, "{:?}", r.pmi.stored);
    assert_eq!(changed[0].class, "changed");
    assert_eq!(
        (changed[0].old.as_deref(), changed[0].new.as_deref()),
        (Some("general"), Some("fit:H8"))
    );
    // Answers given win over the old file's.
    let given = answers(json!({ "part.material": "7075-T6" }));
    assert_eq!(diff(&old, &bare, &given).unwrap().pmi.answers_from, "given");
}

#[test]
fn a_real_part_s_report_names_what_a_person_changed() {
    // build123d's cast bearing unit: the flange 11 → 13 thick, the bolt holes Ø10 → Ø9.
    let r = revised(
        "bearing_flange",
        json!({
            "part.material": "EN AC-42100",
            "hole.function:hole_patterns.set:31.32": "clearance:M8",
            "datum.A": [17]
        }),
    );
    // The holes first; the rounds and drafted surfaces that follow them counted after.
    assert!(
        r.summary
            .starts_with("2 holes changed, 8 rounds and surfaces with them."),
        "{}",
        r.summary
    );
    let md = r.markdown("Bearing", None);
    assert!(
        md.contains(
            "- 2× Ø10 × 11 through holes, on the −Z side, at X 10.9, Y 28.4; X 87.4, Y 28.4 \
             from the −X, −Y corner: depth 11 → 13; diameter 10 → 9"
        ),
        "{md}"
    );
    // A drafted surface is reshaped, and a round riding it moves only as the flange grows.
    assert!(
        md.contains("- 4× freeform surfaces: reshaped; moved Z +2"),
        "{md}"
    );
    assert!(md.contains("- 2× R1 fillets, at "), "{md}");
    assert!(!md.contains("support."), "{md}");
    // The answer on the pair of holes is carried and listed to check, without the holes'
    // middles moving as they deepen.
    let check: Vec<&str> = r.check.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(check, ["hole.function:hole_patterns.set:31.32"]);
    assert!(
        md.contains("**clearance:M8** (`hole.function:hole_patterns.set:31.32`): depth 11 → 13; diameter 10 → 9\n"),
        "{md}"
    );
}
