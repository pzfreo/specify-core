//! The browser session exposes saved-answer recovery through its existing snapshot and review.
use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use serde_json::{Value, json};
use specify_core::{part, resume};
use specify_core_wasm::session::Session;
use std::path::Path;

fn saved(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/fixtures/resume-alpha6/{name}.step.gz"));
    part::load(&path, None).unwrap().bytes.to_vec()
}

#[test]
fn browser_sessions_resume_all_87_alpha6_answers() {
    let mut count = 0;
    for name in ["plate", "plate-moved", "shaft", "corners", "pin"] {
        let bytes = saved(name);
        let expected = &resume::stored(&bytes)[&0]["answers"];
        let session = Session::open(name, &bytes).unwrap();
        let snapshot = session.snapshot();
        assert_eq!(&snapshot["analysis"]["resume"]["answers"], expected);
        count += expected.as_object().unwrap().len();
        assert!(
            session.review(expected.as_object().unwrap()).unwrap()["open"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(count, 87);
}

#[test]
fn browser_reports_unmatched_values_and_leaves_current_question_open() {
    let bytes = saved("pin");
    let mut payload = json!(resume::stored(&bytes)[&0]);
    let answers = payload["answers"].as_object_mut().unwrap();
    let key = answers
        .keys()
        .find(|key| key.starts_with("hole.function:"))
        .unwrap()
        .clone();
    let value = answers.shift_remove(&key).unwrap();
    answers.insert("hole.function:holes:999".into(), value.clone());
    let doc = Document::parse(bytes).unwrap();
    let mut edit = Edit::new();
    for id in doc.ids() {
        if let Some(RawEntity::Simple {
            name, attributes, ..
        }) = doc.get(id)
            && name == "DESCRIPTIVE_REPRESENTATION_ITEM"
            && matches!(attributes.first(), Some(Attribute::String(s)) if s == resume::NAME)
        {
            let mut attributes = attributes.clone();
            attributes[1] = Attribute::String(resume::encode(&payload));
            edit.replace(id, haecceity::p21::simple(name, attributes));
        }
    }
    let session = Session::open("orphan.step", &doc.apply(&edit).unwrap().bytes).unwrap();
    let snapshot = session.snapshot();
    let resumed = &snapshot["analysis"]["resume"];
    assert_eq!(resumed["unresolved"][0]["answer"], value);
    assert_eq!(resumed["unresolved"][0]["id"], "hole.function:holes:999");
    let review: Value = session
        .review(resumed["answers"].as_object().unwrap())
        .unwrap();
    assert!(review["open"].as_array().unwrap().contains(&json!(key)));
    assert!(
        review["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["code"] == "saved_answer_unresolved")
    );
}
