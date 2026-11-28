//! ADR P11 through the engine: another tool re-authored Specify's hole position 0.5 as its own
//! 1.5. Reopening shows 1.5 (no conflict); a save (`edit` of the answers it opened with) keeps
//! it; answering 0.5 afresh and saving is refused, all or nothing, saying the file states 1.5
//! (changing it is an explicit `change`), and the file keeps its 1.5 alone. (The fixture is a
//! file Specify 0.2 wrote, made by the library's writer.)
use serde_json::{Map, Value, json};
use specify_pmi::{api, existing::read_existing, part::load_bytes};
use specify_pmi_wasm::session::Session;

const HOLE: &str = "hole.position:holes:12";
const PLATE: &[u8] = include_bytes!("../../../tests/fixtures/parts/plate.step");

fn re_authored() -> Vec<u8> {
    let given = json!({
        "part.material": "6082-T6",
        "part.locating": "position tolerances",
        "datum.A": [2], "datum.B": [1], "datum.C": [0],
        "datum.A.flatness": "0.03",
        "datum.B.control": "0.1",
        (HOLE): "0.5",
    })
    .as_object()
    .unwrap()
    .clone();
    let loaded = load_bytes("plate.step", PLATE.to_vec()).unwrap();
    let analysis = api::analyse(&loaded).unwrap();
    let intent = api::apply(&analysis, &given, true).unwrap();
    let bytes = specify_pmi::writer::write_bytes(&loaded, &intent, Some(&given))
        .unwrap()
        .0;
    let text = String::from_utf8(bytes).unwrap();
    let line = text
        .lines()
        .find(|l| l.contains("POSITION_TOLERANCE()") && !l.contains("WITH_MODIFIERS"))
        .unwrap()
        .to_string();
    let measure = line
        .split("GEOMETRIC_TOLERANCE(")
        .nth(1)
        .unwrap()
        .split(',')
        .nth(2)
        .unwrap()
        .trim()
        .to_string();
    let value = text
        .lines()
        .find(|l| l.starts_with(&format!("{measure}=")))
        .unwrap()
        .to_string();
    assert!(line.contains("'specify-core:written:v1'") && value.contains("(0.5)"));
    text.replace(&line, &line.replace("'specify-core:written:v1'", "''"))
        .replace(&value, &value.replace("(0.5)", "(1.5)"))
        .into_bytes()
}

fn positions(bytes: &[u8]) -> Vec<(f64, bool)> {
    read_existing(&load_bytes("plate.step", bytes.to_vec()).unwrap())
        .unwrap()
        .into_iter()
        .filter(|e| e.r#type == "Position" && e.faces == [12])
        .map(|e| (e.value.unwrap(), e.provenance.is_some()))
        .collect()
}

/// The editor's answers on opening: the resumed answers.
fn opened(session: &Session) -> Map<String, Value> {
    let resume = session.snapshot()["analysis"]["resume"].clone();
    resume["answers"].as_object().unwrap().clone()
}

/// A save: the engine's `edit` of these answers (a 0.2 answers map, read as fills), the bytes
/// written, or what refused it.
fn save(session: &Session, answers: &Map<String, Value>) -> Result<Vec<u8>, Value> {
    let written = session.edit(&Value::Object(answers.clone()).to_string());
    if written["written"] != json!(true) {
        return Err(written);
    }
    Ok(serde_json::from_value(written["bytes"].clone()).unwrap())
}

#[test]
fn the_editor_does_not_replace_another_tools_position_with_a_fresh_answer() {
    let edited = re_authored();
    assert_eq!(positions(&edited), [(1.5, false)]);
    let session = Session::open("plate.step", &edited).unwrap();
    let analysis = session.snapshot()["analysis"].clone();
    assert!(
        analysis["resume"].get("conflicts").is_none(),
        "{}",
        analysis["resume"]
    );
    let saved = save(&session, &opened(&session)).unwrap();
    assert_eq!(positions(&saved), [(1.5, false)]);
    let session = Session::open("plate.step", &saved).unwrap();
    let resume = session.snapshot()["analysis"]["resume"].clone();
    assert!(resume.get("conflicts").is_none(), "{resume}");
    assert!(resume["answers"].get(HOLE).is_none(), "{resume}");
    let mut answers = opened(&session);
    answers.insert(HOLE.into(), json!("0.5"));
    // Was: written with 0.5 not written ("the file already states this requirement"); an edit
    // list refuses the differing fill instead, naming it, and writes nothing.
    let refused = save(&session, &answers).unwrap_err();
    assert!(
        refused["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["entry"].as_str().is_some_and(|e| e.contains(HOLE))
                && f["reason"]
                    .as_str()
                    .is_some_and(|r| r.contains("already states"))),
        "{refused}"
    );
    assert_eq!(positions(&saved), [(1.5, false)]);
    // Reopened once more: still the other tool's 1.5, and no answer stored for it.
    let again = Session::open("plate.step", &saved).unwrap();
    let resume = again.snapshot()["analysis"]["resume"].clone();
    assert!(resume["answers"].get(HOLE).is_none(), "{resume}");
}
