//! Cases moved from the WASM crate's tests (Specify 0.3.0, piece 6): they exercised the engine's
//! whole-intent `writePmi`, which the browser API no longer has (it applies edit lists, `edit`).
//! The library retains whole-intent replay, which these native tests exercise. The CLI
//! writes edit lists.
use serde_json::{Map, Value, json};
use specify_pmi::{api, part, rules, writer};

/// The whole intent of `answers` (what is answered; the rows not asked listed), written and
/// verified with the answers stored, as the engine's `writePmi` wrote it.
fn write_intent(loaded: &part::LoadedPart, answers: &Map<String, Value>) -> (Vec<u8>, Value) {
    let analysis = api::analyse(loaded).unwrap();
    let mut intent = api::preview(&analysis, answers).unwrap();
    intent
        .requirements
        .extend(rules::unasked_rows(&analysis, answers).unwrap());
    let (bytes, report) = writer::write_bytes(loaded, &intent, Some(answers)).unwrap();
    (bytes, report.to_dict())
}

fn plate() -> part::LoadedPart {
    let bytes = include_bytes!("fixtures/parts/plate.step");
    part::load_bytes("plate.step", bytes.to_vec()).unwrap()
}

fn answers(extra: Value) -> Map<String, Value> {
    let mut answers = json!({
        "part.material": "6082-T6",
        "part.locating": "± dimensions",
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
    })
    .as_object()
    .unwrap()
    .clone();
    answers.extend(extra.as_object().unwrap().clone());
    answers
}

fn not_written(report: &Value) -> Vec<String> {
    serde_json::from_value(report["not_written"].clone()).unwrap()
}

/// Moved from crates/specify-pmi-wasm/tests/unasked_answers.rs (#126; it needs the
/// whole-intent write, which lists answers whose question is not asked): the write lists an
/// unasked answer and an unasked carry block, and reopening restores the block.
#[test]
fn the_whole_intent_write_lists_unasked_answers_and_keeps_blocks() {
    let loaded = plate();
    let (_, report) = write_intent(&loaded, &answers(json!({"datum.A.flatness": "0.05"})));
    assert!(
        not_written(&report).contains(
            &"datum.A.flatness is not asked when part.locating is '± dimensions', so its \
              answer was not written"
                .to_string()
        ),
        "{report}"
    );
    let block = json!({"_specify": {"unresolved":
        {"datum.A.flatness": "its datum datum.A is unresolved"}}});
    let (bytes, report) = write_intent(&loaded, &answers(block));
    assert!(
        not_written(&report)
            .iter()
            .any(|l| l.starts_with("datum.A.flatness was carried")),
        "{report}"
    );
    let reopened = api::analyse(&part::load_bytes("written.step", bytes).unwrap()).unwrap();
    assert_eq!(
        reopened["resume"]["answers"]["_specify"]["unresolved"]["datum.A.flatness"],
        "its datum datum.A is unresolved"
    );
}
