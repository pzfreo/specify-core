//! Native reference for scripts/wasm-parity.mjs. No browser or generated JS involved.
use std::io::{self, Read};

use serde_json::{Map, Value, json};
use specify_core::{api, part, writer};
use specify_core_wasm::{interpret, session::Session};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let request: Value = serde_json::from_str(&input)?;
    let name = request["name"].as_str().ok_or("missing name")?;
    let bytes = std::fs::read(request["path"].as_str().ok_or("missing path")?)?;
    let answers: Map<String, Value> = serde_json::from_value(request["answers"].clone())?;
    let session = Session::open(name, &bytes)?;
    // Load independently through specify-core too; the browser loader must agree with it.
    let loaded = part::load_bytes(name, bytes)?;
    let analysis = api::analyse(&loaded)?;
    let snapshot = session.snapshot();
    assert_eq!(snapshot["analysis"], analysis);
    assert_eq!(snapshot["mesh"], api::mesh(&loaded));
    let intent = api::preview(&analysis, &answers)?;
    let (bare_bytes, report) = writer::write_bytes(&loaded, &intent, Some(&answers))?;
    println!(
        "{}",
        json!({
            "snapshot": snapshot,
            "review": session.review(&answers)?,
            "free_review": api::review(&analysis, &answers)?,
            "interpret": interpret(&analysis, &answers,
                request["question_id"].as_str().ok_or("missing question_id")?,
                request["text"].as_str().ok_or("missing text")?)?,
            "bare": {"bytes": bare_bytes, "report": report.to_dict(),
                "source_sha256": loaded.binding.source_sha256},
            "project": session.write_pmi(&answers, request["project"].as_str())?,
        })
    );
    Ok(())
}
