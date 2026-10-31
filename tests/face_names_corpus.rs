//! #87 round 1: the 20 named generic-label corpus parts retain pre-name behavior.
mod common;
use serde_json::{Map, Value, json};
use specify_core::{api, features, part, resume, writer};
use std::collections::BTreeSet;

type Snapshot = (Value, Value, Value, Vec<u8>, Value, Value);
fn snapshot(p: &part::LoadedPart, answers: &Map<String, Value>) -> Snapshot {
    let analysis = api::analyse(p).unwrap();
    let evidence = resume::evidence(p).unwrap();
    let rematched = resume::rematch_answers(
        p,
        &features::recognise(p).unwrap(),
        answers,
        Some(&evidence),
        None,
    )
    .unwrap();
    let (written, report) =
        writer::write_bytes(p, &api::preview(&analysis, answers).unwrap(), Some(answers)).unwrap();
    let back = part::load_bytes("written.step", written.clone()).unwrap();
    (
        analysis,
        evidence,
        serde_json::to_value(rematched).unwrap(),
        written,
        report.to_dict(),
        api::analyse(&back).unwrap(),
    )
}

#[test]
fn twenty_named_corpus_parts_preserve_geometric_analysis_write_and_resume() {
    if common::corpus_dir().is_none() {
        return;
    }
    let mut seen = BTreeSet::new();
    let mut compared = 0;
    for case in common::cases() {
        let stem = case
            .name
            .strip_prefix("corpus/")
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("");
        if !common::corpus::SUBSET.contains(&stem) {
            continue;
        }
        if !["corpus/mfcadpp/", "corpus/mfcadpp_holdout/", "corpus/nist/"]
            .iter()
            .any(|prefix| case.name.starts_with(prefix))
        {
            continue;
        }
        let Some(path) = case.input() else { continue };
        let Ok(mut p) = part::load(&path, case.part()) else {
            continue;
        };
        if !seen.insert((path, p.binding.part)) || p.face_names.iter().all(Option::is_none) {
            continue;
        }
        let mut answers = Map::new();
        answers.insert("part.material".into(), json!("steel"));
        if let Some(f) = features::recognise(&p)
            .unwrap()
            .iter()
            .find(|f| f.family == "holes")
        {
            answers.insert(format!("hole.function:{}", f.id), json!("general"));
        }
        if let Some(f) = features::describe_faces(&p)
            .unwrap()
            .iter()
            .find(|f| f.kind == "plane")
        {
            answers.insert("datum.A".into(), json!([f.id]));
        }
        let actual = snapshot(&p, &answers);
        assert_eq!(actual.1["schema"], 2, "{}", case.name);
        assert!(
            actual.0["faces"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f.get("name").is_none()),
            "{}",
            case.name
        );
        // Erase only the interview's cached labels. Keep the original STEP bytes,
        // geometry, binding and foreign PMI: this exercises the pre-#87 path.
        // The independent main-vs-branch example additionally compares two builds.
        p.face_names.fill(None);
        let geometric = snapshot(&p, &answers);
        assert_eq!(actual.0, geometric.0, "{}: analysis", case.name);
        assert_eq!(actual.1, geometric.1, "{}: evidence", case.name);
        assert_eq!(actual.2, geometric.2, "{}: rematch", case.name);
        assert!(actual.3 == geometric.3, "{}: written STEP bytes", case.name);
        assert_eq!(actual.4, geometric.4, "{}: write report", case.name);
        assert_eq!(actual.5, geometric.5, "{}: resumed analysis", case.name);
        compared += 1;
    }
    assert_eq!(
        compared, 20,
        "the pinned default corpus has 12 MFCAD++, 6 holdout and 2 named NIST inputs"
    );
}
