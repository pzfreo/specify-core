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
    let mut result = json!({"value": value, "code": choices::code(&value)});
    if let Some(diameter) = question.diameter
        && (question.id.starts_with("hole.function:") || question.id.starts_with("diameter.fit:"))
    {
        result["callout"] = json!(choices::callout(
            &value,
            diameter,
            question.id.starts_with("hole."),
        )?);
    }
    Ok(result)
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
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
