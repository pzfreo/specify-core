//! Cross-version probe for #87. Run the same source on main and the branch with
//! the pinned corpus, then compare its JSON-lines output byte for byte.
#[path = "../tests/common/mod.rs"]
mod common;
use haecceity::{json::Anchors, p21::Document};
use serde_json::json;
use sha2::{Digest, Sha256};
use specify_core::{api, features, part, resume, writer};
use std::collections::BTreeMap;

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("SPECIFY_CORPUS_ALL").as_deref() == Ok("1") {
        return Err("this cross-version probe requires the default pinned corpus selection".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut compared = 0;
    for case in common::cases() {
        if !["corpus/mfcadpp/", "corpus/mfcadpp_holdout/", "corpus/nist/"]
            .iter()
            .any(|prefix| case.name.starts_with(prefix))
        {
            continue;
        }
        let Some(path) = case.input() else { continue };
        let Ok(p) = part::load(&path, case.part()) else {
            continue;
        };
        if !seen.insert((path.clone(), p.binding.part)) {
            continue;
        }
        let doc = Document::parse(p.bytes.to_vec())?;
        let names = Anchors::read(&doc, &p.definition).names;
        let mut counts = BTreeMap::new();
        for name in names.iter().flatten() {
            *counts.entry(name).or_insert(0) += 1;
        }
        if counts.is_empty() {
            continue;
        }
        compared += 1;
        let analysis = api::analyse(&p)?;
        let mut answers = serde_json::Map::new();
        answers.insert("part.material".into(), json!("steel"));
        if let Some(f) = features::recognise(&p)?
            .iter()
            .find(|f| f.family == "holes")
        {
            answers.insert(format!("hole.function:{}", f.id), json!("general"));
        }
        let flat = features::describe_faces(&p)?
            .iter()
            .find(|f| f.kind == "plane")
            .map(|f| f.id);
        if let Some(flat) = flat {
            answers.insert("datum.A".into(), json!([flat]));
        }
        let evidence = resume::evidence(&p)?;
        let rematch = resume::rematch_answers(
            &p,
            &features::recognise(&p)?,
            &answers,
            Some(&evidence),
            None,
        )?;
        let (bytes, report) =
            writer::write_bytes(&p, &api::preview(&analysis, &answers)?, Some(&answers))?;
        let back = part::load_bytes("written.step", bytes.clone())?;
        let resumed = api::analyse(&back)?;
        println!(
            "{}",
            json!({
                "case": case.name, "part":p.binding.part, "faces":names.len(), "raw_names":counts,
                "analysis":sha(&serde_json::to_vec(&analysis)?),
                "evidence":sha(&serde_json::to_vec(&evidence)?),
                "rematch":sha(&serde_json::to_vec(&rematch)?),
                "write":sha(&bytes), "write_report":sha(&serde_json::to_vec(&report.to_dict())?),
                "resume":sha(&serde_json::to_vec(&resumed)?),
                "stored_schema":resume::stored(&back.bytes).get(&p.binding.part).and_then(|v| v.get("schema")),
            })
        );
    }
    assert_eq!(
        compared, 20,
        "expected 20 named generic-label parts in the pinned corpus"
    );
    Ok(())
}
