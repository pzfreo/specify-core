//! Native reference for scripts/wasm-parity.mjs. No browser or generated JS involved.
use std::io::{self, Read};

use serde_json::{Map, Value, json};
use specify_core::{api, part, writer};
use specify_core_wasm::{accept_defaults, interpret, session::Session};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let request: Value = serde_json::from_str(&input)?;
    if let Some(pair) = request["diff"].as_array() {
        let old = part::File::read(std::path::Path::new(pair[0].as_str().ok_or("old path")?))?;
        let new = part::File::read(std::path::Path::new(pair[1].as_str().ok_or("new path")?))?;
        println!(
            "{}",
            specify_core_wasm::session::diff_with_options(
                ("old.step", old.bytes()),
                ("new.step", new.bytes()),
                "Spindle",
                specify_core::revision::DiffOptions { scenes: true },
            )?
        );
        return Ok(());
    }
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
    let accepted = accept_defaults(&analysis, &answers)?;
    // The edit list the browser applies (`edit`): the answers as fills, all or nothing.
    let list = json!({ "fill": answers });
    let edited = match writer::apply::apply(
        &loaded,
        &specify_core::edits::parse_value(list.clone()).map_err(|f| format!("{f:?}"))?,
    ) {
        Ok(applied) => json!({"written": true, "bytes": applied.bytes, "report": applied.to_dict(),
            "source_sha256": loaded.binding.source_sha256}),
        Err(failures) => json!({"written": false, "failures": failures}),
    };
    assert_eq!(
        session.edit(&list.to_string())["written"],
        edited["written"]
    );
    println!(
        "{}",
        json!({
            "snapshot": snapshot,
            "pmi_scene": session.pmi_scene(&answers)?,
            "review": session.review(&answers)?,
            "free_review": api::review(&analysis, &answers)?,
            "accepted": accepted,
            "defaults_without_answers": accept_defaults(&analysis, &Map::new())?,
            "accepted_review": session.review(&accepted)?,
            "accepted_free_review": api::review(&analysis, &accepted)?,
            "accepted_settle": session.settle(&specify_core::tolerancing::State {
                answers: accepted.clone(), auto: Some(vec![]), gdt: None,
            })?,
            "interpret": interpret(&analysis, &answers,
                request["question_id"].as_str().ok_or("missing question_id")?,
                request["text"].as_str().ok_or("missing text")?)?,
            "edit": edited,
            "check": writer::apply::check(&loaded, &list.to_string()).to_json(),
            "table": analysis["table"],
        })
    );
    Ok(())
}
