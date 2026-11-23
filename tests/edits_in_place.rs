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
        assert!(r["value"].as_str().unwrap().contains("⌖ ⌀0.1 A B C"), "{r}");
        assert!(
            r["requirements"]
                .as_array()
                .unwrap()
                .iter()
                .any(|q| q["number"] == "#16" && q["text"] == "⌖ ⌀0.1 A B C"),
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
    assert!(r["value"].as_str().unwrap().contains("⌖ ⌀0.25"), "{r}");
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
        json!({"change": {"hole.function:holes:14": {"to": "tapped:M4", "expect": "⌀3.3 H11"}}}),
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
fn removing_and_refilling_the_same_measurement_retires_its_number() {
    let original = applied(
        &fixture("parts/plate.step"),
        json!({"fill": {"extra.size": {"kind": "size", "faces": [9], "value": "+0.2/-0.1"}}}),
    );
    let original = reload(&original.bytes);
    let number = *numbers(&original).keys().next().unwrap();
    let id = format!("#{number}");
    let fill = json!({"extra.size": {"kind": "size", "faces": [9], "value": "+0.5/-0.3"}});
    let both = applied(&original, json!({"items": {"remove": [id]}, "fill": fill}));
    let removed = applied(&original, json!({"items": {"remove": [id]}}));
    let separate = applied(&reload(&removed.bytes), json!({"fill": fill}));
    assert_eq!(both.retired, [number]);
    assert_eq!(removed.retired, [number]);
    assert!(separate.retired.is_empty());
    for bytes in [&both.bytes, &separate.bytes] {
        let saved = reload(bytes);
        let after = numbers(&saved);
        assert_eq!(after.keys().copied().collect::<Vec<_>>(), [number + 1]);
        assert_eq!(after[&(number + 1)].faces, [9]);
        let existing = api::analyse(&saved).unwrap()["existing"].clone();
        assert_eq!(existing[0]["upper"], 0.5);
        assert_eq!(existing[0]["lower"], -0.3);
    }
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
            "items": {"change": {"#16": {"tolerance": "0.1", "expect": "⌖ ⌀0.2 A B C"}}},
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
    assert_eq!(row(&table(&changed), "datum.A.flatness")["value"], "⏥ 0.02");
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
fn cli_rejects_whole_intent_write_with_migration_instructions() {
    let step = common::fixtures().join("parts/plate.step");
    let tmp = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("cli-retired-write");
    std::fs::create_dir_all(&tmp).unwrap();
    let analysis = tmp.join("analysis.json");
    let answers = tmp.join("answers.json");
    let out = tmp.join("out.step");
    let _ = std::fs::remove_file(&out);
    std::fs::write(
        &analysis,
        api::analyse_file(&step, None).unwrap().to_string(),
    )
    .unwrap();
    std::fs::write(&answers, r#"{"part.material":"steel"}"#).unwrap();
    for flags in [vec![], vec!["--accept-defaults"]] {
        let mut args = vec![
            "write",
            step.to_str().unwrap(),
            analysis.to_str().unwrap(),
            answers.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ];
        args.extend(flags);
        let (code, stdout, stderr) = cli(&args);
        assert_eq!(code, 2, "{stdout} {stderr}");
        assert!(stdout.is_empty(), "{stdout}");
        for text in [
            "whole-intent write was removed",
            "write STEP EDITS.json",
            "accept_suggestions",
            "docs/answers.md",
        ] {
            assert!(stderr.contains(text), "missing {text}: {stderr}");
        }
        assert!(!out.exists());
    }
    std::fs::remove_dir_all(tmp).unwrap();
}

#[test]
fn issue_228_cli_edit_list_writes_answered_rows_and_lists_open_gaps() {
    let step = common::fixtures().join("parts/plate.step");
    let tmp = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("cli-issue-228");
    std::fs::create_dir_all(&tmp).unwrap();
    let edits = tmp.join("edits.json");
    let out = tmp.join("out.step");
    std::fs::write(
        &edits,
        r#"{"fill":{"part.material":"steel","part.general_tolerance":"ISO 2768-m"}}"#,
    )
    .unwrap();
    let (code, stdout, stderr) = cli(&[
        "write",
        step.to_str().unwrap(),
        edits.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr}");
    let report: Value = serde_json::from_str(&stdout).unwrap();
    let after = api::analyse_file(&out, None).unwrap();
    let rows = after["table"]["rows"].as_array().unwrap();
    let open: Vec<_> = rows
        .iter()
        .filter(|r| r["open"] == true)
        .map(|r| &r["id"])
        .collect();
    assert_eq!(report["missing"], json!(open));
    assert!(open.contains(&&json!("part.surface_finish")), "{open:?}");
    assert_eq!(after["part"]["material"], "steel");
    assert_eq!(after["part"]["general_tolerance"], "ISO 2768-m");
    assert_eq!(row(&after["table"], "part.surface_finish")["state"], "gap");
    std::fs::remove_dir_all(tmp).unwrap();
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
            .contains("⌖ ⌀0.1")
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
            .contains("⌖ ⌀0.1")
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
    // Hole 12 located by another tool's plain 95 from face 2 and a plain 30 it shares with the
    // grid, or by the shared 30 alone. Under decision D8 a location dimension locates only its
    // located side, per direction: the shared 30s locate hole 12 across y only, so its
    // location is a gap missing x, not a stated row to change. A change is refused (use fill),
    // and nothing is written.
    for (moved, alone) in [(true, false), (false, true)] {
        let theirs = unframed_hole_12(moved, alone);
        let r = row(&table(&theirs), "hole.position:holes:12");
        assert_eq!(
            (&r["state"], &r["missing"], &r["located"]),
            (&json!("gap"), &json!(["x"]), &json!(["y"])),
            "{r}"
        );
        let failures = refused(
            &theirs,
            json!({"change": {"hole.position:holes:12": {"to": "0.2", "expect": "0.2"}}}),
        );
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].reason.contains("use fill"), "{failures:?}");
    }
    // With the shared 30 basic (as Specify's own plate has it), the change applies and adds no
    // location: the frame's basic dimensions are the file's.
    let plate = fixture("resume-p9/edited-plate.step.gz");
    let before = locations_on(&pmi_of(&plate.bytes), 12);
    let saved = applied(
        &plate,
        json!({"change": {"hole.position:holes:12": {"to": "0.3", "expect": "0.2"}}}),
    );
    assert_eq!(locations_on(&pmi_of(&saved.bytes), 12), before);
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
    // What stands is the 30 it shares with the grid (#14) and the plate's plain 30 to hole 9
    // (#12). Under D8 they locate hole 12 in one direction only, so the row is no longer
    // stated: it reads the none just decided, listing what locates it so far.
    let r = row(&table(&after), "hole.position:holes:12");
    let left: Vec<&Value> = r["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| &q["number"])
        .collect();
    assert_eq!(r["state"], "decided_none", "{r}");
    assert_eq!(left, [&json!("#14"), &json!("#12")], "{r}");
    // The report says so, and what follows: with #14 removed later, the row reads decided none.
    assert!(
        saved.report.warnings.iter().any(|w| w
            .starts_with("hole.position:holes:12: changed to none, but 30 (basic)")
            && w.ends_with("hole.position:holes:12 reads decided none")),
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
            .contains("⌖ ⌀0.1")
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
                    let now = table(&reload(&saved.bytes));
                    // The planner lists gaps first and recovers stated dimensions afterwards.
                    // Compare every original row by id so that reordering cannot hide a change.
                    for (rid, s0, v0) in rows(&t) {
                        let after = now["rows"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|r| r["id"] == rid);
                        // One hole measurement can close several gaps in its direction.
                        // Check that its single new item states each additional stated row.
                        if id.starts_with("location:holes:")
                            && rid != id
                            && rid.starts_with("location:holes:")
                            && s0 == "gap"
                            && after.is_none_or(|r| r["state"] == "stated")
                        {
                            let new = numbers(&reload(&saved.bytes));
                            let old = numbers(&part);
                            let added: Vec<_> =
                                new.keys().filter(|n| !old.contains_key(n)).collect();
                            assert_eq!(added.len(), 1, "{name}: {id}");
                            if let Some(after) = after {
                                let number = format!("#{}", added[0]);
                                let items = after["requirements"].as_array().unwrap();
                                assert!(
                                    !items.is_empty()
                                        && items.iter().all(|r| r["number"] == number)
                                );
                            }
                            continue;
                        }
                        let after =
                            after.unwrap_or_else(|| panic!("{name}: {id} removed row {rid}"));
                        let (s1, v1) = (&after["state"], &after["value"]);
                        if rid != id {
                            assert_eq!((&s0, &v0), (s1, v1), "{name}: {id} changed {rid}");
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
fn a_page_save_does_not_silently_drop_working_answers() {
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
    let err = session::save_edits(&shared, None, &remove, "page").unwrap_err();
    assert!(
        err.to_string().contains("include the staged answer"),
        "{err}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        session::lock(&shared).answers(0).unwrap()["part.coating"],
        "anodise clear"
    );
    session::discard_answers(&shared, None, None, "page").unwrap();
    session::save_edits(&shared, None, &remove, "page").unwrap();
    let after = load(&path, None).unwrap();
    assert!(!numbers(&after).contains_key(&5));
    let _ = std::fs::remove_dir_all(&dir);
}

// Parts of an assembly (Paul, 2026-11-21: in-place edits on parts inside an assembly are
// required for 0.3.0). `parts/assembly.step`: a plate (part 0) and a pin (part 1) placed twice.

const PLATE: usize = 0;
const PIN: usize = 1;

fn assembly() -> Vec<u8> {
    std::fs::read(common::fixtures().join("parts").join("assembly.step")).unwrap()
}

/// Part `k` of the assembly `bytes`, as `--part K` and `open_part` load it.
fn part_of(bytes: &[u8], k: usize) -> LoadedPart {
    specify_core::part::File::of_bytes(std::path::Path::new("assembly.step"), bytes.to_vec())
        .unwrap()
        .load(Some(k))
        .unwrap()
}

fn plate_list() -> Value {
    json!({"fill": {
        "part.material": "steel",
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
        "hole.function:holes:11": "tapped:M5",
        "hole.position:holes:11": "0.25",
    }})
}

fn pin_list() -> Value {
    json!({"fill": {
        "part.material": "brass",
        "datum.A": [0], "datum.B": [6],
        "hole.function:holes:3.5": "tapped:M5",
        "hole.position:holes:3.5": "0.25",
    }})
}

/// Every entity of part `k`'s PMI (each collection's records, its stored answers and number
/// record included), as haecceity's reader attributes them.
fn pmi_entities(bytes: &[u8], k: usize) -> Vec<u64> {
    let defs = haecceity::step::read_part_definitions(bytes).unwrap();
    let read = haecceity::pmi::read(&Document::parse(bytes.to_vec()).unwrap(), &defs).unwrap();
    let p = &read.provenance.parts[k];
    [
        &p.features,
        &p.datums,
        &p.datum_targets,
        &p.dimensions,
        &p.tolerances,
        &p.general,
        &p.threads,
        &p.knurls,
        &p.notes,
        &p.surface_textures,
        &p.attributes,
    ]
    .into_iter()
    .flatten()
    .flatten()
    .copied()
    .chain(p.material.iter().copied())
    .collect()
}

/// The records `after` has that `before` has not: what a save wrote.
fn written_by(before: &[u8], after: &[u8]) -> std::collections::BTreeSet<u64> {
    let old = records(before);
    records(after)
        .into_keys()
        .filter(|id| !old.contains_key(id))
        .collect()
}

/// Asserts that every record of `before` is in `after` with the same text, but those in
/// `may_go` (the edited part's own items, which the save may take out, and its own callouts
/// model, which then lists fewer callouts).
fn only_part_changed(before: &[u8], after: &[u8], may_go: &std::collections::BTreeSet<u64>) {
    let (old, new) = (records(before), records(after));
    for (id, text) in &old {
        match new.get(id) {
            Some(now) => assert!(
                now == text || may_go.contains(id),
                "#{id} changed: {text} -> {now}"
            ),
            None => assert!(may_go.contains(id), "#{id} went: {text}"),
        }
    }
}

#[test]
fn one_part_of_an_assembly_is_edited_and_every_other_part_keeps_its_bytes() {
    let bare = assembly();
    let pin_rows = rows(&table(&part_of(&bare, PIN)));
    // The plate: every record of the file (the pin's definition, both its placements, the
    // assembly's structure) keeps its bytes, and the pin reads as it did.
    let plate = applied(&part_of(&bare, PLATE), plate_list());
    only_part_changed(&bare, &plate.bytes, &Default::default());
    assert_eq!(rows(&table(&part_of(&plate.bytes, PIN))), pin_rows);
    let t = table(&part_of(&plate.bytes, PLATE));
    for id in ["part.material", "hole.position:holes:11", "datum.C"] {
        assert_eq!(row(&t, id)["state"], "stated", "{id}");
    }
    // Then the pin: the plate's PMI, numbers and stored state keep their bytes.
    let plate_rows = rows(&t);
    let both = applied(&part_of(&plate.bytes, PIN), pin_list());
    only_part_changed(&plate.bytes, &both.bytes, &Default::default());
    untouched_but(
        &part_of(&plate.bytes, PLATE),
        &part_of(&both.bytes, PLATE),
        &[],
    );
    assert_eq!(rows(&table(&part_of(&both.bytes, PLATE))), plate_rows);
    // A change and a removal on the pin, which Specify wrote: only the pin's own items go.
    let pin = part_of(&both.bytes, PIN);
    let n = numbers(&pin);
    let position = n.values().find(|c| c.r#type == "Position").unwrap().number;
    let edited = applied(
        &pin,
        json!({
            "change": {"hole.position:holes:3.5": {"to": "0.1", "expect": "0.25"}},
            "none": ["part.coating"],
        }),
    );
    // What goes is the pin's own: what writing the pin wrote (its stored answers and number
    // record, the changed frame and its presentation).
    let pins_own = written_by(&plate.bytes, &both.bytes);
    only_part_changed(&both.bytes, &edited.bytes, &pins_own);
    assert_eq!(rewritten(&both.bytes, &edited.bytes), []);
    let after = part_of(&edited.bytes, PIN);
    untouched_but(&pin, &after, &[position]);
    assert_eq!(numbers(&after)[&position].r#type, "Position");
    assert!(
        row(&table(&after), "hole.position:holes:3.5")["value"]
            .as_str()
            .unwrap()
            .contains("⌖ ⌀0.1"),
    );
    assert_eq!(row(&table(&after), "part.coating")["state"], "decided_none");
    assert_eq!(rows(&table(&part_of(&edited.bytes, PLATE))), plate_rows);
    // An assembly-level item (anything not of a part: structure, placements) is in the bare
    // file; every one of its records survived each save.
    only_part_changed(&bare, &edited.bytes, &Default::default());
}

#[test]
fn another_tools_pmi_on_another_part_keeps_its_bytes() {
    // Both parts' PMI written by another tool (haecceity's own writer, as `another_tool`):
    // no Specify marker, no stored answers, no numbers.
    let written = applied(
        &part_of(
            &applied(&part_of(&assembly(), PLATE), plate_list()).bytes,
            PIN,
        ),
        pin_list(),
    )
    .bytes;
    let defs = haecceity::step::read_part_definitions(&written).unwrap();
    let read = haecceity::pmi::read(&Document::parse(written.clone()).unwrap(), &defs).unwrap();
    let items: Vec<_> = [PLATE, PIN]
        .into_iter()
        .map(|k| {
            let mut copied = read.parts[k].clone();
            for t in &mut copied.tolerances {
                t.description = None;
            }
            copied
                .attributes
                .retain(|set| !resume::is_answers(set) && set.name != characteristics::PROPERTY);
            (haecceity::pmi::PartId(k), copied)
        })
        .collect();
    // The plate first, then the pin: what the second write adds is the pin's.
    let write = |bytes: Vec<u8>, item: &(haecceity::pmi::PartId, haecceity::pmi::PartPmi)| {
        let doc = Document::parse(bytes).unwrap();
        let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
        let (edit, _) = haecceity::pmi::write(
            &doc,
            &defs,
            std::slice::from_ref(item),
            haecceity::pmi::write::Mode::Replace,
            haecceity::pmi::write::PresentationPolicy::RemovePresentation,
        )
        .unwrap();
        doc.apply(&edit).unwrap().bytes
    };
    let plate_only = write(assembly(), &items[0]);
    let foreign = write(plate_only.clone(), &items[1]);
    let pins = written_by(&plate_only, &foreign);
    assert!(resume::stored(&foreign).is_empty());
    assert!(numbers(&part_of(&foreign, PLATE)).is_empty());
    let plate_rows = rows(&table(&part_of(&foreign, PLATE)));
    // A fill, a decided none and a removal on the pin.
    let pin = part_of(&foreign, PIN);
    let saved = applied(
        &pin,
        json!({"fill": {"part.coating": "Zinc plate"}, "none": ["part.heat_treatment"],
               "change": {"hole.position:holes:3.5": {"to": "0.1", "expect": "0.25"}}}),
    );
    // What goes is the pin's changed frame, of what the pin's write added.
    only_part_changed(&foreign, &saved.bytes, &pins);
    assert_eq!(rewritten(&foreign, &saved.bytes), []);
    // Every entity of the plate's foreign PMI is there, byte for byte.
    let (old, new) = (records(&foreign), records(&saved.bytes));
    let plate_ids = pmi_entities(&foreign, PLATE);
    assert!(!plate_ids.is_empty());
    for id in plate_ids {
        assert_eq!(new.get(&id), old.get(&id), "the plate's #{id}");
    }
    assert_eq!(rows(&table(&part_of(&saved.bytes, PLATE))), plate_rows);
    let t = table(&part_of(&saved.bytes, PIN));
    assert_eq!(row(&t, "part.coating")["value"], "Zinc plate");
    assert_eq!(row(&t, "part.heat_treatment")["state"], "decided_none");
    assert!(
        row(&t, "hole.position:holes:3.5")["value"]
            .as_str()
            .unwrap()
            .contains("⌖ ⌀0.1")
    );
}

#[test]
fn a_part_placed_twice_is_edited_once_and_both_placements_show_it() {
    let bare = assembly();
    let placements = |bytes: &[u8]| -> Vec<usize> {
        haecceity::step::read_part_definitions(bytes)
            .unwrap()
            .iter()
            .map(|d| d.placements.len())
            .collect()
    };
    assert_eq!(placements(&bare), [1, 2]);
    let saved = applied(&part_of(&bare, PIN), pin_list());
    // Still two definitions, the pin placed twice; its thread and position are written once,
    // on the one definition both placements show.
    assert_eq!(placements(&saved.bytes), [1, 2]);
    let defs = haecceity::step::read_part_definitions(&saved.bytes).unwrap();
    let read = haecceity::pmi::read(&Document::parse(saved.bytes.clone()).unwrap(), &defs).unwrap();
    assert_eq!(read.parts[PIN].threads.len(), 1);
    assert_eq!(read.parts[PIN].tolerances.len(), 1);
    assert_eq!(read.parts[PLATE], haecceity::pmi::PartPmi::default());
    let listed = api::parts_of(
        &specify_core::part::File::of_bytes(std::path::Path::new("x.step"), saved.bytes.clone())
            .unwrap(),
    );
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[PIN]["instances"], 2);
    let n = numbers(&part_of(&saved.bytes, PIN));
    assert_eq!(
        n.values().filter(|c| c.r#type == "Internal").count(),
        1,
        "{n:?}"
    );
}

#[test]
fn a_refusal_on_one_part_of_an_assembly_writes_nothing_anywhere() {
    use specify_core::session::{self, Opened, Session};
    use std::sync::{Arc, Mutex};
    let plate = applied(&part_of(&assembly(), PLATE), plate_list()).bytes;
    // One good entry and one unknown: all or nothing.
    let bad = json!({"fill": {"part.material": "brass", "hole.size:holes:404": "5"}});
    let failures = refused(&part_of(&plate, PIN), bad.clone());
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].entry.contains("holes:404"), "{failures:?}");
    // The command line and the session write nothing either.
    let dir = std::env::temp_dir().join(format!("specify-asm-refused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (step, list, out) = (dir.join("a.step"), dir.join("e.json"), dir.join("o.step"));
    std::fs::write(&step, &plate).unwrap();
    std::fs::write(&list, bad.to_string()).unwrap();
    let s = |p: &std::path::Path| p.to_str().unwrap().to_string();
    let (code, _, stderr) = cli(&["write", &s(&step), &s(&list), "--part", "1", "-o", &s(&out)]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("holes:404"), "{stderr}");
    assert!(!out.exists());
    let shared = Arc::new(Mutex::new(
        Session::open(Opened::read(&step).unwrap(), true).unwrap(),
    ));
    let err = session::save_part_edits(
        &shared,
        Some(PIN),
        None,
        &edits::parse_value(bad).unwrap(),
        "mcp",
    )
    .unwrap_err();
    assert!(err.to_string().contains("holes:404"), "{err}");
    assert_eq!(std::fs::read(&step).unwrap(), plate);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn characteristic_numbers_are_per_part_in_an_assembly() {
    let plate = applied(&part_of(&assembly(), PLATE), plate_list()).bytes;
    let both = applied(&part_of(&plate, PIN), pin_list()).bytes;
    let (p, q) = (
        numbers(&part_of(&both, PLATE)),
        numbers(&part_of(&both, PIN)),
    );
    assert_eq!(
        p.keys().copied().collect::<Vec<_>>(),
        (1..=7).collect::<Vec<_>>()
    );
    assert_eq!(
        q.keys().copied().collect::<Vec<_>>(),
        (1..=4).collect::<Vec<_>>()
    );
    // Removing the pin's #3 retires the pin's #3; the plate's #3 stands, and the pin's next
    // number is its own 5.
    let removed = applied(&part_of(&both, PIN), json!({"items": {"remove": ["#3"]}}));
    assert_eq!(removed.retired, [3]);
    untouched_but(&part_of(&both, PLATE), &part_of(&removed.bytes, PLATE), &[]);
    let again = applied(
        &part_of(&removed.bytes, PIN),
        json!({"fill": {"hole.position:holes:3.5": "0.3"}}),
    );
    let q = numbers(&part_of(&again.bytes, PIN));
    assert!(!q.contains_key(&3), "{q:?}");
    assert_eq!(q.keys().max(), Some(&5));
    assert_eq!(numbers(&part_of(&again.bytes, PLATE)), p);
}

#[test]
fn write_and_the_session_edit_one_part_of_an_assembly() {
    use specify_core::session::{self, Opened, Session};
    use std::sync::{Arc, Mutex};
    let dir = std::env::temp_dir().join(format!("specify-asm-write-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (step, list, out) = (dir.join("a.step"), dir.join("e.json"), dir.join("o.step"));
    std::fs::write(&step, assembly()).unwrap();
    std::fs::write(&list, pin_list().to_string()).unwrap();
    let s = |p: &std::path::Path| p.to_str().unwrap().to_string();
    // Without --part the file must say which part.
    let (code, _, stderr) = cli(&["write", &s(&step), &s(&list), "-o", &s(&out)]);
    assert_ne!(code, 0, "{stderr}");
    assert!(!out.exists());
    let (code, stdout, stderr) = cli(&["write", &s(&step), &s(&list), "--check", "--part", "1"]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, _, stderr) = cli(&["write", &s(&step), &s(&list), "--part", "1", "-o", &s(&out)]);
    assert_eq!(code, 0, "{stderr}");
    let written = std::fs::read(&out).unwrap();
    only_part_changed(&assembly(), &written, &Default::default());
    assert_eq!(
        row(&table(&part_of(&written, PIN)), "part.material")["value"],
        "brass"
    );
    // Selecting a part does not bring back the retired analysis and answers form.
    let (code, _, stderr) = cli(&[
        "write",
        &s(&step),
        &s(&list),
        &s(&list),
        "--part",
        "1",
        "-o",
        &s(&out),
    ]);
    assert_eq!(code, 2, "{stderr}");
    assert!(
        stderr.contains("whole-intent write was removed")
            && stderr.contains("write STEP EDITS.json")
            && stderr.contains("docs/answers.md"),
        "{stderr}"
    );
    assert_eq!(std::fs::read(&out).unwrap(), written);

    // The session: the plate, then the pin, each saved in place.
    let shared = Arc::new(Mutex::new(
        Session::open(Opened::read(&step).unwrap(), true).unwrap(),
    ));
    let parse = |v: Value| edits::parse_value(v).unwrap();
    session::save_part_edits(&shared, Some(PLATE), None, &parse(plate_list()), "mcp").unwrap();
    let after_plate = std::fs::read(&step).unwrap();
    only_part_changed(&assembly(), &after_plate, &Default::default());
    session::save_part_edits(&shared, Some(PIN), None, &parse(pin_list()), "mcp").unwrap();
    let after_pin = std::fs::read(&step).unwrap();
    only_part_changed(&after_plate, &after_pin, &Default::default());
    assert_eq!(numbers(&part_of(&after_pin, PIN)).len(), 4);
    assert_eq!(numbers(&part_of(&after_pin, PLATE)).len(), 7);
    // A file of several parts needs its part.
    let err = session::save_edits(
        &shared,
        None,
        &parse(json!({"none": ["part.coating"]})),
        "mcp",
    )
    .unwrap_err();
    assert_eq!(std::fs::read(&step).unwrap(), after_pin, "{err}");
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

// ----- removal by id (an unnumbered row or item) and a change keeping what it does not touch -----

fn spindle() -> LoadedPart {
    let gz = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("ui/test/fixtures/19-spindle-semantic-links-only.step.gz"),
    )
    .unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&gz[..]), &mut bytes).unwrap();
    load_bytes("spindle.step", bytes).unwrap()
}

/// The file PMI items as (id, text).
fn file_pmi(table: &Value) -> Vec<(Value, String)> {
    table["file_pmi"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["id"].clone(), r["text"].as_str().unwrap().to_string()))
        .collect()
}

/// The `existing` items of kind `kind` and type `ty` on exactly `faces`.
fn existing_on(loaded: &LoadedPart, kind: &str, ty: &str, faces: Value) -> Vec<Value> {
    api::analyse(loaded).unwrap()["existing"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == kind && e["type"] == ty && e["faces"] == faces)
        .cloned()
        .collect()
}

#[test]
fn an_unnumbered_item_of_another_tools_file_is_removed_by_its_id() {
    let ctc = fixture("nist/nist_ctc_01_asme1_ap242.stp.gz");
    let before = file_pmi(&table(&ctc));
    // Every item NIST's file states off the table has an id from its STEP entity, and none
    // has a number to be removed by.
    assert!(
        before
            .iter()
            .all(|(id, _)| id.as_str().is_some_and(|id| id.starts_with("file_pmi:#")))
    );
    let target = before
        .iter()
        .find(|(_, text)| text == "⌓ 0.5 A")
        .expect("CTC-01 states ⌓ 0.5 A off the table")
        .0
        .clone();
    let saved = applied(&ctc, json!({"items": {"remove": [{"id": target}]}}));
    assert_eq!(
        saved.report.written,
        [format!("removed {}", target.as_str().unwrap())]
    );
    assert!(saved.retired.is_empty());
    let after = reload(&saved.bytes);
    let kept: Vec<(Value, String)> = before
        .iter()
        .filter(|(id, _)| *id != target)
        .cloned()
        .collect();
    // The rest keep their ids (and so their entities) and text.
    assert_eq!(file_pmi(&table(&after)), kept);
    assert_eq!(rewritten(&ctc.bytes, &saved.bytes), []);
    let entity: u64 = target.as_str().unwrap()["file_pmi:#".len()..]
        .parse()
        .unwrap();
    assert!(!records(&saved.bytes).contains_key(&entity));
    // An id the file does not have is refused, and nothing is written.
    let failures = refused(
        &ctc,
        json!({"items": {"remove": [{"id": "file_pmi:#999999"}]}}),
    );
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(
        failures[0].entry,
        r#"items.remove[{"id":"file_pmi:#999999"}]"#
    );
}

#[test]
fn a_stated_row_without_a_number_is_removed_by_its_id() {
    let plate = fixture("resume-p9/edited-plate.step.gz");
    assert_eq!(row(&table(&plate), "part.material")["state"], "stated");
    let saved = applied(
        &plate,
        json!({"items": {"remove": [{"id": "part.material"}]}}),
    );
    let after = reload(&saved.bytes);
    assert_eq!(row(&table(&after), "part.material")["state"], "gap");
    // Every numbered requirement and every other record is as it was.
    untouched_but(&plate, &after, &[]);
    assert_eq!(rewritten(&plate.bytes, &saved.bytes), []);
    // Removed and decided none in one list: decided none.
    let decided = applied(
        &plate,
        json!({"items": {"remove": [{"id": "part.material"}]}, "none": ["part.material"]}),
    );
    assert_eq!(
        row(&table(&reload(&decided.bytes)), "part.material")["state"],
        "decided_none"
    );
    // A row with a number: removing it by id retires that number, as by number.
    let by_id = applied(
        &plate,
        json!({"items": {"remove": [{"id": "datum.B.control"}]}}),
    );
    assert_eq!(by_id.retired, [18]);
    // A gap has nothing to remove; a removal beside a change of the same row contradicts it.
    let gap = refused(
        &after,
        json!({"items": {"remove": [{"id": "part.material"}]}}),
    );
    assert!(gap[0].reason.contains("nothing to remove"), "{gap:?}");
    let both = refused(
        &plate,
        json!({"items": {"remove": [{"id": "part.material"}]},
               "change": {"part.material": {"to": "steel", "expect": "6082-T6"}}}),
    );
    assert!(
        both.iter().any(|f| f.reason.contains("both removes")),
        "{both:?}"
    );
}

#[test]
fn a_tolerance_change_keeps_its_zone_and_modifiers() {
    let part = spindle();
    let saved = applied(
        &part,
        json!({"change": {"hole.position:holes:14": {"to": "0.2", "expect": "0.3"}}}),
    );
    let after = reload(&saved.bytes);
    let position = existing_on(&after, "geometric_tolerance", "Position", json!([14]));
    assert_eq!(position.len(), 1, "{position:?}");
    let p = &position[0];
    assert_eq!(p["text"], "⌖ ⌀0.2Ⓜ A", "{p}");
    assert_eq!(
        (&p["diametral"], &p["mmc"]),
        (&json!(true), &json!(true)),
        "{p}"
    );
    assert_eq!(
        p["modifiers"],
        json!(["maximum_material_requirement"]),
        "{p}"
    );
    assert_eq!(p["datums"], json!(["A"]), "{p}");
    // A change of limits keeps the number, and nothing else changes.
    assert_eq!(p["number"], 13, "{p}");
    untouched_but(&part, &after, &[13]);
    // Field by field, the same.
    let fields = applied(
        &part,
        json!({"change": {"hole.position:holes:14": {"to": {"value": 0.2}, "expect": "0.3"}}}),
    );
    let p = &existing_on(
        &reload(&fields.bytes),
        "geometric_tolerance",
        "Position",
        json!([14]),
    )[0];
    assert_eq!(p["text"], "⌖ ⌀0.2Ⓜ A", "{p}");
}

#[test]
fn a_nominal_change_keeps_its_fit_class() {
    let part = spindle();
    let saved = applied(
        &part,
        json!({"change": {"hole.function:holes:14": {"to": {"value": 3.4}, "expect": "⌀3.3 H11"}}}),
    );
    let after = reload(&saved.bytes);
    let size = existing_on(&after, "dimension", "Size_Diameter", json!([14]));
    assert_eq!(size.len(), 1, "{size:?}");
    let d = &size[0];
    assert_eq!(d["text"], "⌀3.4 H11", "{d}");
    assert_eq!(d["fit"], "H11", "{d}");
    assert_eq!(
        (&d["value"], &d["upper"], &d["lower"]),
        (&json!(3.4), &json!(0.075), &json!(0.0)),
        "{d}"
    );
    assert_eq!(d["number"], 3, "{d}");
    untouched_but(&part, &after, &[3]);
    // Only `value` changes field by field.
    let failures = refused(
        &part,
        json!({"change": {"hole.function:holes:14": {"to": {"fit": "H7"}, "expect": "⌀3.3 H11"}}}),
    );
    assert!(
        failures[0].reason.contains("field by field"),
        "{failures:?}"
    );
}

#[test]
fn a_text_only_requirement_is_refused_a_change_rather_than_losing_parts() {
    use haecceity::pmi::{AuxiliaryClassification, ToleranceKind};
    let plate = fixture("resume-p9/edited-plate.step.gz");
    // B's perpendicularity stated "all over": its text holds that, no field does.
    let bytes = another_tool(&plate.bytes, |p| {
        for t in &mut p.tolerances {
            if t.kind == ToleranceKind::Perpendicularity {
                t.auxiliary.push(AuxiliaryClassification::AllOver);
            }
        }
    });
    let part = reload(&bytes);
    let control = row(&table(&part), "datum.B.control");
    assert_eq!(control["requirements"][0]["text_only"], true, "{control}");
    let expect = edits::stated_value(&serde_json::from_value(control.clone()).unwrap());
    for to in [json!("0.05"), json!({"value": 0.05})] {
        let failures = refused(
            &part,
            json!({"change": {"datum.B.control": {"to": to, "expect": expect}}}),
        );
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(
            failures[0].reason.contains("only its text holds"),
            "{failures:?}"
        );
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

/// The assembly with PMI and a property of its own, on its root (`#5`, shape `#4`): a datum,
/// a flatness on a shape aspect, and a property with its representation. Returned with the
/// ids of those records.
fn with_assembly_items() -> (Vec<u8>, Vec<u64>) {
    let text = String::from_utf8(assembly()).unwrap();
    let records = [
        "#5001=SHAPE_ASPECT('assembly face','',#4,.F.);",
        "#5002=DATUM('assembly datum','',#4,.F.,'A');",
        "#5003=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.05),#28);",
        "#5004=FLATNESS_TOLERANCE('assembly flatness','',#5003,#5001);",
        "#5005=PROPERTY_DEFINITION('assembly note','',#5);",
        "#5006=DESCRIPTIVE_REPRESENTATION_ITEM('torque','5 Nm');",
        "#5007=REPRESENTATION('assembly note',(#5006),#27);",
        "#5008=PROPERTY_DEFINITION_REPRESENTATION(#5005,#5007);",
    ];
    let end = text.rfind("ENDSEC;").unwrap();
    let bytes = format!("{}{}\n{}", &text[..end], records.join("\n"), &text[end..]).into_bytes();
    (bytes, (5001..=5008).collect())
}

#[test]
fn assembly_level_items_keep_their_bytes_through_part_edits() {
    let (bare, ids) = with_assembly_items();
    let kept = |bytes: &[u8]| {
        let (old, new) = (records(&bare), records(bytes));
        for id in &ids {
            assert_eq!(new.get(id), old.get(id), "assembly-level #{id}");
        }
    };
    // A fill on the plate, a none on the pin, then a change and a removal on the plate.
    let filled = applied(&part_of(&bare, PLATE), plate_list());
    kept(&filled.bytes);
    let none = applied(
        &part_of(&filled.bytes, PIN),
        json!({"none": ["part.coating"]}),
    );
    kept(&none.bytes);
    assert_eq!(
        row(&table(&part_of(&none.bytes, PIN)), "part.coating")["state"],
        "decided_none"
    );
    let changed = applied(
        &part_of(&none.bytes, PLATE),
        json!({"change": {"hole.position:holes:11": {"to": "0.1", "expect": "0.25"}}}),
    );
    kept(&changed.bytes);
    let thread = numbers(&part_of(&changed.bytes, PLATE))
        .into_values()
        .find(|c| c.r#type == "Internal")
        .unwrap()
        .number;
    let remove = json!({"items": {"remove": [format!("#{thread}")]}});
    let removed = applied(&part_of(&changed.bytes, PLATE), remove.clone());
    assert_eq!(removed.retired, [thread]);
    kept(&removed.bytes);

    // A record the assembly's items reach that the removal would rewrite (the plate's
    // callouts model, made to list the assembly's datum too): the removal is refused, and
    // nothing is written.
    let text = String::from_utf8(changed.bytes.clone()).unwrap();
    let model = text
        .lines()
        .position(|l| l.contains("DRAUGHTING_MODEL('PMI callouts"))
        .unwrap();
    let lines: Vec<String> = text
        .lines()
        .enumerate()
        .map(|(i, l)| {
            if i == model {
                l.replacen("',(", "',(#5002,", 1)
            } else {
                l.to_string()
            }
        })
        .collect();
    let mut tied = lines.join("\n");
    tied.push('\n');
    let failures = refused(&part_of(tied.as_bytes(), PLATE), remove);
    assert!(
        failures[0].reason.contains("items the list does not name"),
        "{failures:?}"
    );
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

/// An added requirement's state comes from the file: stated once written. A fill of the same
/// spec again changes nothing (the bytes are the file's own); a fill with another value is
/// refused, all or nothing, naming the requirement's number; removing it by number in the same
/// list and filling the new value replaces it.
#[test]
fn an_added_requirement_the_file_states_is_a_no_op_or_refused() {
    let size = |value: &str| json!({"kind": "size", "faces": [9], "value": value});
    let part = fixture("parts/plate.step");
    let first = applied(&part, json!({"fill": {"extra.1": size("7.8/8.2")}}));
    let written = reload(&first.bytes);
    let again = applied(&written, json!({"fill": {"extra.1": size("7.8/8.2")}}));
    assert!(again.unchanged, "{:?}", again.report.written);
    assert_eq!(
        again.bytes, first.bytes,
        "a same-value fill is a byte-identical no-op"
    );
    let differing = refused(
        &written,
        json!({"fill": {"part.material": "steel", "extra.1": size("7.9/8.1")}}),
    );
    assert_eq!(differing.len(), 1, "{differing:?}");
    assert_eq!(differing[0].entry, r#"fill["extra.1"]"#);
    assert!(differing[0].reason.contains("#1"), "{differing:?}");
    let number = *numbers(&written).keys().next().unwrap();
    let replaced = applied(
        &written,
        json!({"items": {"remove": [format!("#{number}")]}, "fill": {"extra.1": size("7.9/8.1")}}),
    );
    let limits: Vec<_> = specify_core::existing::read_existing(&reload(&replaced.bytes))
        .unwrap()
        .into_iter()
        .filter_map(|e| e.limits)
        .collect();
    assert_eq!(limits, [[7.9, 8.1]]);
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
    let fill = json!({"fill": {"part.coating": "Anodised"}});
    let result = apply(&after, &edits::parse_value(fill).unwrap());
    assert!(result.is_err(), "fill overwrote a stored decided none");
    let failures = result.unwrap_err();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].entry, r#"fill["part.coating"]"#);
    let reason = r#"part.coating is decided none; to give it a value, clear it first: "clear": ["part.coating"] with the fill in the same list"#;
    assert_eq!(failures[0].reason, reason);
    let failures = refused(
        &after,
        json!({"change": {"part.coating": {"to": "Anodised", "expect": "none"}}}),
    );
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].entry, r#"change["part.coating"]"#);
    assert_eq!(failures[0].reason, reason);

    let same = json!({"fill": {"part.coating": " None (as machined) "}});
    let check = specify_core::writer::apply::check(&after, &same.to_string());
    assert!(check.applicable, "{}", check.message());
    assert_eq!(check.unchanged, [r#"fill["part.coating"]"#]);
    let unchanged = applied(&after, same);
    assert!(unchanged.unchanged);
    assert_eq!(unchanged.bytes, decided.bytes);

    let replaced = applied(
        &after,
        json!({"clear": ["part.coating"], "fill": {"part.coating": "Anodised"}}),
    );
    let replacement = row(&table(&reload(&replaced.bytes)), "part.coating");
    assert_eq!(replacement["state"], "stated");
    assert_eq!(replacement["value"], "Anodised");
    assert!(
        stored_answers(&replaced.bytes)
            .get("part.coating")
            .is_none()
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
    // An answers file saved on OLD with its face evidence (as 0.2 exported one; the export is
    // removed, ADR P11, but such files are still read).
    let mut saved = given.as_object().unwrap().clone();
    specify_core::answer_file::attach(&old, &mut saved, &Default::default(), []).unwrap();
    let text = Value::Object(saved).to_string();
    let checked = specify_core::writer::apply::check(&new, &text);
    let stale = |f: &Failure| f.entry == "_specify.evidence" && f.reason.contains("stale answers");
    assert!(checked.failures.iter().any(stale), "{checked:?}");
    let list = edits::read(&text).unwrap();
    let refused = apply(&new, &list).expect_err("refused");
    assert!(refused.iter().any(stale), "{refused:?}");
    assert!(apply(&old, &list).is_ok(), "on its own revision it applies");
}

#[test]
fn filling_a_grm05_length_only_adds_that_dimension() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let part = load(&corpus.join("gramel/GRM-05_depth_lock_bolt.step"), None).unwrap();
    let id = "length:planes:2-3";
    let output = applied(&part, json!({"fill": {id: "14 ±0.2 from B"}}));
    let after = reload(&output.bytes);
    let analysis = api::analyse(&after).unwrap();
    let existing = analysis["existing"].as_array().unwrap();
    assert_eq!(existing.len(), 1, "{existing:?}");
    assert_eq!(existing[0]["value"], 14.0);
    assert_eq!(existing[0]["upper"], 0.2);
    assert_eq!(row(&analysis["table"], id)["state"], "stated");
    let new = records(&output.bytes);
    for (entity, original) in records(&part.bytes) {
        assert_eq!(
            new.get(&entity),
            Some(&original),
            "original entity #{entity}"
        );
    }
}

#[test]
fn general_length_suggestions_can_be_accepted_declined_and_removed() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    let part = load(&corpus.join("gramel/GRM-05_depth_lock_bolt.step"), None).unwrap();
    let declined = "length:planes:2-3";
    let accepted = ["length:planes:2-5", "length:planes:2-8"];
    let offered: Vec<Value> = table(&part)["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["state"] == "gap" && !r["suggestion"].is_null())
        .map(|r| r["id"].clone())
        .collect();
    let bulk = applied(&part, json!({"accept_suggestions": offered}));
    let filled = table(&reload(&bulk.bytes));
    for id in [declined, accepted[0], accepted[1]] {
        assert_eq!(row(&filled, id)["state"], "stated");
    }
    let datum_size = "diameter.fit:turned_steps:0";
    assert_eq!(row(&filled, datum_size)["state"], "decided_none");
    assert!(
        bulk.report
            .not_written
            .iter()
            .any(|line| line.starts_with(&format!("answer {datum_size} ")))
    );
    let decision = applied(&part, json!({"none": [declined]}));
    let decided = reload(&decision.bytes);
    assert_eq!(row(&table(&decided), declined)["state"], "decided_none");
    let output = applied(&decided, json!({"accept_suggestions": accepted}));
    let after = reload(&output.bytes);
    let state = table(&after);
    assert_eq!(row(&state, declined)["state"], "decided_none");
    for id in accepted {
        assert_eq!(row(&state, id)["state"], "stated");
    }
    let output = applied(&after, json!({"items": {"remove": [{"id": accepted[0]}]}}));
    let state = table(&reload(&output.bytes));
    assert_eq!(row(&state, accepted[0])["state"], "gap");
    assert_eq!(row(&state, accepted[1])["state"], "stated");
}

#[test]
fn filling_an_unowned_hole_location_preserves_imported_pmi() {
    let part = fixture("spindle.step");
    let id = "location:holes:10-7";
    let before = api::analyse(&part).unwrap();
    let suggestion = row(&before["table"], id)["suggestion"].clone();
    assert!(
        suggestion.is_string(),
        "expected a location suggestion for {id}"
    );
    let output = applied(&part, json!({"fill": {id: suggestion}}));
    let after = reload(&output.bytes);
    assert_eq!(row(&table(&after), id)["state"], "stated");
    let old = records(&part.bytes);
    let new = records(&output.bytes);
    let imported: Vec<_> = old
        .iter()
        .filter(|(_, text)| text.contains("POSITION_TOLERANCE"))
        .collect();
    assert!(!imported.is_empty());
    for (entity, original) in imported {
        assert_eq!(new.get(entity), Some(original), "imported PMI #{entity}");
    }
}
