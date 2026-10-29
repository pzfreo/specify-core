//! Two revisions of an assembly compared part by part: gramil's, with its shaft and drive
//! plate changed (tests/fixtures/revisions/gramil_assembly.py).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Map, Value, json};
use specify_core::part::load_all;
use specify_core::revision::assembly;

fn fixture(which: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "tests/fixtures/revisions/gramil_assembly.{which}.step.gz"
    ))
}

fn dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "specify-core-assembly-{name}-{}",
        std::process::id()
    ))
}

fn diff(out: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("diff")
        .args([fixture("old"), fixture("new")])
        .args(["--title", "gramil_assembly.step", "-o"])
        .arg(out)
        .args(extra)
        .output()
        .unwrap()
}

fn json_file(path: PathBuf) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_parts_pair_by_name_and_only_the_two_changed_parts_are_changed() {
    let (old, new) = (
        load_all(&fixture("old")).unwrap(),
        load_all(&fixture("new")).unwrap(),
    );
    let a = assembly::diff(&old, &new, &Map::new()).unwrap();
    assert_eq!(a.parts.len(), 12);
    let changed: Vec<&str> = a
        .parts
        .iter()
        .filter(|p| p.status == "changed")
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(changed, ["GRM-02 shaft", "GRM-04 drive plate"]);
    assert_eq!(
        a.parts.iter().filter(|p| p.status == "unchanged").count(),
        10
    );
    let retainer = a
        .parts
        .iter()
        .find(|p| p.name == "GRM-06 blade retainer")
        .unwrap();
    assert_eq!(
        (
            retainer.old.as_ref().unwrap().instances,
            retainer.new.as_ref().unwrap().instances
        ),
        (4, 4)
    );
    assert_eq!(a.verdict, "ok");
    assert_eq!(
        a.summary,
        "2 parts changed (GRM-02 shaft, GRM-04 drive plate), 10 unchanged."
    );
}

#[test]
fn the_command_line_writes_the_parts_table_each_changed_part_and_the_picture() {
    let out = dir("report");
    let viewer = "https://draftwright.io/diff?repo=o/r&base=b&head=h&path=a.step";
    let run = diff(&out, &["--viewer", viewer]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let printed: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(printed["verdict"], "ok");
    let md = std::fs::read_to_string(out.join("report.md")).unwrap();
    assert!(
        md.starts_with(
            "### gramil_assembly.step: Specify\n\n✅ 2 parts changed (GRM-02 shaft, GRM-04 drive \
             plate), 10 unchanged.\n\n![The assembly: changed parts in orange, added in green, \
             removed in red](model.png)\n\n| Part | Quantity | Change |\n|---|---|---|\n\
             | GRM-02 shaft | 1 | ✅ changed |\n| GRM-04 drive plate | 1 | ✅ changed |\n\n\
             10 other parts are unchanged.\n\n#### GRM-02 shaft\n\n"
        ),
        "{md}"
    );
    assert!(
        md.contains("#### GRM-04 drive plate\n\n✅ 1 hole changed."),
        "{md}"
    );
    assert!(
        md.contains(&format!(
            "[Turn GRM-04 drive plate round in 3D]({viewer}&part=GRM-04%20drive%20plate)"
        )),
        "{md}"
    );
    assert!(md.contains(": diameter 2.2 → 2.4\n"), "{md}");
    assert!(md.contains("- chamfer added"), "{md}");
    assert_eq!(md.matches("](model.png)").count(), 1, "one picture");
    let png = std::fs::read(out.join("model.png")).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    let report = json_file(out.join("report.json"));
    assert_eq!(report["parts"].as_array().unwrap().len(), 12);
    assert_eq!(report["parts"][1]["report"]["verdict"], "ok");
    assert_eq!(json_file(out.join("answers.json")), json!({}));
}

#[test]
fn an_assembly_s_answers_are_given_and_carried_by_part_name() {
    let out = dir("answers");
    std::fs::create_dir_all(&out).unwrap();
    let given = out.join("in.json");
    let write = |v: Value| std::fs::write(&given, v.to_string()).unwrap();
    let path = given.to_str().unwrap();

    write(json!({"GRM-04 drive plate": {"part.material": "6061-T6"}}));
    let run = diff(&out, &["--answers", path]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        json_file(out.join("answers.json")),
        json!({"GRM-04 drive plate": {"part.material": "6061-T6"}})
    );

    write(json!({"nope": {}}));
    let run = diff(&out, &["--answers", path]);
    assert_eq!(run.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("invalid answer: ") && stderr.contains("has no part named \"nope\""),
        "{stderr}"
    );

    write(json!({"part.material": "6061-T6"}));
    let run = diff(&out, &["--answers", path]);
    assert_eq!(run.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&run.stderr).contains(
            "invalid answer: an assembly's answers are {part name: {question id: value}}"
        )
    );
}

#[test]
fn one_part_of_an_assembly_is_still_compared_on_its_own() {
    let out = dir("part");
    let run = diff(&out, &["--part", "5"]);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let md = std::fs::read_to_string(out.join("report.md")).unwrap();
    assert!(
        md.starts_with("### gramil_assembly.step: Specify\n\n✅ 1 hole changed."),
        "{md}"
    );
    assert!(!md.contains("| Part |"), "{md}");
    assert!(json_file(out.join("report.json")).get("parts").is_none());
}
