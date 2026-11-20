//! Saving edits in place (Specify 0.3.0 piece 2, ADR P11): a save applies an explicit edit
//! list and touches only what it names. Every other entity keeps its bytes and its number,
//! whoever wrote it, and a change in Specify's rules never alters a file.

mod common;

use std::collections::BTreeMap;
use std::process::Command;

use haecceity::p21::Document;
use serde_json::{Value, json};
use specify_core::characteristics::{self, Characteristic};
use specify_core::edits::{self, Failure};
use specify_core::part::{LoadedPart, load, load_bytes};
use specify_core::writer::apply::{Applied, apply};
use specify_core::{api, resume};

fn fixture(name: &str) -> LoadedPart {
    load(&common::fixtures().join(name), None).unwrap()
}

fn reload(bytes: &[u8]) -> LoadedPart {
    load_bytes("edited.step", bytes.to_vec()).unwrap()
}

fn applied(loaded: &LoadedPart, list: Value) -> Applied {
    apply(loaded, &edits::parse_value(list).unwrap()).unwrap_or_else(|f| panic!("refused: {f:#?}"))
}

fn refused(loaded: &LoadedPart, list: Value) -> Vec<Failure> {
    match apply(loaded, &edits::parse_value(list).unwrap()) {
        Ok(a) => panic!("applied: {:?}", a.report),
        Err(f) => f,
    }
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
        .unwrap_or_else(|| panic!("no row {id} in {table}"))
}

/// The rows as (id, state, value).
fn rows(table: &Value) -> Vec<(String, Value, Value)> {
    table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap().to_string(),
                r["state"].clone(),
                r["value"].clone(),
            )
        })
        .collect()
}

fn numbers(loaded: &LoadedPart) -> BTreeMap<u64, Characteristic> {
    characteristics::read(loaded)
        .unwrap()
        .into_iter()
        .map(|c| (c.number, c))
        .collect()
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

/// The original entities that a save changed in place (same id, other text), other than a
/// set that only lost its references to entities the save removed (a removed requirement's
/// callout leaving the draughting model's list), and Specify's own Notes saved view, which
/// shows the part's notes and is updated with them (design notes, piece 2).
fn rewritten(before: &[u8], after: &[u8]) -> Vec<(u64, String, String)> {
    let (old, new) = (records(before), records(after));
    let refs = |text: &str| -> Vec<u64> {
        text.split('#')
            .skip(2)
            .filter_map(|t| {
                let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().ok()
            })
            .collect()
    };
    let shape = |text: &str| -> String {
        text.split('#')
            .map(|t| t.trim_start_matches(|c: char| c.is_ascii_digit()))
            .collect::<Vec<_>>()
            .join("")
            .replace(",)", ")")
            .replace("(,", "(")
            .replace(",,", ",")
    };
    old.iter()
        .filter_map(|(id, text)| {
            let now = new.get(id)?;
            if now == text
                || text.starts_with(&format!("#{id}=DRAUGHTING_MODEL('Notes',"))
                || text.starts_with(&format!("#{id}=DEFAULT_MODEL_GEOMETRIC_VIEW('Notes',"))
            {
                return None;
            }
            let (was, is) = (refs(text), refs(now));
            let only_dropped_removed = is.iter().all(|r| was.contains(r))
                && was
                    .iter()
                    .filter(|r| !is.contains(r))
                    .all(|r| !new.contains_key(r))
                && text.split('(').next() == now.split('(').next()
                && shape(text).replace(',', "") == shape(now).replace(',', "");
            (!only_dropped_removed).then(|| (*id, text.clone(), now.clone()))
        })
        .collect()
}

/// Asserts that every numbered requirement but `addressed` is in `after` unchanged: the same
/// number on the same entity, with the same record text, whoever wrote it.
fn untouched_but(before: &LoadedPart, after: &LoadedPart, addressed: &[u64]) {
    let (old, new) = (numbers(before), numbers(after));
    let (old_text, new_text) = (records(&before.bytes), records(&after.bytes));
    for (n, c) in &old {
        if addressed.contains(n) {
            continue;
        }
        let now = new
            .get(n)
            .unwrap_or_else(|| panic!("#{n} is gone: {:?}", new.keys()));
        assert_eq!(now.entity, c.entity, "#{n} moved");
        assert_eq!(now, c, "#{n} changed");
        assert_eq!(
            new_text[&c.entity], old_text[&c.entity],
            "#{n}'s record changed"
        );
    }
}

fn stored_answers(bytes: &[u8]) -> Value {
    resume::stored(bytes)
        .get(&0)
        .map(|p| p["answers"].clone())
        .unwrap_or(Value::Null)
}

/// `bytes` with every requirement copied by haecceity's own writer onto the bare plate: the
/// same PMI, written by another tool (no Specify marker, no stored answers, no numbers).
fn as_another_tool(bytes: &[u8]) -> Vec<u8> {
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
    let bare = fixture("parts/plate.step").bytes.to_vec();
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
fn an_unaddressed_item_keeps_its_bytes_whoever_wrote_it() {
    // Specify's own: the edited plate (written by Specify, then edited elsewhere).
    let plate = fixture("resume-p9/edited-plate.step.gz");
    let saved = applied(
        &plate,
        json!({"change": {"hole.position:holes:12": {"to": "0.1", "expect": "0.2"}}}),
    );
    let after = reload(&saved.bytes);
    untouched_but(&plate, &after, &[16]);
    // No record of the file was rewritten in place; the only ones that went are #16's and
    // Specify's own bookkeeping (stored answers, the number record).
    assert_eq!(rewritten(&plate.bytes, &saved.bytes), []);

    // Another tool's: the same PMI with no marker, stored answers or numbers.
    let theirs = load_bytes("plate.step", as_another_tool(&plate.bytes)).unwrap();
    let saved = applied(
        &theirs,
        json!({"fill": {"datum.A.flatness": "0.05"}, "none": ["part.coating"]}),
    );
    assert!(!saved.unchanged);
    let (old, new) = (records(&theirs.bytes), records(&saved.bytes));
    for (id, text) in &old {
        assert_eq!(
            new.get(id),
            Some(text),
            "#{id} of another tool's file changed"
        );
    }
    let after = reload(&saved.bytes);
    let t = table(&after);
    assert_eq!(row(&t, "datum.A.flatness")["state"], "stated");
    assert_eq!(row(&t, "part.coating")["state"], "decided_none");
    // Every other row reads as it did.
    let before = rows(&table(&theirs));
    for (id, state, value) in rows(&t) {
        if id != "datum.A.flatness" && id != "part.coating" {
            let was = before.iter().find(|(i, _, _)| *i == id).unwrap();
            assert_eq!((&was.1, &was.2), (&state, &value), "{id}");
        }
    }
}

#[test]
fn a_rules_change_alters_nothing() {
    // Captured Specify 0.1.15 output, written before the depth fix: today's rules would
    // dimension its hole depth differently. A save that addresses something else, or
    // nothing, leaves every requirement as it is.
    for name in [
        "characteristic-numbers-enclosure-curve-depth.step.gz",
        "characteristic-numbers-ctc04-main.step.gz",
    ] {
        let old = fixture(name);
        let nothing = applied(&old, json!({}));
        assert!(nothing.unchanged, "{name}");
        assert_eq!(nothing.bytes, old.bytes.to_vec(), "{name}");
        let saved = applied(&old, json!({"none": ["part.coating"]}));
        let after = reload(&saved.bytes);
        untouched_but(&old, &after, &[]);
        assert_eq!(numbers(&old).len(), numbers(&after).len(), "{name}");
        assert_eq!(rewritten(&old.bytes, &saved.bytes), [], "{name}");
        assert!(saved.retired.is_empty(), "{name}: {:?}", saved.retired);
        let (was, now) = (rows(&table(&old)), rows(&table(&after)));
        for ((id, s0, v0), (_, s1, v1)) in was.iter().zip(&now) {
            if id == "part.coating" {
                assert_eq!(s1, "decided_none");
            } else {
                assert_eq!((s0, v0), (s1, v1), "{name}: {id}");
            }
        }
    }
}

#[test]
fn a_change_of_limits_keeps_the_number() {
    let plate = fixture("resume-p9/edited-plate.step.gz");
    // The value the caller saw, as the table shows it, is accepted too.
    let shown = row(&table(&plate), "hole.position:holes:12")["value"].clone();
    for expect in [json!("0.2"), shown] {
        let saved = applied(
            &plate,
            json!({"change": {"hole.position:holes:12": {"to": "0.1", "expect": expect}}}),
        );
        assert!(saved.retired.is_empty(), "{:?}", saved.retired);
        let after = reload(&saved.bytes);
        let n = numbers(&after);
        assert_eq!(n.len(), numbers(&plate).len());
        assert_eq!(n[&16].r#type, "Position");
        assert_eq!(n[&16].faces, [12]);
        let r = row(&table(&after), "hole.position:holes:12");
        assert!(
            r["value"]
                .as_str()
                .unwrap()
                .contains("Position 0.1 | A | B | C"),
            "{r}"
        );
        assert!(
            r["requirements"]
                .as_array()
                .unwrap()
                .iter()
                .any(|q| q["number"] == "#16" && q["text"] == "Position 0.1 | A | B | C"),
            "{r}"
        );
    }
    // A 0.2 file (Specify's spindle): 0.3 to 0.25 on hole 14 keeps #13.
    let spindle = fixture("revisions/spindle_pmi.old.step.gz");
    let saved = applied(
        &spindle,
        json!({"change": {"hole.position:holes:14": {"to": "0.25", "expect": "0.3"}}}),
    );
    let after = reload(&saved.bytes);
    untouched_but(&spindle, &after, &[13]);
    let r = row(&table(&after), "hole.position:holes:14");
    assert!(
        r["value"].as_str().unwrap().contains("Position 0.25"),
        "{r}"
    );
    assert_eq!(r["requirements"][0]["number"], "#13");
}

#[test]
fn a_change_to_a_different_measurement_gets_a_new_number_and_retires_the_old() {
    // Hole 14 of the spindle: a clearance hole (its size, #3) made a tapped M4 (a thread).
    let spindle = fixture("revisions/spindle_pmi.old.step.gz");
    let before = numbers(&spindle);
    let high = *before.keys().max().unwrap();
    let saved = applied(
        &spindle,
        json!({"change": {"hole.function:holes:14": {"to": "tapped:M4", "expect": "Size_Diameter 3.3"}}}),
    );
    assert_eq!(saved.retired, [3]);
    let after = reload(&saved.bytes);
    let now = numbers(&after);
    assert!(!now.contains_key(&3), "{now:?}");
    let thread: Vec<&Characteristic> = now.values().filter(|c| c.number > high).collect();
    assert_eq!(thread.len(), 1, "{now:?}");
    assert_eq!(thread[0].kind, "thread");
    assert_eq!(thread[0].faces, [14]);
    untouched_but(&spindle, &after, &[3]);
    let r = row(&table(&after), "hole.function:holes:14");
    assert_eq!(r["state"], "stated");
    assert_eq!(r["requirements"][0]["number"], format!("#{}", high + 1));
    // The retired number is never reused: a later addition takes the next one.
    let later = applied(
        &after,
        json!({"change": {"hole.position:holes:14": {"to": "0.2", "expect": "0.3"}}}),
    );
    let later = reload(&later.bytes);
    assert!(!numbers(&later).contains_key(&3));
}

#[test]
fn remove_retires_its_number_and_retired_numbers_are_never_reused() {
    let plate = fixture("resume-p9/edited-plate.step.gz");
    let before = numbers(&plate);
    let high = *before.keys().max().unwrap();
    // #5 is the plate's length, on no recognised feature; #18 B's perpendicularity.
    let saved = applied(&plate, json!({"items": {"remove": ["#5", "#18"]}}));
    assert_eq!(saved.retired, [5, 18]);
    let after = reload(&saved.bytes);
    let now = numbers(&after);
    assert_eq!(now.len(), before.len() - 2);
    assert!(!now.contains_key(&5) && !now.contains_key(&18));
    untouched_but(&plate, &after, &[5, 18]);
    // B's control is a gap again, unless it is decided none in the same list.
    assert_eq!(row(&table(&after), "datum.B.control")["state"], "gap");
    let decided = applied(
        &plate,
        json!({"items": {"remove": ["#18"]}, "none": ["datum.B.control"]}),
    );
    assert_eq!(
        row(&table(&reload(&decided.bytes)), "datum.B.control")["state"],
        "decided_none"
    );
    // Put back, B's control is a new requirement with a new number; so is the flatness.
    let again = applied(
        &after,
        json!({"fill": {"datum.B.control": "0.1", "datum.A.flatness": "0.05"}}),
    );
    let again = reload(&again.bytes);
    let now = numbers(&again);
    let added: Vec<u64> = now
        .keys()
        .copied()
        .filter(|n| !before.contains_key(n))
        .collect();
    assert_eq!(added, [high + 1, high + 2], "{now:?}");
    assert!(!now.contains_key(&5) && !now.contains_key(&18));
    untouched_but(&plate, &again, &[5, 18]);
}

#[test]
fn a_stale_expect_is_refused_and_nothing_is_written() {
    let plate = fixture("resume-p9/edited-plate.step.gz");
    // The file says 0.2: someone changed it since the caller read 0.1. With a good entry.
    let failures = refused(
        &plate,
        json!({
            "change": {"hole.position:holes:12": {"to": "0.05", "expect": "0.1"}},
            "items": {"remove": ["#5"]},
        }),
    );
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].entry, r#"change["hole.position:holes:12"]"#);
    assert!(failures[0].reason.contains("expected"), "{failures:?}");
    // A fill on a stated row is a change, refused; and so is changing one item by number.
    let failures = refused(
        &plate,
        json!({
            "fill": {"hole.position:holes:12": "0.1"},
            "items": {"change": {"#16": {"tolerance": "0.1", "expect": "Position 0.2 | A | B | C"}}},
        }),
    );
    let entries: Vec<&str> = failures.iter().map(|f| f.entry.as_str()).collect();
    assert!(
        entries.contains(&r#"fill["hole.position:holes:12"]"#),
        "{failures:?}"
    );
    assert!(
        entries.contains(&r##"items.change["#16"]"##),
        "{failures:?}"
    );
}

#[test]
fn a_removal_never_reaches_a_kept_item_through_a_shared_aspect_or_datum() {
    let plate = fixture("resume-p9/edited-plate.step.gz");
    // #16 (hole 12's position) cites A|B|C, as #13 (the grid's) does. #9 is hole 9's size, on
    // the bore the plate's locations #11 and #12 measure to.
    let saved = applied(&plate, json!({"items": {"remove": ["#16", "#9"]}}));
    let after = reload(&saved.bytes);
    untouched_but(&plate, &after, &[16, 9]);
    assert_eq!(rewritten(&plate.bytes, &saved.bytes), []);
    let now = numbers(&after);
    for n in [1, 2, 3, 11, 12, 13] {
        assert!(now.contains_key(&n), "#{n}: {now:?}");
    }
    // A datum a kept frame cites cannot go on its own: refused, nothing written.
    let failures = refused(&plate, json!({"items": {"remove": ["#3"]}}));
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].reason.contains("cannot be taken out"),
        "{failures:?}"
    );
}

#[test]
fn an_added_control_on_the_same_face_is_a_change_that_keeps_its_number() {
    // The plate with datum A's flatness 0.05: a second flatness on face 4 is a change.
    let plate = fixture("parts/plate.step");
    let first = applied(
        &plate,
        json!({
            "fill": {"part.material": "6082-T6", "datum.A.flatness": "0.05"},
            "accept_suggestions": ["datum.A"],
        }),
    );
    let first = reload(&first.bytes);
    let flatness = |p: &LoadedPart| {
        numbers(p)
            .into_values()
            .filter(|c| c.r#type == "Flatness")
            .collect::<Vec<_>>()
    };
    let was = flatness(&first);
    assert_eq!(was.len(), 1);
    // As a fill it is refused, all or nothing.
    let failures = refused(&first, json!({"fill": {"datum.A.flatness": "0.02"}}));
    assert_eq!(
        failures[0].entry, r#"fill["datum.A.flatness"]"#,
        "{failures:?}"
    );
    // As a change it replaces the control, which keeps its number.
    let changed = applied(
        &first,
        json!({"change": {"datum.A.flatness": {"to": "0.02", "expect": "0.05"}}}),
    );
    let changed = reload(&changed.bytes);
    let now = flatness(&changed);
    assert_eq!(now.len(), 1);
    assert_eq!(now[0].number, was[0].number);
    assert_eq!(
        row(&table(&changed), "datum.A.flatness")["value"],
        "Flatness 0.02"
    );
}

#[test]
fn only_decided_none_and_choices_are_stored() {
    // The 0.2 spindle stores every answer; a save keeps only what leaves no PMI behind.
    let spindle = fixture("revisions/spindle_pmi.old.step.gz");
    let saved = applied(&spindle, json!({"none": ["part.process"]}));
    let stored = stored_answers(&saved.bytes);
    let keys: Vec<&str> = stored
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "part.locating",
            "part.coating",
            "part.heat_treatment",
            "part.notes",
            "datum.C",
            "datum.B.control",
            "part.process"
        ]
    );
}

fn cli(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_specify-core"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn write_applies_an_edit_list_all_or_nothing() {
    let dir = std::env::temp_dir().join(format!("specify-in-place-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let step = common::fixtures().join("resume-p9/edited-plate.step.gz");
    let edits = dir.join("edits.json");
    let out = dir.join("out.step");
    let step_s = step.to_str().unwrap();
    let (edits_s, out_s) = (edits.to_str().unwrap(), out.to_str().unwrap());

    std::fs::write(
        &edits,
        r##"{"change": {"hole.position:holes:12": {"to": "0.1", "expect": "0.3"}},
             "items": {"remove": ["#5"]}}"##,
    )
    .unwrap();
    // Checked against the file: the stale expect is named, nothing written.
    let (code, stdout, _) = cli(&["write", step_s, edits_s, "--check"]);
    assert_eq!(code, 2, "{stdout}");
    let outcome: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(outcome["checked_against_file"], true);
    assert_eq!(
        outcome["failures"].as_array().unwrap().len(),
        1,
        "{outcome}"
    );
    let (code, _, stderr) = cli(&["write", step_s, edits_s, "-o", out_s]);
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains(r#"change["hole.position:holes:12"]"#),
        "{stderr}"
    );
    assert!(!out.exists());

    std::fs::write(
        &edits,
        r##"{"change": {"hole.position:holes:12": {"to": "0.1", "expect": "0.2"}},
             "items": {"remove": ["#5"]}}"##,
    )
    .unwrap();
    let (code, stdout, stderr) = cli(&["write", step_s, edits_s, "-o", out_s]);
    assert_eq!(code, 0, "{stderr}");
    let report: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["retired"], json!([5]), "{report}");
    let after = load(&out, None).unwrap();
    assert!(!numbers(&after).contains_key(&5));
    assert!(
        row(&table(&after), "hole.position:holes:12")["value"]
            .as_str()
            .unwrap()
            .contains("Position 0.1")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_session_saves_an_edit_list_in_place_on_its_own_copy() {
    use specify_core::session::{self, Opened, Session};
    use std::sync::{Arc, Mutex};
    let dir = std::env::temp_dir().join(format!("specify-session-edits-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plate.step");
    std::fs::write(&path, &*fixture("resume-p9/edited-plate.step.gz").bytes).unwrap();
    let shared = Arc::new(Mutex::new(
        Session::open(Opened::read(&path).unwrap(), true).unwrap(),
    ));
    let list = |v: Value| edits::parse_value(v).unwrap();
    // A refused list writes nothing.
    let before = std::fs::read(&path).unwrap();
    let err = session::save_edits(
        &shared,
        &list(json!({"items": {"remove": ["#404"]}})),
        "mcp",
    )
    .unwrap_err();
    assert!(err.to_string().contains("#404"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // Two saves: the second applies to what the first left.
    let first = list(json!({"change": {"hole.position:holes:12": {"to": "0.1", "expect": "0.2"}}}));
    session::save_edits(&shared, &first, "mcp").unwrap();
    let saved =
        session::save_edits(&shared, &list(json!({"items": {"remove": ["#5"]}})), "mcp").unwrap();
    assert_eq!(saved.report["retired"], json!([5]));
    let after = load(&path, None).unwrap();
    assert!(!numbers(&after).contains_key(&5));
    assert!(
        row(&table(&after), "hole.position:holes:12")["value"]
            .as_str()
            .unwrap()
            .contains("Position 0.1")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_part_setting_change_replaces_only_the_setting() {
    let plate = fixture("resume-p9/edited-plate.step.gz");
    let saved = applied(
        &plate,
        json!({"change": {"part.material": {"to": "6061-T6", "expect": "6082-T6"}}}),
    );
    let after = reload(&saved.bytes);
    untouched_but(&plate, &after, &[]);
    assert_eq!(rewritten(&plate.bytes, &saved.bytes), []);
    let (was, now) = (rows(&table(&plate)), rows(&table(&after)));
    for ((id, s0, v0), (_, s1, v1)) in was.iter().zip(&now) {
        if id == "part.material" {
            assert_eq!(v1, "6061-T6");
        } else {
            assert_eq!((s0, v0), (s1, v1), "{id}");
        }
    }
}
