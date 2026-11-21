//! The whole-intent write these tests used to reach through the engine's `writePmi`, which the
//! browser API no longer has (Specify 0.3.0: the engine applies edit lists, `edit`). The tests
//! that check what the native writer makes of a set of answers (computed tap depths, stored
//! answers, characteristic numbers) call it here, natively, as `writePmi` did.
#![allow(dead_code)]

use serde_json::{Map, Value, json};
use specify_core::{api, rules};
use specify_core_wasm::session::Session;

/// The answered PMI written and verified, the answers stored: `{bytes, source_sha256,
/// report}`, the report with `complete` and `missing`, as `writePmi` gave it.
pub fn write_pmi(session: &Session, answers: &Map<String, Value>) -> Result<Value, String> {
    write_pmi_with_bom(session, answers, false)
}

/// [`write_pmi`], with Specify's parts-list suggestions when `bom`.
pub fn write_pmi_with_bom(
    session: &Session,
    answers: &Map<String, Value>,
    bom: bool,
) -> Result<Value, String> {
    write_pmi_carrying(session, answers, bom, None, None)
}

/// [`write_pmi_with_bom`], also storing the editor's base64 `project` or an application's
/// `{name, value}` `payload` (with the original part), as `writePmi` stored them.
pub fn write_pmi_carrying(
    session: &Session,
    answers: &Map<String, Value>,
    bom: bool,
    project: Option<&str>,
    payload: Option<&Value>,
) -> Result<Value, String> {
    let (loaded, analysis) = (session.loaded(), session.analysis());
    specify_core::answer_file::check(loaded, answers)?;
    specify_core::input::answers(analysis, answers).map_err(|e| e.to_string())?;
    let mut intent = api::preview(analysis, answers).map_err(|e| e.to_string())?;
    intent
        .requirements
        .extend(rules::unasked_rows(analysis, answers).map_err(|e| e.to_string())?);
    if bom {
        specify_core::bom::accept_suggestions(analysis, answers, &mut intent)?;
    }
    let source: &[u8] = &loaded.bytes;
    let extra = match (project, payload) {
        (Some(text), None) => {
            specify_core::carried::attributes_encoded(source, "draftwright editor project", text)
                .map_err(|_| "the project to store is not base64".to_string())?
        }
        (None, Some(p)) => specify_core::carried::attributes(
            source,
            p["name"].as_str().ok_or("payload name")?,
            &p["value"],
        )
        .map_err(|e| e.to_string())?,
        (None, None) => vec![],
        (Some(_), Some(_)) => return Err("give a project or a payload, not both".into()),
    };
    let (bytes, report) =
        specify_core::writer::write_bytes_with(loaded, &intent, Some(answers), &extra)
            .map_err(|e| e.to_string())?;
    let missing = match api::apply(analysis, answers, false) {
        Err(api::Error::Rules(rules::Error::Incomplete(e))) => e.missing,
        _ => Vec::new(),
    };
    let mut report = report.to_dict();
    report["complete"] = json!(missing.is_empty());
    report["missing"] = json!(missing);
    Ok(json!({"bytes": bytes, "source_sha256": loaded.binding.source_sha256, "report": report}))
}

/// Every unanswered default taken, to the same fixed point as `write --accept-defaults`.
pub fn accept_defaults(
    session: &Session,
    answers: &Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    specify_core_wasm::accept_defaults(session.analysis(), answers)
}
