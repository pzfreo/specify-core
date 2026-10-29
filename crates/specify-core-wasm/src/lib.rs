#![cfg_attr(not(test), warn(clippy::unwrap_used, clippy::expect_used))]

use serde_json::{Map, Value, json};
use specify_core::{api, choices};
mod input;
pub mod session;

/// Accept every unanswered default to the same fixed point as CLI/serve apply.
pub fn accept_defaults(
    analysis: &Value,
    answers: &Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    input::answers(analysis, answers).map_err(|e| e.to_string())?;
    specify_core::rules::with_all_defaults(analysis, answers).map_err(|error| error.to_string())
}

pub fn interpret(
    analysis: &Value,
    answers: &Map<String, Value>,
    question_id: &str,
    text: &str,
) -> Result<Value, String> {
    input::answers(analysis, answers).map_err(|e| e.to_string())?;
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
    // result is an object constructed here, so adding its callout key cannot panic.
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
        /// Part `part` (an index from `IntentEngine.parts`) of any file, an assembly included.
        #[wasm_bindgen(js_name = openPart)]
        pub fn open_part(name: &str, bytes: &[u8], part: usize) -> Result<IntentEngine, JsValue> {
            session::Session::open_part(name, bytes, part)
                .map(IntentEngine)
                .map_err(|error| JsValue::from_str(&error))
        }

        /// The file's distinct parts, as `specify-core parts` lists them: `[{part, name,
        /// product_id, faces, instances, placements, written}]`.
        pub fn parts(name: &str, bytes: &[u8]) -> Result<String, JsValue> {
            session::Session::parts(name, bytes)
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }
        pub fn snapshot(&self) -> String {
            self.0.snapshot().to_string()
        }

        /// `project` is the editor's base64 project; `payload` is `{name, value}` JSON. At most one.
        /// Rewriting replaces what a previous write carried: without either, the written file
        /// carries no project, payload or original record, even if the opened one did.
        #[wasm_bindgen(js_name = writePmi)]
        pub fn write_pmi(
            &self,
            answers: &str,
            project: Option<String>,
            payload: Option<String>,
        ) -> Result<String, JsValue> {
            let answers = self::answers(answers)?;
            let payload = payload
                .as_deref()
                .map(|text| json("Payload", text))
                .transpose()?;
            self.0
                .write_pmi(&answers, project.as_deref(), payload.as_ref())
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }

        /// `{schema, face_count, faces}`: keep it with answers saved outside the STEP.
        pub fn evidence(&self) -> Result<String, JsValue> {
            self.0
                .evidence()
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }

        /// Saved answers matched to this part: `{answers, unresolved?, migrated?, unverified?,
        /// automatic?}`. `evidence` is what `evidence()` gave when they were saved; `automatic`
        /// a JSON list of the IDs the default path answered. Without evidence answers are
        /// matched by feature ID; with evidence that does not match every answer on a feature
        /// or a datum is in `unresolved`, with its value.
        #[wasm_bindgen(js_name = rematchAnswers)]
        pub fn rematch_answers(
            &self,
            answers: &str,
            evidence: Option<String>,
            automatic: Option<String>,
        ) -> Result<String, JsValue> {
            let answers = self::answers(answers)?;
            let evidence = evidence
                .as_deref()
                .map(|text| json("Evidence", text))
                .transpose()?;
            let automatic: Option<Vec<String>> = automatic
                .as_deref()
                .map(|text| {
                    serde_json::from_value(json("Automatic", text)?)
                        .map_err(|error| JsValue::from_str(&format!("Invalid automatic: {error}")))
                })
                .transpose()?;
            self.0
                .rematch_answers(&answers, evidence.as_ref(), automatic.as_deref())
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }

        /// The JSON payload `name` the opened STEP carries, if any.
        pub fn payload(&self, name: &str) -> Result<Option<String>, JsValue> {
            limit("Payload name", name, input::TEXT_LIMIT)?;
            Ok(self.0.payload(name).map(|value| value.to_string()))
        }

        /// The part the opened STEP was first written from, if it records it.
        pub fn original(&self) -> Option<Vec<u8>> {
            self.0.original()
        }
        /// The review settled to the default path, and the settled `{answers, auto, gdt}`.
        pub fn settle(&self, state: &str) -> Result<String, JsValue> {
            let state = self::state(state)?;
            self.0
                .settle(&state)
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }
        pub fn review(&self, answers: &str) -> Result<String, JsValue> {
            let answers = self::answers(answers)?;
            self.0
                .review(&answers)
                .map(|value| value.to_string())
                .map_err(|error| JsValue::from_str(&error))
        }

        #[wasm_bindgen(js_name = acceptDefaults)]
        pub fn accept_defaults(&self, answers: &str) -> Result<String, JsValue> {
            self.0
                .accept_defaults(&self::answers(answers)?)
                .map(|answers| Value::Object(answers).to_string())
                .map_err(|error| JsValue::from_str(&error))
        }
    }

    fn parse(analysis: &str, answers: &str) -> Result<(Value, Map<String, Value>), JsValue> {
        limit("Analysis", analysis, input::JSON_LIMIT)?;
        let analysis = serde_json::from_str(analysis)
            .map_err(|error| JsValue::from_str(&format!("Invalid analysis: {error}")))?;
        let answers = self::answers(answers)?;
        Ok((analysis, answers))
    }

    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
    }

    /// Two revisions of a part compared (`session::diff`): `{report, markdown, view}` JSON; of
    /// an assembly, `{report, markdown, parts}`, each changed part with its own `markdown` and
    /// `view`. `answers` is a JSON object (by part name for an assembly), `{}` to carry the old
    /// file's stored answers.
    #[wasm_bindgen(js_name = diff)]
    pub fn diff_json(
        old_name: &str,
        old: &[u8],
        new_name: &str,
        new: &[u8],
        answers: &str,
        title: &str,
    ) -> Result<String, JsValue> {
        limit("Title", title, input::TEXT_LIMIT)?;
        let answers = self::answers(answers)?;
        session::diff((old_name, old), (new_name, new), &answers, title)
            .map(|value| value.to_string())
            .map_err(|error| JsValue::from_str(&error))
    }

    #[wasm_bindgen(js_name = review)]
    pub fn review_json(analysis: &str, answers: &str) -> Result<String, JsValue> {
        let (analysis, answers) = parse(analysis, answers)?;
        input::answers(&analysis, &answers).map_err(js_error)?;
        api::review(&analysis, &answers)
            .map(|value| value.to_string())
            .map_err(|error| JsValue::from_str(&error.to_string()))
    }

    #[wasm_bindgen(js_name = acceptDefaults)]
    pub fn accept_defaults_json(analysis: &str, answers: &str) -> Result<String, JsValue> {
        let (analysis, answers) = parse(analysis, answers)?;
        accept_defaults(&analysis, &answers)
            .map(|answers| Value::Object(answers).to_string())
            .map_err(|error| JsValue::from_str(&error))
    }

    fn js_error(error: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&error.to_string())
    }
    fn limit(field: &'static str, text: &str, limit: usize) -> Result<(), JsValue> {
        input::size(field, text.len(), limit).map_err(js_error)
    }
    fn state(json: &str) -> Result<tolerancing::State, JsValue> {
        limit("State", json, input::JSON_LIMIT)?;
        let state: tolerancing::State = serde_json::from_str(json)
            .map_err(|error| JsValue::from_str(&format!("Invalid state: {error}")))?;
        // Saved auto IDs may be stale or repeated; core transforms handle them safely.
        input::answers_shape(&state.answers).map_err(js_error)?;
        Ok(state)
    }
    fn json(field: &'static str, text: &str) -> Result<Value, JsValue> {
        limit(field, text, input::JSON_LIMIT)?;
        serde_json::from_str(text).map_err(|error| {
            JsValue::from_str(&format!("Invalid {}: {error}", field.to_lowercase()))
        })
    }
    fn answers(json: &str) -> Result<Map<String, Value>, JsValue> {
        limit("Answers", json, input::JSON_LIMIT)?;
        let answers = serde_json::from_str(json)
            .map_err(|error| JsValue::from_str(&format!("Invalid answers: {error}")))?;
        input::answers_shape(&answers).map_err(js_error)?;
        Ok(answers)
    }
    fn out(state: tolerancing::State) -> Result<String, JsValue> {
        serde_json::to_string(&state).map_err(js_error)
    }

    /// The person's answers replace the state's (`tolerancing::answer_edit`).
    #[wasm_bindgen(js_name = answerEdit)]
    pub fn answer_edit(state: &str, answers: &str) -> Result<String, JsValue> {
        out(tolerancing::answer_edit(
            &self::state(state)?,
            &self::answers(answers)?,
        ))
    }

    /// A change of tolerancing mode (`plusminus`, `geometric`, or none for the default path).
    #[wasm_bindgen(js_name = tolerancingEdit)]
    pub fn tolerancing_edit(
        state: &str,
        mode: Option<String>,
        answers: &str,
    ) -> Result<String, JsValue> {
        if let Some(text) = mode.as_deref() {
            limit("Mode", text, input::TEXT_LIMIT)?;
        }
        let mode = mode
            .map(|mode| serde_json::from_value(Value::String(mode)))
            .transpose()
            .map_err(|error| JsValue::from_str(&format!("Invalid mode: {error}")))?;
        out(tolerancing::tolerancing_edit(
            &self::state(state)?,
            mode,
            &self::answers(answers)?,
        ))
    }

    /// The state a project opens with, given the answers a STEP Specify wrote resumes, if any.
    #[wasm_bindgen(js_name = openedTolerancing)]
    pub fn opened_tolerancing(state: &str, resume: Option<String>) -> Result<String, JsValue> {
        let resume = resume.as_deref().map(self::answers).transpose()?;
        out(tolerancing::opened(&self::state(state)?, resume.as_ref()))
    }

    #[wasm_bindgen(js_name = interpret)]
    pub fn interpret_json(
        analysis: &str,
        answers: &str,
        question_id: &str,
        text: &str,
    ) -> Result<String, JsValue> {
        limit("Question ID", question_id, input::TEXT_LIMIT)?;
        limit("Interpretation", text, input::TEXT_LIMIT)?;
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
        // #20: the proposed tap is written only once its interpreted answer is supplied.
        let answers = Map::from_iter([(hole.id.clone(), result["value"].clone())]);
        let answers = accept_defaults(&analysis, &answers).unwrap();
        let intent = api::preview(&analysis, &answers).unwrap();
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
