//! Whole-intent writes for tests. The library writes by edit list (`writer::apply`); these
//! helpers write an intent that a test built directly (`writer::write_parts`), as the removed
//! `api::write*` functions did, so the writer's own behaviour stays testable.

use std::path::Path;

use serde_json::{Map, Value};
use specify_pmi::api::{self, Error};
use specify_pmi::part::{self, LoadedPart};
use specify_pmi::rules::Intent;
use specify_pmi::writer::{self, WriteError, WriteReport};

pub type Result<T> = std::result::Result<T, Error>;

pub fn write(
    loaded: &LoadedPart,
    intent: &Intent,
    output: &Path,
    answers: Option<&Map<String, Value>>,
) -> Result<WriteReport> {
    Ok(writer::write(loaded, intent, output, answers)?)
}

pub fn write_file(
    step: &Path,
    intent: &Intent,
    output: &Path,
    answers: Option<&Map<String, Value>>,
) -> Result<WriteReport> {
    let loaded = part::load(step, Some(api::intent_part(intent)?))?;
    write(&loaded, intent, output, answers)
}

pub fn write_parts(
    step: &Path,
    intents: &[(Intent, Option<Map<String, Value>>)],
    output: &Path,
) -> Result<WriteReport> {
    let loaded = part::load_all(step)?;
    let mut jobs = Vec::with_capacity(intents.len());
    for (intent, answers) in intents {
        let k = api::intent_part(intent)?;
        let Some(part) = loaded.get(k) else {
            return Err(Error::Write(WriteError::Invalid(format!(
                "{} has no part {k}",
                step.display()
            ))));
        };
        jobs.push((part, intent, answers.as_ref()));
    }
    Ok(writer::write_parts(&jobs, output)?)
}

pub const IN_MEMORY: &str = "<in memory>";

pub fn write_bytes(
    bytes: Vec<u8>,
    jobs: &[(Intent, Option<Map<String, Value>>)],
) -> Result<(Vec<u8>, WriteReport)> {
    let file = part::File::of_bytes(Path::new(IN_MEMORY), bytes)?;
    let mut loaded = Vec::with_capacity(jobs.len());
    for (intent, _) in jobs {
        let k = api::intent_part(intent)?;
        if k >= file.definitions().len() {
            return Err(Error::Write(WriteError::Invalid(format!(
                "{IN_MEMORY} has no part {k}"
            ))));
        }
        loaded.push(file.load(Some(k))?);
    }
    let writes: Vec<writer::Job<'_>> = loaded
        .iter()
        .zip(jobs)
        .map(|(l, (intent, answers))| (l, intent, answers.as_ref()))
        .collect();
    Ok(writer::write_parts_bytes(&writes)?)
}
