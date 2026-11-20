//! What a plan of answers writes (ADR P11 replaced the reconciliation of stored answers
//! against the file's PMI that lived here: the file's PMI is the only copy, and the feature
//! table, [`crate::feature_table`], reads it).

use std::collections::BTreeSet;

use serde_json::Value;

/// The questions whose answers `plan`, a comparison scene of answers, states PMI for: a
/// requirement or part setting tagged with the question, or the material.
pub(crate) fn writing(plan: &Value) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = plan["specify"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, meta)| meta["status"] != "existing")
        .filter_map(|(_, meta)| meta["question"].as_str().map(str::to_owned))
        .collect();
    if !plan["pmi"]["pmi"]["material"].is_null() {
        out.insert("part.material".into());
    }
    out
}
