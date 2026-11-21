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
    let own = own_notes_view(&old);
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
            if now == text || own.contains(id) {
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

/// Specify's own Notes saved view in a file Specify wrote: the `DEFAULT_MODEL_GEOMETRIC_VIEW`
/// named Notes and the `DRAUGHTING_MODEL` named Notes it shows (haecceity's owned pairing).
/// Empty for a file Specify did not write, whatever its views are called.
fn own_notes_view(records: &BTreeMap<u64, String>) -> Vec<u64> {
    if !records
        .values()
        .any(|t| t.contains(&format!("'{}'", specify_core::writer::PMI_PROVENANCE)))
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (id, text) in records {
        let Some(rest) = text.strip_prefix(&format!("#{id}=DEFAULT_MODEL_GEOMETRIC_VIEW('Notes',"))
        else {
            continue;
        };
        // ('Notes',$,#camera,#model,'Notes',...): the model is the second reference.
        let refs: Vec<u64> = rest
            .split('#')
            .skip(1)
            .filter_map(|t| t.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok())
            .collect();
        if let Some(model) = refs.get(1)
            && records
                .get(model)
                .is_some_and(|m| m.starts_with(&format!("#{model}=DRAUGHTING_MODEL('Notes',")))
        {
            out.extend([*id, *model]);
        }
    }
    out
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
    another_tool(bytes, |_| {})
}

/// [`as_another_tool`], with `edit` changing the copied PMI first.
fn another_tool(bytes: &[u8], edit: impl FnOnce(&mut haecceity::pmi::PartPmi)) -> Vec<u8> {
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
    edit(&mut copied);
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
    let saved = applied(&spindle, json!({"items": {"remove": ["#11"]}}));
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
            "datum.B.control"
        ]
    );
    // Every file saved here reads again.
    for list in [
        json!({"items": {"remove": ["#11"]}}),
        json!({"none": ["part.notes"]}),
        json!({"accept_suggestions": ["part.process"]}),
    ] {
        let part = fixture("revisions/spindle_pmi.new.step.gz");
        let saved = applied(&part, list.clone());
        api::analyse(&reload(&saved.bytes)).unwrap_or_else(|e| panic!("{list}: {e}"));
    }
}

#[test]
fn none_for_a_choice_without_a_none_option_is_refused() {
    // part.process is machined, printed, …: "none" is not one of them, and a file storing
    // it would not read again.
    let spindle = fixture("revisions/spindle_pmi.old.step.gz");
    for list in [
        json!({"none": ["part.process"]}),
        json!({"fill": {"part.process": "none"}}),
    ] {
        let failures = refused(&spindle, list.clone());
        assert_eq!(failures.len(), 1, "{list}: {failures:?}");
        assert!(failures[0].entry.contains("part.process"), "{failures:?}");
        assert!(
            failures[0]
                .reason
                .contains("is not an answer to part.process"),
            "{failures:?}"
        );
        let checked = specify_core::writer::apply::check(&spindle, &list.to_string());
        assert_eq!(checked.failures, failures);
    }
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
        None,
        &list(json!({"items": {"remove": ["#404"]}})),
        "mcp",
    )
    .unwrap_err();
    assert!(err.to_string().contains("#404"), "{err}");
    assert!(
        matches!(&err, session::SaveError::Edits(f) if f.len() == 1 && f[0].entry.contains("#404")),
        "{err:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // A save at a rev the session has moved on from is refused, and writes nothing.
    let first = list(json!({"change": {"hole.position:holes:12": {"to": "0.1", "expect": "0.2"}}}));
    let rev = session::lock(&shared).rev();
    let stale = session::save_edits(&shared, Some(rev + 1), &first, "page").unwrap_err();
    assert!(
        matches!(stale, session::SaveError::Stale(r) if r == rev),
        "{stale:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // Two saves: the second applies to what the first left.
    session::save_edits(&shared, Some(rev), &first, "page").unwrap();
    let saved = session::save_edits(
        &shared,
        None,
        &list(json!({"items": {"remove": ["#5"]}})),
        "mcp",
    )
    .unwrap();
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

// ----- the review's probes, as tests -----

/// The part's PMI as haecceity reads it.
fn pmi_of(bytes: &[u8]) -> haecceity::pmi::PartPmi {
    haecceity::pmi::read(
        &Document::parse(bytes.to_vec()).unwrap(),
        &haecceity::step::read_part_definitions(bytes).unwrap(),
    )
    .unwrap()
    .parts[0]
        .clone()
}

/// The location dimensions on face `face`, as (from faces, to faces, basic).
fn locations_on(p: &haecceity::pmi::PartPmi, face: usize) -> Vec<(Vec<usize>, Vec<usize>, bool)> {
    use haecceity::pmi::{DimTolerance, DimensionKind};
    use specify_core::existing::faces_of;
    p.dimensions
        .iter()
        .filter_map(|d| match &d.kind {
            DimensionKind::Location { from, to, .. } => {
                let (a, b) = (faces_of(p, &[*from]), faces_of(p, &[*to]));
                (a.contains(&face) || b.contains(&face)).then_some((
                    a,
                    b,
                    matches!(d.tolerance, DimTolerance::Basic),
                ))
            }
            DimensionKind::Size { .. } => None,
        })
        .collect()
}

/// The edited plate's PMI as another tool's, with hole 12 (feature 6, face 12) unframed: its
/// position tolerance dropped and its locations plain (`moved`: its own 95 measured from face
/// 2, not face 0; `alone`: its own 95 dropped, leaving only the 30 it shares with the grid).
fn unframed_hole_12(moved: bool, alone: bool) -> LoadedPart {
    use haecceity::pmi::{DimTolerance, DimensionKind, ToleranceTarget};
    let plate = fixture("resume-p9/edited-plate.step.gz");
    let bytes = another_tool(&plate.bytes, |p| {
        p.tolerances
            .retain(|t| !matches!(&t.target, ToleranceTarget::Feature(f) if f.0 == 6));
        // dim 2 is hole 12's own 95 basic from face 0 (feature 2).
        if alone {
            p.dimensions.remove(2);
            for t in &mut p.tolerances {
                if let ToleranceTarget::Dimension(d) = &mut t.target
                    && d.0 > 2
                {
                    d.0 -= 1;
                }
            }
        } else if moved && let DimensionKind::Location { from, .. } = &mut p.dimensions[2].kind {
            from.0 = 0; // face 2
        }
        for d in &mut p.dimensions {
            if matches!(d.kind, DimensionKind::Location { .. })
                && matches!(d.tolerance, DimTolerance::Basic)
            {
                d.tolerance = DimTolerance::None;
            }
        }
    });
    load_bytes("theirs.step", bytes).unwrap()
}

#[test]
fn a_plain_location_states_a_hole_so_a_fill_on_it_is_refused() {
    let theirs = unframed_hole_12(false, false);
    let t = table(&theirs);
    assert_eq!(row(&t, "hole.position:holes:12")["state"], "stated", "{t}");
    for list in [
        json!({"fill": {"hole.position:holes:12": "0.2"}}),
        json!({"accept_suggestions": ["hole.position:holes:12"]}),
    ] {
        let failures = refused(&theirs, list.clone());
        assert_eq!(failures.len(), 1, "{list}: {failures:?}");
        assert!(
            failures[0].reason.contains("already states"),
            "{failures:?}"
        );
    }
}

#[test]
fn a_change_never_adds_a_location_beside_another_tools() {
    // Hole 12 located by another tool's plain 95 from face 2 (Specify would plan a basic 95
    // from face 0) and a plain 30 it shares with the grid (Specify would plan a basic 30 of
    // its own). Its own 95 could be replaced, but the frame would add a 30 beside the shared
    // one: refused, naming it, and nothing written.
    let theirs = unframed_hole_12(true, false);
    let before = locations_on(&pmi_of(&theirs.bytes), 12);
    assert_eq!(before.len(), 2, "{before:?}");
    let shown = row(&table(&theirs), "hole.position:holes:12")["value"].clone();
    let failures = refused(
        &theirs,
        json!({"change": {"hole.position:holes:12": {"to": "0.2", "expect": shown}}}),
    );
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].reason.contains("Location_LinearDistance 30.0"),
        "{failures:?}"
    );
    // With the shared 30 basic (as Specify's own plate has it), the same change applies and
    // adds no location: the frame's basic dimensions are the file's.
    let plate = fixture("resume-p9/edited-plate.step.gz");
    let before = locations_on(&pmi_of(&plate.bytes), 12);
    let saved = applied(
        &plate,
        json!({"change": {"hole.position:holes:12": {"to": "0.3", "expect": "0.2"}}}),
    );
    assert_eq!(locations_on(&pmi_of(&saved.bytes), 12), before);
    // Stated only by the shared 30: refused, naming it, and nothing written.
    let alone = unframed_hole_12(false, true);
    let r = row(&table(&alone), "hole.position:holes:12");
    assert_eq!(r["state"], "stated", "{r}");
    let failures = refused(
        &alone,
        json!({"change": {"hole.position:holes:12": {"to": "0.2", "expect": r["value"]}}}),
    );
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].reason.contains("Location_LinearDistance 30.0"),
        "{failures:?}"
    );
}

#[test]
fn a_general_tolerance_fill_writes_the_class_alone() {
    for name in [
        "revisions/cadgen_pmi_plate.step.gz",
        "revisions/complex_pattern.old.step.gz",
    ] {
        let part = fixture(name);
        let p0 = pmi_of(&part.bytes);
        let saved = applied(
            &part,
            json!({"accept_suggestions": ["part.general_tolerance"]}),
        );
        let p1 = pmi_of(&saved.bytes);
        assert_eq!(p1.dimensions.len(), p0.dimensions.len(), "{name}");
        assert_eq!(p1.tolerances.len(), p0.tolerances.len(), "{name}");
        assert_eq!(p1.general.len(), p0.general.len() + 1, "{name}");
        let (was, now) = (rows(&table(&part)), rows(&table(&reload(&saved.bytes))));
        for ((id, s0, v0), (_, s1, v1)) in was.iter().zip(&now) {
            if id != "part.general_tolerance" {
                assert_eq!((s0, v0), (s1, v1), "{name}: {id}");
            }
        }
    }
}

#[test]
fn accepting_specifys_own_suggestion_applies_or_is_refused_by_the_check_alike() {
    // A pattern left to the general tolerance writes its size under it: on a part with no
    // general tolerance, the check and the write refuse it alike, saying so; with the general
    // tolerance in the same list, it writes that one size and the class, nothing else.
    let part = fixture("revisions/complex_pattern.old.step.gz");
    let id = "hole.function:hole_patterns.rectangle:8.9.11.12";
    assert_eq!(row(&table(&part), id)["suggestion"], "general");
    let alone = json!({"accept_suggestions": [id]});
    let failures = refused(&part, alone.clone());
    assert!(
        failures[0].reason.contains("part.general_tolerance"),
        "{failures:?}"
    );
    let checked = specify_core::writer::apply::check(&part, &alone.to_string());
    assert_eq!(checked.failures, failures);
    let saved = applied(
        &part,
        json!({"accept_suggestions": [id, "part.general_tolerance"]}),
    );
    let (p0, p1) = (pmi_of(&part.bytes), pmi_of(&saved.bytes));
    assert_eq!(p1.dimensions.len(), p0.dimensions.len() + 1);
    let t = table(&reload(&saved.bytes));
    assert_eq!(row(&t, id)["state"], "stated", "{t}");
    // A position on a part with no datums: the check refuses it with apply's reason, and
    // filling the datums in the same list applies it.
    let cadgen = fixture("revisions/cadgen_pmi_plate.step.gz");
    let list = json!({"accept_suggestions": ["hole.position:holes:6"]});
    let failures = refused(&cadgen, list.clone());
    assert!(failures[0].reason.contains("datum"), "{failures:?}");
    let checked = specify_core::writer::apply::check(&cadgen, &list.to_string());
    assert_eq!(checked.failures, failures);
    let datums: Vec<String> = rows(&table(&cadgen))
        .into_iter()
        .map(|(id, _, _)| id)
        .filter(|id| id.strip_prefix("datum.").is_some_and(|l| !l.contains('.')))
        .collect();
    let mut all = vec!["hole.position:holes:6".to_string()];
    all.extend(datums);
    let saved = applied(&cadgen, json!({"accept_suggestions": all}));
    let t = table(&reload(&saved.bytes));
    assert_eq!(row(&t, "hole.position:holes:6")["state"], "stated", "{t}");
}

#[test]
fn every_numbered_requirement_can_be_removed_by_its_number() {
    for (name, n) in [
        ("revisions/enclosure_pmi.old.step.gz", 23),
        ("revisions/spindle_pmi.new.step.gz", 14),
        ("characteristic-numbers-ctc04-main.step.gz", 154),
    ] {
        let part = fixture(name);
        let t = table(&part);
        let all = t.to_string();
        for c in numbers(&part).keys() {
            assert!(
                all.contains(&format!("\"#{c}\"")),
                "{name}: #{c} not in the table"
            );
        }
        let finish = row(&t, "part.surface_finish");
        assert!(
            finish["requirements"]
                .as_array()
                .unwrap()
                .iter()
                .any(|q| q["number"] == format!("#{n}")),
            "{name}: {finish}"
        );
        let saved = applied(&part, json!({"items": {"remove": [format!("#{n}")]}}));
        assert_eq!(saved.retired, [n], "{name}");
        let after = reload(&saved.bytes);
        untouched_but(&part, &after, &[n]);
        assert_eq!(
            row(&table(&after), "part.surface_finish")["state"],
            "gap",
            "{name}"
        );
    }
}

#[test]
fn a_change_to_none_takes_the_basic_dimensions_only_its_frame_used() {
    let plate = fixture("resume-p9/edited-plate.step.gz");
    // Hole 12: #16 its frame, #15 the 95 basic only it uses, #14 the 30 it shares with the
    // grid.
    let saved = applied(
        &plate,
        json!({"change": {"hole.position:holes:12": {"to": "none", "expect": "0.2"}}}),
    );
    let mut retired = saved.retired.clone();
    retired.sort();
    assert_eq!(retired, [15, 16]);
    let after = reload(&saved.bytes);
    untouched_but(&plate, &after, &[15, 16]);
    // What stands is the 30 it shares with the grid, which still locates it.
    let r = row(&table(&after), "hole.position:holes:12");
    let left: Vec<&Value> = r["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| &q["number"])
        .collect();
    assert_eq!(left, [&json!("#14")], "{r}");
    // The report says so, and what follows: with #14 removed later, the row reads decided none.
    assert!(
        saved
            .report
            .warnings
            .iter()
            .any(|w| w.starts_with(
                "hole.position:holes:12: changed to none, but Location_LinearDistance 30.0 basic"
            ) && w.ends_with("hole.position:holes:12 reads decided none")),
        "{:?}",
        saved.report.warnings
    );
    let later = applied(&after, json!({"items": {"remove": ["#14"]}}));
    assert_eq!(
        row(&table(&reload(&later.bytes)), "hole.position:holes:12")["state"],
        "decided_none"
    );
}

#[test]
fn a_second_edits_save_keeps_the_first_saves_edits() {
    use specify_core::session::{self, Opened, Session};
    use std::sync::{Arc, Mutex};
    let dir = std::env::temp_dir().join(format!("specify-edits-then-edits-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plate.step");
    std::fs::write(&path, &*fixture("resume-p9/edited-plate.step.gz").bytes).unwrap();
    let shared = Arc::new(Mutex::new(
        Session::open(Opened::read(&path).unwrap(), true).unwrap(),
    ));
    let list = json!({
        "change": {"hole.position:holes:12": {"to": "0.1", "expect": "0.2"}},
        "items": {"remove": ["#5"]},
    });
    session::save_edits(&shared, None, &edits::parse_value(list).unwrap(), "mcp").unwrap();
    let none = json!({"none": ["part.coating"]});
    session::save_edits(&shared, None, &edits::parse_value(none).unwrap(), "page").unwrap();
    let after = load(&path, None).unwrap();
    assert!(!numbers(&after).contains_key(&5));
    assert!(
        row(&table(&after), "hole.position:holes:12")["value"]
            .as_str()
            .unwrap()
            .contains("Position 0.1")
    );
    assert_eq!(row(&table(&after), "part.coating")["state"], "decided_none");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The review's sweep: every gap with a suggestion, accepted alone, either applies and changes
/// no other row, or is refused, by the check alike.
#[test]
fn accepting_any_one_suggestion_changes_that_row_alone() {
    for name in [
        "resume-p9/edited-plate.step.gz",
        "revisions/spindle_pmi.old.step.gz",
        "revisions/spindle_pmi.new.step.gz",
        "revisions/enclosure_pmi.old.step.gz",
        "revisions/xcaf_pmi_plate.step.gz",
        "revisions/cadgen_pmi_plate.step.gz",
        "revisions/presentation_plate.step.gz",
        "revisions/complex_pattern.old.step.gz",
    ] {
        let part = fixture(name);
        let t = table(&part);
        let gaps: Vec<String> = t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| {
                r["state"] == "gap"
                    && r["suggestion"].is_string()
                    && !specify_core::feature_table::is_none_value(&r["suggestion"])
            })
            .map(|r| r["id"].as_str().unwrap().to_string())
            .collect();
        for id in gaps {
            let list = json!({"accept_suggestions": [&id]});
            let checked = specify_core::writer::apply::check(&part, &list.to_string());
            match apply(&part, &edits::parse_value(list).unwrap()) {
                Err(failures) => assert_eq!(checked.failures, failures, "{name}: {id}"),
                Ok(saved) => {
                    assert!(checked.applicable, "{name}: {id}: {checked:?}");
                    let now = rows(&table(&reload(&saved.bytes)));
                    for ((rid, s0, v0), (_, s1, v1)) in rows(&t).iter().zip(&now) {
                        if *rid != id {
                            assert_eq!((s0, v0), (s1, v1), "{name}: {id} changed {rid}");
                        } else {
                            assert_ne!(s1, "gap", "{name}: {id} still a gap");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn an_edits_save_is_refused_while_answers_are_unsaved() {
    use specify_core::session::{self, Opened, Session};
    use std::sync::{Arc, Mutex};
    let dir = std::env::temp_dir().join(format!("specify-unsaved-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plate.step");
    std::fs::write(&path, &*fixture("resume-p9/edited-plate.step.gz").bytes).unwrap();
    let shared = Arc::new(Mutex::new(
        Session::open(Opened::read(&path).unwrap(), true).unwrap(),
    ));
    let before = std::fs::read(&path).unwrap();
    {
        let mut s = session::lock(&shared);
        let mut answers = s.answers(0).cloned().unwrap_or_default();
        answers.insert("part.coating".into(), json!("anodise clear"));
        s.set(0, answers, "page");
    }
    let remove = edits::parse_value(json!({"items": {"remove": ["#5"]}})).unwrap();
    let err = session::save_edits(&shared, None, &remove, "mcp").unwrap_err();
    assert!(
        err.to_string()
            .contains("save or discard the unsaved answers first"),
        "{err}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        session::lock(&shared).answers(0).unwrap()["part.coating"],
        "anodise clear"
    );
    // Discarded (the answers back as the file holds them), the edits save goes ahead.
    {
        let mut s = session::lock(&shared);
        let mut answers = s.answers(0).cloned().unwrap_or_default();
        answers.remove("part.coating");
        s.set(0, answers, "page");
    }
    session::save_edits(&shared, None, &remove, "mcp").unwrap();
    let after = load(&path, None).unwrap();
    assert!(!numbers(&after).contains_key(&5));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every question id, decided none three ways (none, a fill of "none", a change to "none"):
/// each is refused, or written and read again by `analyse` without error.
#[test]
fn every_none_is_refused_or_reads_again() {
    for name in [
        "resume-p9/edited-plate.step.gz",
        "revisions/spindle_pmi.new.step.gz",
        "revisions/enclosure_pmi.old.step.gz",
        "revisions/enclosure_pmi.new.step.gz",
    ] {
        let part = fixture(name);
        let t = table(&part);
        for r in t["rows"].as_array().unwrap() {
            let id = r["id"].as_str().unwrap();
            for list in [
                json!({"none": [id]}),
                json!({"fill": {id: "none"}}),
                json!({"change": {id: {"to": "none", "expect": r["value"]}}}),
            ] {
                if let Ok(saved) = apply(&part, &edits::parse_value(list.clone()).unwrap())
                    && !saved.unchanged
                {
                    api::analyse(&reload(&saved.bytes))
                        .unwrap_or_else(|e| panic!("{name} {list}: written, then {e}"));
                }
            }
        }
    }
}

/// A choice that is not a requirement and has no feature-table row (how holes are located,
/// `part.locating`; writer decision 4): a fill sets it, validated against its options, and it is
/// stored beside the PMI; the same value again changes nothing; a value it does not offer, or
/// none (not one of its options), refuses the whole list. (Asked of a part with holes and no
/// datums of its own: the bare plate.)
#[test]
fn a_choice_with_no_row_is_filled_and_stored() {
    let part = fixture("parts/plate.step");
    let t = table(&part);
    assert!(
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["id"] != "part.locating"),
        "part.locating has no row"
    );
    let saved = applied(&part, json!({"fill": {"part.locating": "± dimensions"}}));
    assert!(!saved.unchanged);
    let after = reload(&saved.bytes);
    assert_eq!(
        stored_answers(&saved.bytes)["part.locating"],
        "± dimensions"
    );
    assert_eq!(
        api::analyse(&after).unwrap()["resume"]["answers"]["part.locating"],
        "± dimensions"
    );
    let again = applied(&after, json!({"fill": {"part.locating": "± dimensions"}}));
    assert!(again.unchanged, "the same choice again changes nothing");
    assert_eq!(again.bytes, saved.bytes);
    for bad in [
        json!({"fill": {"part.locating": "by eye"}}),
        json!({"none": ["part.locating"]}),
    ] {
        let failures = refused(&part, bad.clone());
        assert!(
            failures.iter().any(|f| f.entry.contains("part.locating")),
            "{bad}: {failures:?}"
        );
    }
}

/// An added requirement (`extra.<name>`, the editor's added requirements): a fill of its spec
/// adds it, as a 0.2 answer did, all or nothing with the rest of the list; a spec the rules refuse
/// fails its entry and nothing is written.
#[test]
fn a_fill_of_an_added_requirement_adds_it() {
    let part = fixture("parts/plate.step");
    let saved = applied(
        &part,
        json!({"fill": {"part.material": "steel",
                        "extra.1": {"kind": "size", "faces": [9], "value": "7.8/8.2"}}}),
    );
    let limits: Vec<_> = specify_core::existing::read_existing(&reload(&saved.bytes))
        .unwrap()
        .into_iter()
        .filter_map(|e| e.limits)
        .collect();
    assert_eq!(limits, [[7.8, 8.2]], "{:?}", saved.report.written);
    let bad = refused(
        &part,
        json!({"fill": {"part.material": "steel",
                        "extra.1": {"kind": "size", "faces": [9999], "value": "7.8/8.2"}}}),
    );
    assert!(bad.iter().any(|f| f.entry.contains("extra.1")), "{bad:?}");
}

/// `clear` takes back a stored decision that leaves no PMI (a decided none, a stored choice):
/// the row is a gap, the choice unanswered, again. It never touches PMI: a clear of something
/// stated, or of a gap, is refused.
#[test]
fn clear_takes_back_a_stored_decision_and_never_pmi() {
    let part = fixture("parts/plate.step");
    let decided = applied(
        &part,
        json!({"none": ["part.coating"], "fill": {"part.material": "steel",
               "part.locating": "± dimensions"}}),
    );
    let after = reload(&decided.bytes);
    assert_eq!(row(&table(&after), "part.coating")["state"], "decided_none");
    assert_eq!(
        stored_answers(&decided.bytes)["part.locating"],
        "± dimensions"
    );
    let cleared = applied(&after, json!({"clear": ["part.coating", "part.locating"]}));
    let back = reload(&cleared.bytes);
    assert_eq!(row(&table(&back), "part.coating")["state"], "gap");
    assert!(
        stored_answers(&cleared.bytes)
            .get("part.locating")
            .is_none()
    );
    assert_eq!(
        row(&table(&back), "part.material")["state"],
        "stated",
        "PMI untouched"
    );
    for (bad, why) in [
        (json!({"clear": ["part.material"]}), "never touches PMI"),
        (
            json!({"clear": ["part.surface_finish"]}),
            "nothing is stored",
        ),
        (
            json!({"clear": ["part.coating"], "none": ["part.coating"]}),
            "keep one of them",
        ),
    ] {
        let failures = refused(&after, bad.clone());
        assert!(
            failures.iter().any(|f| f.reason.contains(why)),
            "{bad}: {failures:?}"
        );
    }
}

/// A 0.2 answers file carries its face evidence: one saved on another revision of the part is
/// refused as stale, by `check` and by `apply` alike, naming `_specify.evidence`, as 0.2's write
/// refused it; on its own revision it applies.
#[test]
fn an_answers_file_from_another_revision_is_refused_as_stale() {
    let old = fixture("revisions/plate_split.old.step.gz");
    let new = fixture("revisions/plate_split.new.step.gz");
    let given = json!({"part.material": "6061-T6"});
    let analysis = api::analyse(&old).unwrap();
    let saved = api::export_answers(
        &old,
        &analysis,
        given.as_object().unwrap(),
        &Default::default(),
    )
    .unwrap()
    .answers;
    let text = Value::Object(saved).to_string();
    let checked = specify_core::writer::apply::check(&new, &text);
    let stale = |f: &Failure| f.entry == "_specify.evidence" && f.reason.contains("stale answers");
    assert!(checked.failures.iter().any(stale), "{checked:?}");
    let list = edits::read(&text).unwrap();
    let refused = apply(&new, &list).expect_err("refused");
    assert!(refused.iter().any(stale), "{refused:?}");
    assert!(apply(&old, &list).is_ok(), "on its own revision it applies");
}
