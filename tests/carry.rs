//! `carry OLD NEW -o OUT` (Specify 0.3.0 piece 4, ADR P11 "Carry forward"): OLD's requirements
//! carried onto a new revision, file to file, through the in-place writer, all or nothing.

mod common;

use std::collections::BTreeMap;
use std::process::Command;

use haecceity::p21::Document;
use serde_json::{Value, json};
use specify_core::carry::{self, Outcome};
use specify_core::characteristics::{self, Characteristic};
use specify_core::edits;
use specify_core::part::{File, LoadedPart, load_bytes};
use specify_core::{api, resume, writer};

fn file(name: &str) -> File {
    File::read(&common::fixtures().join(name)).unwrap()
}

fn of_bytes(bytes: Vec<u8>) -> File {
    File::of_bytes(std::path::Path::new("part.step"), bytes).unwrap()
}

fn carried(old: &File, new: &File) -> Outcome {
    carry::carry(old, new).unwrap()
}

fn written(outcome: &Outcome) -> LoadedPart {
    let bytes = outcome
        .bytes
        .clone()
        .unwrap_or_else(|| panic!("refused: {:#?}", outcome.report.refused));
    load_bytes("out.step", bytes).unwrap()
}

/// `name` with an edit list applied by the in-place writer: an annotated OLD revision.
fn annotated(name: &str, list: Value) -> File {
    let loaded = file(name).load(None).unwrap();
    let applied = writer::apply::apply(&loaded, &edits::parse_value(list).unwrap())
        .unwrap_or_else(|f| panic!("refused: {f:#?}"));
    of_bytes(applied.bytes)
}

/// `name` with every suggestion of its table accepted, `but` left out.
fn with_suggestions(name: &str, but: &[&str]) -> File {
    let loaded = file(name).load(None).unwrap();
    let ids: Vec<String> = table(&loaded)["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["state"] == "gap" && !r["suggestion"].is_null())
        .map(|r| r["id"].as_str().unwrap().to_string())
        .filter(|id| !but.iter().any(|b| id.starts_with(b)))
        .collect();
    annotated(name, json!({ "accept_suggestions": ids }))
}

fn table(loaded: &LoadedPart) -> Value {
    api::analyse(loaded).unwrap()["table"].clone()
}

fn row(table: &Value, id: &str) -> Value {
    table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("no row {id}"))
}

fn numbers(loaded: &LoadedPart) -> BTreeMap<u64, Characteristic> {
    characteristics::read(loaded)
        .unwrap()
        .into_iter()
        .map(|c| (c.number, c))
        .collect()
}

fn ids(rows: &[carry::Carried]) -> Vec<&str> {
    rows.iter().map(|c| c.new.as_str()).collect()
}

/// Every entity's record text, by id.
fn records(bytes: &[u8]) -> BTreeMap<u64, String> {
    let doc = Document::parse(bytes.to_vec()).unwrap();
    doc.graph()
        .entities
        .keys()
        .map(|&id| {
            let span = doc.span(id).unwrap();
            (id, String::from_utf8_lossy(&doc.bytes()[span]).into_owned())
        })
        .collect()
}

#[test]
fn a_bare_new_revision_gets_everything() {
    // The enclosure's annotated revision onto the same geometry, bare.
    let old = file("revisions/enclosure_pmi.old.step.gz");
    let new = file("parts/electronics-enclosure-base.step.gz");
    let outcome = carried(&old, &new);
    let report = &outcome.report;
    assert!(report.written && report.refused.is_empty());
    assert!(report.kept.is_empty() && report.to_check.is_empty());
    let old_part = old.load(None).unwrap();
    let before = table(&old_part);
    let out = written(&outcome);
    let after = table(&out);
    // Every feature-and-kind the old file states is stated, and every decided none decided.
    for r in before["rows"].as_array().unwrap() {
        let id = r["id"].as_str().unwrap();
        match r["state"].as_str().unwrap() {
            "stated" => {
                assert!(ids(&report.carried).contains(&id), "{id} not carried");
                let now = row(&after, id);
                assert_eq!(now["state"], "stated", "{id}");
                assert_eq!(now["value"], r["value"], "{id}");
            }
            "decided_none" => {
                assert!(ids(&report.decided_none).contains(&id), "{id}");
                assert_eq!(row(&after, id)["state"], "decided_none", "{id}");
            }
            _ => {}
        }
    }
    // Their requirements keep the old file's numbers.
    let (was, now) = (numbers(&old_part), numbers(&out));
    let carried_rows: Vec<Value> = before["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["state"] == "stated")
        .cloned()
        .collect();
    let mut kept = 0;
    for r in &carried_rows {
        for req in r["requirements"].as_array().into_iter().flatten() {
            let Some(n) = req["number"].as_str() else {
                continue;
            };
            let n: u64 = n[1..].parse().unwrap();
            let c = &was[&n];
            let found = now
                .get(&n)
                .unwrap_or_else(|| panic!("#{n} ({} {:?}) lost", c.r#type, c.faces));
            assert_eq!((&found.r#type, &found.faces), (&c.r#type, &c.faces), "#{n}");
            kept += 1;
        }
    }
    assert!(kept >= 10, "{kept}");
    assert!(report.numbers_carried);
}

#[test]
fn a_bare_new_revision_with_changed_geometry_gets_everything_it_can() {
    // The gramil shaft with Specify's suggestions accepted, onto the bare next revision (an
    // end chamfer added): every stated feature-and-kind carried, the chamfer listed as new.
    let old = with_suggestions("revisions/gramil_shaft.old.step.gz", &["hole.full_thread"]);
    let new = file("revisions/gramil_shaft.new.step.gz");
    let outcome = carried(&old, &new);
    let report = &outcome.report;
    let after = table(&written(&outcome));
    let before = table(&old.load(None).unwrap());
    for r in before["rows"].as_array().unwrap() {
        if r["state"] == "stated" {
            let id = r["id"].as_str().unwrap();
            let entry = report
                .carried
                .iter()
                .chain(&report.to_check)
                .find(|c| c.old == id)
                .unwrap_or_else(|| panic!("{id}: {:#?}", report.not_carried));
            assert_eq!(row(&after, &entry.new)["state"], "stated", "{id}");
        }
    }
    // The thread depth is stated by the thread callout its hole's function writes.
    assert!(
        report
            .carried
            .iter()
            .any(|c| c.old.starts_with("hole.full_thread") && c.with.is_some()),
        "{:#?}",
        report.carried
    );
    assert!(
        report
            .new_unstated
            .iter()
            .any(|u| u.what.contains("chamfer")),
        "{:?}",
        report.new_unstated
    );
}

#[test]
fn new_revisions_own_requirements_stand_and_differences_are_reported() {
    // The enclosure: the new file states its Ø3.4 holes itself.
    let outcome = carried(
        &file("revisions/enclosure_pmi.old.step.gz"),
        &file("revisions/enclosure_pmi.new.step.gz"),
    );
    let kept = &outcome.report.kept;
    let holes = kept
        .iter()
        .find(|k| k.new.starts_with("hole.function:"))
        .unwrap_or_else(|| panic!("{kept:#?}"));
    assert_eq!(holes.old_value, "Size_Diameter 3 H11");
    assert_eq!(holes.new_value, "Size_Diameter 3.4 H11");
    // The spindle: hole 14's position is 0.3 in the old file and 0.2 in the new; the new
    // file's stands, in its file as it was.
    let new = file("revisions/spindle_pmi.new.step.gz");
    let outcome = carried(&file("revisions/spindle_pmi.old.step.gz"), &new);
    let report = &outcome.report;
    let position = report
        .kept
        .iter()
        .find(|k| k.new == "hole.position:holes:14")
        .unwrap();
    assert_eq!(position.old_value, "Position 0.3 | A");
    assert_eq!(position.new_value, "Position 0.2 | A");
    assert!(report.carried.is_empty() && report.to_check.is_empty());
    let out = written(&outcome);
    assert_eq!(
        row(&table(&out), "hole.position:holes:14")["value"],
        row(&table(&new.load(None).unwrap()), "hole.position:holes:14")["value"]
    );
    // Equal values are not reported.
    assert!(!kept_ids(report).contains(&"part.material"));
}

fn kept_ids(report: &carry::Report) -> Vec<&str> {
    report.kept.iter().map(|k| k.new.as_str()).collect()
}

#[test]
fn a_moved_feature_is_carried_and_flagged_to_check() {
    let old = with_suggestions("revisions/hole_moved.old.step.gz", &[]);
    let outcome = carried(&old, &file("revisions/hole_moved.new.step.gz"));
    let report = &outcome.report;
    let moved = report
        .to_check
        .iter()
        .find(|c| c.new == "hole.function:holes:8")
        .unwrap_or_else(|| panic!("{report:#?}"));
    assert!(
        moved.check.as_deref().unwrap().contains("position"),
        "{moved:?}"
    );
    // The holes that stayed are carried without a flag.
    assert!(ids(&report.carried).contains(&"hole.function:holes:7"));
    assert_eq!(
        row(&table(&written(&outcome)), "hole.function:holes:8")["state"],
        "stated"
    );
    assert!(report.markdown().contains("## Carried, to check"));
}

/// The hole_removed revision with four H7 holes, numbered #1-#4.
fn h7_holes() -> File {
    annotated(
        "revisions/hole_removed.old.step.gz",
        json!({"fill": {"hole.function:holes:6": "fit:H7", "hole.function:holes:7": "fit:H7",
                        "hole.function:holes:8": "fit:H7", "hole.function:holes:9": "fit:H7"}}),
    )
}

#[test]
fn an_unmatched_feature_is_not_written_and_is_reported() {
    let old = h7_holes();
    let outcome = carried(&old, &file("revisions/hole_removed.new.step.gz"));
    let report = &outcome.report;
    let gone = report
        .not_carried
        .iter()
        .find(|n| n.id.as_deref() == Some("hole.function:holes:7"))
        .unwrap_or_else(|| panic!("{report:#?}"));
    assert_eq!(gone.faces, [7]);
    assert!(gone.reason.contains("not found"), "{}", gone.reason);
    assert_eq!(report.carried.len(), 3);
    // Only the three holes still there are written.
    assert_eq!(numbers(&written(&outcome)).len(), 3);
}

#[test]
fn a_kind_specify_does_not_understand_is_reported_not_carried() {
    // The enclosure's planner dimensions sit on no feature-and-kind: each is listed with its
    // kind, type, faces and number.
    let outcome = carried(
        &file("revisions/enclosure_pmi.old.step.gz"),
        &file("parts/electronics-enclosure-base.step.gz"),
    );
    let report = &outcome.report;
    let file_pmi = table(
        &file("revisions/enclosure_pmi.old.step.gz")
            .load(None)
            .unwrap(),
    )["file_pmi"]
        .as_array()
        .unwrap()
        .clone();
    assert!(!file_pmi.is_empty());
    for item in &file_pmi {
        let n = report
            .not_carried
            .iter()
            .find(|n| n.number.as_deref() == item["number"].as_str())
            .unwrap_or_else(|| panic!("{item} not reported"));
        assert_eq!(n.kind, item["kind"].as_str().unwrap());
        assert_eq!(n.r#type, item["type"].as_str().unwrap());
        assert_eq!(json!(n.faces), item["faces"]);
        assert!(n.reason.contains("no feature-and-kind"), "{}", n.reason);
    }
    // And not written.
    let out = numbers(&written(&outcome));
    for item in &file_pmi {
        let n: u64 = item["number"].as_str().unwrap()[1..].parse().unwrap();
        assert!(!out.contains_key(&n), "#{n} written");
    }
    let md = report.markdown();
    assert!(md.contains("## Not carried"), "{md}");
}

#[test]
fn characteristic_numbers_carry_and_retired_numbers_stay_retired() {
    let old = h7_holes();
    let was = numbers(&old.load(None).unwrap());
    assert_eq!(was.len(), 4);
    let outcome = carried(&old, &file("revisions/hole_removed.new.step.gz"));
    let out = written(&outcome);
    let now = numbers(&out);
    // Each hole still there keeps its number (faces after 7 shift down by one).
    for c in was.values().filter(|c| c.faces != [7]) {
        let face = if c.faces[0] > 7 {
            c.faces[0] - 1
        } else {
            c.faces[0]
        };
        assert_eq!(
            now.values().find(|n| n.faces == [face]).unwrap().number,
            c.number
        );
    }
    // The removed hole's number is retired: a requirement added next takes a new one.
    let added = writer::apply::apply(
        &out,
        &edits::parse_value(json!({"fill": {"datum.A.flatness": "0.02"}})).unwrap(),
    );
    let added = match added {
        Ok(a) => a,
        // The part has no datum A to put a flatness on: fill it too.
        Err(_) => writer::apply::apply(
            &out,
            &edits::parse_value(json!({"fill": {"datum.A": [4], "datum.A.flatness": "0.02"}}))
                .unwrap(),
        )
        .unwrap(),
    };
    let max = was.keys().max().unwrap();
    let flatness = numbers(&load_bytes("x.step", added.bytes).unwrap())
        .into_values()
        .find(|c| c.r#type == "Flatness")
        .unwrap();
    assert!(flatness.number > *max, "{} <= {max}", flatness.number);
    // The transport is consumed: nothing of it is stored as an answer.
    let stored = resume::answers_for(&out).unwrap().unwrap_or_default();
    assert!(!stored.contains_key(characteristics::CARRY));
}

#[test]
fn untouched_entities_of_the_new_revision_keep_their_bytes() {
    // NEW carries another tool's PMI: the plate's requirements copied by haecceity's own
    // writer onto the bare plate (no Specify marker, stored answers or numbers). OLD is the
    // plate Specify wrote, then given a flatness on datum A, a process and no coating, which
    // NEW does not have.
    let plate = file("resume-p9/edited-plate.step.gz").load(None).unwrap();
    let theirs = another_tool(&plate.bytes);
    let new = of_bytes(theirs.clone());
    let old = annotated(
        "resume-p9/edited-plate.step.gz",
        json!({"fill": {"datum.A.flatness": "0.05", "part.process": "machined"},
               "none": ["part.coating"]}),
    );
    let outcome = carried(&old, &new);
    let report = &outcome.report;
    assert!(report.written, "{:#?}", report.refused);
    assert_eq!(
        ids(&report.carried),
        ["datum.A.flatness", "part.process"],
        "{report:#?}"
    );
    assert_eq!(ids(&report.decided_none), ["part.coating"]);
    // What both state alike is neither carried nor reported.
    assert!(report.kept.is_empty(), "{:#?}", report.kept);
    let after = table(&written(&outcome));
    assert_eq!(row(&after, "datum.A.flatness")["state"], "stated");
    assert_eq!(row(&after, "part.coating")["state"], "decided_none");
    let bytes = outcome.bytes.clone().unwrap();
    assert_ne!(bytes, theirs);
    let (before, after) = (records(&theirs), records(&bytes));
    for (id, text) in &before {
        assert_eq!(after.get(id), Some(text), "#{id} of the new file changed");
    }
}

/// `bytes`' PMI written by haecceity onto the bare plate (as `tests/edits_in_place.rs`).
fn another_tool(bytes: &[u8]) -> Vec<u8> {
    use haecceity::pmi;
    let defs = haecceity::step::read_part_definitions(bytes).unwrap();
    let read = pmi::read(&Document::parse(bytes.to_vec()).unwrap(), &defs).unwrap();
    let mut copied = read.parts[0].clone();
    for t in &mut copied.tolerances {
        t.description = None;
    }
    copied
        .attributes
        .retain(|set| !resume::is_answers(set) && set.name != characteristics::PROPERTY);
    let bare = File::read(&common::fixtures().join("parts/plate.step"))
        .unwrap()
        .bytes()
        .as_ref()
        .clone();
    let doc = Document::parse(bare).unwrap();
    let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
    let (edit, _) = pmi::write(
        &doc,
        &defs,
        &[(pmi::PartId(0), copied)],
        pmi::write::Mode::Replace,
        pmi::write::PresentationPolicy::RemovePresentation,
    )
    .unwrap();
    doc.apply(&edit).unwrap().bytes
}

#[test]
fn a_carry_that_cannot_be_applied_writes_nothing_and_says_why() {
    // NEW decided datum C none; OLD's hole positions cite A|B|C. NEW's decision stands, so the
    // positions cannot be written: the whole carry is refused.
    let new = annotated(
        "parts/electronics-enclosure-base.step.gz",
        json!({"none": ["datum.C"]}),
    );
    let outcome = carried(&file("revisions/enclosure_pmi.old.step.gz"), &new);
    let report = &outcome.report;
    assert!(outcome.bytes.is_none() && !report.written);
    assert!(
        report
            .refused
            .iter()
            .any(|f| f.entry.starts_with("fill[\"hole.position:") && f.reason.contains("datum.C")),
        "{:#?}",
        report.refused
    );
    assert!(report.kept.iter().any(|k| k.new == "datum.C"));
    assert!(
        report
            .markdown()
            .starts_with("# Carry\n\nNothing was written.")
    );

    // The command line: exit 2, the reasons on stderr, the report on stdout, no file.
    let dir = std::env::temp_dir().join(format!("specify-carry-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let new_path = dir.join("new.step");
    std::fs::write(&new_path, new.bytes().as_ref()).unwrap();
    let out = dir.join("out.step");
    let run = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("carry")
        .arg(common::fixtures().join("revisions/enclosure_pmi.old.step.gz"))
        .arg(&new_path)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("nothing was written"), "{stderr}");
    let printed: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(printed["written"], false);
    assert!(!out.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_command_line_writes_out_and_prints_the_report() {
    let dir = std::env::temp_dir().join(format!("specify-carry-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let old = dir.join("old.step");
    std::fs::write(&old, h7_holes().bytes().as_ref()).unwrap();
    let out = dir.join("out.step");
    let run = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .arg("carry")
        .arg(&old)
        .arg(common::fixtures().join("revisions/hole_removed.new.step.gz"))
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(
        run.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let printed: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(printed["written"], true);
    assert_eq!(printed["carried"].as_array().unwrap().len(), 3);
    assert!(printed["markdown"].as_str().unwrap().contains("## Carried"));
    let loaded = specify_core::part::load(&out, None).unwrap();
    assert_eq!(numbers(&loaded).len(), 3);
    std::fs::remove_dir_all(&dir).unwrap();
}
