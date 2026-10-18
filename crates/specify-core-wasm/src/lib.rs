use serde_json::{Map, Value, json};
use specify_core::{api, choices};
pub mod session;

pub fn interpret(
    analysis: &Value,
    answers: &Map<String, Value>,
    question_id: &str,
    text: &str,
) -> Result<Value, String> {
    let questions = api::questions(analysis, answers).map_err(|error| error.to_string())?;
    let question = questions
        .iter()
        .find(|question| question.id == question_id && question.kind == "choice")
        .ok_or_else(|| format!("No choice question {question_id}"))?;
    let value = choices::interpret(
        &question.id,
        text,
        &question.options,
        question.diameter,
        question.depth,
    )
    .map_err(|error| error.0)?;
    let shown = if question.id.starts_with("hole.function:") {
        choices::with_default_full_thread(&value, question.depth)
    } else {
        value.clone()
    };
    let mut result = json!({"value": value, "code": choices::code(&shown)});
    if let Some(diameter) = question.diameter
        && (question.id.starts_with("hole.function:") || question.id.starts_with("diameter.fit:"))
    {
        result["callout"] = json!(choices::callout(
            &shown,
            diameter,
            question.id.starts_with("hole."),
        )?);
    }
    Ok(result)
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use specify_core::tolerancing;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub struct IntentEngine(session::Session);

    #[wasm_bindgen]
    impl IntentEngine {
        #[wasm_bindgen(constructor)]
        pub fn open(name: &str, bytes: &[u8]) -> Result<IntentEngine, JsValue> {
            session::Session::open(name, bytes)
                .map(IntentEngine)
                .map_err(|error| JsValue::from_str(&error))
        }
        pub fn snapshot(&self) -> String {
            self.0.snapshot().to_string()
        }

        #[wasm_bindgen(js_name = writePmi)]
        pub fn write_pmi(&self, answers: &str, project: Option<String>) -> Result<String, JsValue> {
            let answers = serde_json::from_str(answers)
                .map_err(|error| JsValue::from_str(&format!("Invalid answers: {error}")))?;
            self.0
                .write_pmi(&answers, project.as_deref())
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }
        /// The review settled to the default path, and the settled `{answers, auto, gdt}`.
        pub fn settle(&self, state: &str) -> Result<String, JsValue> {
            let state = serde_json::from_str(state)
                .map_err(|error| JsValue::from_str(&format!("Invalid state: {error}")))?;
            self.0
                .settle(&state)
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }
        pub fn review(&self, answers: &str) -> Result<String, JsValue> {
            let answers = serde_json::from_str(answers)
                .map_err(|error| JsValue::from_str(&format!("Invalid answers: {error}")))?;
            self.0
                .review(&answers)
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }
    }

    fn parse(analysis: &str, answers: &str) -> Result<(Value, Map<String, Value>), JsValue> {
        let analysis = serde_json::from_str(analysis)
            .map_err(|error| JsValue::from_str(&format!("Invalid analysis: {error}")))?;
        let answers = serde_json::from_str(answers)
            .map_err(|error| JsValue::from_str(&format!("Invalid answers: {error}")))?;
        Ok((analysis, answers))
    }

    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
    }

    #[wasm_bindgen(js_name = review)]
    pub fn review_json(analysis: &str, answers: &str) -> Result<String, JsValue> {
        let (analysis, answers) = parse(analysis, answers)?;
        api::review(&analysis, &answers)
            .map(|value| value.to_string())
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    fn state(json: &str) -> Result<tolerancing::State, JsValue> {
        serde_json::from_str(json)
            .map_err(|error| JsValue::from_str(&format!("Invalid state: {error}")))
    }
    fn answers(json: &str) -> Result<Map<String, Value>, JsValue> {
        serde_json::from_str(json)
            .map_err(|error| JsValue::from_str(&format!("Invalid answers: {error}")))
    }
    fn out(state: tolerancing::State) -> String {
        serde_json::to_string(&state).unwrap()
    }

    /// The person's answers replace the state's (`tolerancing::answer_edit`).
    #[wasm_bindgen(js_name = answerEdit)]
    pub fn answer_edit(state: &str, answers: &str) -> Result<String, JsValue> {
        Ok(out(tolerancing::answer_edit(
            &self::state(state)?,
            &self::answers(answers)?,
        )))
    }

    /// A change of tolerancing mode (`plusminus`, `geometric`, or none for the default path).
    #[wasm_bindgen(js_name = tolerancingEdit)]
    pub fn tolerancing_edit(
        state: &str,
        mode: Option<String>,
        answers: &str,
    ) -> Result<String, JsValue> {
        let mode = mode
            .map(|mode| serde_json::from_value(Value::String(mode)))
            .transpose()
            .map_err(|error| JsValue::from_str(&format!("Invalid mode: {error}")))?;
        Ok(out(tolerancing::tolerancing_edit(
            &self::state(state)?,
            mode,
            &self::answers(answers)?,
        )))
    }

    /// The state a project opens with, given the answers a STEP Specify wrote resumes, if any.
    #[wasm_bindgen(js_name = openedTolerancing)]
    pub fn opened_tolerancing(state: &str, resume: Option<String>) -> Result<String, JsValue> {
        let resume = resume.as_deref().map(self::answers).transpose()?;
        Ok(out(tolerancing::opened(
            &self::state(state)?,
            resume.as_ref(),
        )))
    }

    #[wasm_bindgen(js_name = interpret)]
    pub fn interpret_json(
        analysis: &str,
        answers: &str,
        question_id: &str,
        text: &str,
    ) -> Result<String, JsValue> {
        let (analysis, answers) = parse(analysis, answers)?;
        interpret(&analysis, &answers, question_id, text)
            .map(|value| value.to_string())
            .map_err(|error| JsValue::from_str(&error))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn blind_thread_interpretation_summary_matches_the_intent() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/spindle.step");
        let analysis = api::analyse_file(&path, None).unwrap();
        let questions = api::questions(&analysis, &Map::new()).unwrap();
        let hole = questions
            .iter()
            .find(|q| {
                q.id.starts_with("hole.function:")
                    && q.default == Some(json!("tapped:M5"))
                    && q.depth.is_some()
            })
            .unwrap();
        let result = interpret(&analysis, &Map::new(), &hole.id, "M5").unwrap();
        assert_eq!(result["value"], "tapped:M5");
        let intent = api::preview(&analysis, &Map::new()).unwrap();
        let thread = intent
            .requirements
            .iter()
            .find(|r| r["kind"] == "thread")
            .unwrap();
        let full = thread["full_thread"].as_f64().unwrap();
        assert_eq!(result["code"], format!("M5 tapped, {full} mm full thread"));
        assert!(
            result["callout"]
                .as_str()
                .unwrap()
                .contains(&format!("{full} mm min full thread"))
        );
    }
}
