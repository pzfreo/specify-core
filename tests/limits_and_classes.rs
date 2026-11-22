//! Limits and combined ISO 2768 classes preserve their meaning across all public views.
mod common;

use haecceity::{p21::Document, pmi};
use serde_json::{Map, Value, json};
use specify_core::{
    api, choices, existing, general, part, pmi_scene, revision, rules, standards, writer,
};
use std::path::Path;

fn plate() -> part::LoadedPart {
    part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parts/plate.step"),
        None,
    )
    .unwrap()
}
fn model(p: &part::LoadedPart) -> pmi::PartPmi {
    pmi::read(
        &Document::parse(p.bytes.to_vec()).unwrap(),
        std::slice::from_ref(&p.definition),
    )
    .unwrap()
    .parts
    .remove(0)
}
fn write(p: &part::LoadedPart, given: &Map<String, Value>) -> part::LoadedPart {
    let i = api::preview(&api::analyse(p).unwrap(), given).unwrap();
    let (bytes, report) = writer::write_bytes(p, &i, Some(given)).unwrap();
    if given
        .get("extra.size")
        .is_some_and(|v| v["value"] == "7.8/8.2")
    {
        assert!(
            report.written.iter().any(|line| line.contains("7.8/8.2")),
            "{report:?}"
        );
    }
    if given
        .get("extra.size")
        .is_some_and(|v| v["value"] == "±0.2")
    {
        assert!(
            report.written.iter().any(|line| line.contains("Ø8 ±0.2")),
            "{report:?}"
        );
    }
    part::load_bytes("limits.step", bytes).unwrap()
}
fn given(size: &str, distance: &str) -> Map<String, Value> {
    json!({"extra.size":{"kind":"size","faces":[9],"value":size},
        "extra.distance":{"kind":"distance","faces":[2],"reference":[4],"value":distance}})
    .as_object()
    .unwrap()
    .clone()
}
fn notation(scene: &Value) -> String {
    scene["specify"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(|m| m["notation"].as_str())
        .collect::<Vec<_>>()
        .join("; ")
}
#[test]
fn answered_size_and_plate_thickness_round_trip_as_limits() {
    let p = plate();
    let answers = given("7.8/8.2", "9.8/10.2");
    let a = api::analyse(&p).unwrap();
    let intent = api::preview(&a, &answers).unwrap();
    for r in &intent.requirements {
        if r.contains_key("limits") {
            let nominal = r
                .get("nominal")
                .or_else(|| r.get("distance"))
                .unwrap()
                .as_f64()
                .unwrap();
            assert!(
                (r["upper"].as_f64().unwrap() - (r["limits"][1].as_f64().unwrap() - nominal)).abs()
                    < 1e-9
            );
            assert!(
                (r["lower"].as_f64().unwrap() - (r["limits"][0].as_f64().unwrap() - nominal)).abs()
                    < 1e-9
            );
        }
    }
    let applied = common::apply_edits(&p, json!({"fill": answers}));
    assert!(
        applied
            .report
            .written
            .iter()
            .any(|line| line.contains("7.8/8.2")),
        "{:?}",
        applied.report
    );
    let written = part::load_bytes("limits.step", applied.bytes).unwrap();
    assert_eq!(
        model(&written)
            .dimensions
            .iter()
            .filter(|d| matches!(d.tolerance, pmi::DimTolerance::Limits(_)))
            .count(),
        2
    );
    let found = existing::read_existing(&written).unwrap();
    assert_eq!(found.iter().filter(|e| e.limits.is_some()).count(), 2);
    let resumed = api::analyse(&written).unwrap();
    for id in answers.keys() {
        assert!(resumed["resume"]["answers"][id].is_null(), "{id}");
    }
    let reapply = common::apply_edits(&written, json!({}));
    assert!(reapply.unchanged);
    assert_eq!(reapply.bytes, written.bytes.as_slice());
    let rewritten = part::load_bytes("unchanged.step", reapply.bytes).unwrap();
    assert_eq!(model(&rewritten).dimensions, model(&written).dimensions);
    for p in [&written, &rewritten] {
        let scene = pmi_scene::pmi_scene(&p.bytes, None).unwrap();
        let labels = notation(&scene);
        assert!(
            labels.contains("7.8 - ⌀8.2") && labels.contains("9.8 - 10.2"),
            "{labels}"
        );
        assert!(!labels.contains('±'), "{labels}");
    }
    // Changing the representation alone is a real drawing change.
    let items: Vec<_> = found
        .iter()
        .map(|item| format!("#{}", item.number.unwrap()))
        .collect();
    let changed = common::apply_edits(
        &written,
        json!({
            "items": {"remove": items},
            "fill": given("±0.2", "±0.2")
        }),
    );
    let previous: Vec<_> = found.iter().map(|item| item.number.unwrap()).collect();
    let mut retired = previous.clone();
    retired.sort_unstable();
    assert_eq!(changed.retired, retired);
    assert!(
        changed
            .report
            .written
            .iter()
            .any(|line| line.contains("Ø8 ±0.2")),
        "{:?}",
        changed.report
    );
    let pm = part::load_bytes("deviations.step", changed.bytes).unwrap();
    let after = existing::read_existing(&pm).unwrap();
    let mut issued: Vec<_> = after.iter().map(|item| item.number.unwrap()).collect();
    issued.sort_unstable();
    let max = *previous.iter().max().unwrap();
    assert_eq!(issued, [max + 1, max + 2]);
    for before in found {
        let item = after
            .iter()
            .find(|item| item.r#type == before.r#type)
            .unwrap();
        assert_eq!(item.faces, before.faces);
        assert_eq!(item.faces2, before.faces2);
        assert_eq!(item.upper, Some(0.2));
        assert_eq!(item.lower, Some(-0.2));
        assert!(item.limits.is_none());
    }
    let report = revision::diff(&written, &pm).unwrap();
    let changes: Vec<_> = report
        .pmi
        .items
        .iter()
        .filter(|c| c.class == "changed" && c.kind == "dimension")
        .collect();
    assert_eq!(changes.len(), 2, "{:?}", report.pmi.items);
    assert!(
        changes
            .iter()
            .all(|c| c.old.as_ref().unwrap().contains(" - ")
                && c.new.as_ref().unwrap().contains('±')),
        "{changes:?}"
    );
    let markdown = report.markdown("Limits", None);
    assert!(
        markdown.contains("9.8 - 10.2") && markdown.contains('±'),
        "{markdown}"
    );
}
#[test]
fn grammar_preserves_signed_deviations_and_requires_ordered_absolute_bounds() {
    let a = api::analyse(&plate()).unwrap();
    for text in [
        "±0.05",
        "+-0.05",
        "+/-0.05",
        "0.05",
        "+0.1/-0",
        "-0.05/-0.1",
        "+0.1/+0.05",
        "+0.1/0",
    ] {
        let i = api::preview(&a, &given(text, text)).unwrap();
        assert!(
            i.requirements.iter().all(|r| !r.contains_key("limits")),
            "{text}"
        );
    }
    for text in [
        "10.2/9.8",
        "10/10",
        "9.8–10.2",
        "9.8-10.2",
        "limits:NaN/10",
        "limits:9/inf",
    ] {
        assert!(api::preview(&a, &given(text, text)).is_err(), "{text}");
    }
    let signed = api::preview(&a, &given("limits:-8.2/-7.8", "limits:-10.2/-9.8")).unwrap();
    assert!(
        signed
            .requirements
            .iter()
            .filter(|r| r.contains_key("limits"))
            .all(|r| r["limits"][0].as_f64().unwrap() < 0.)
    );
}
#[test]
fn combined_classes_are_choices_and_round_trip_without_changing_defaults() {
    let p = plate();
    let a = api::analyse(&p).unwrap();
    let q = rules::questions(&a, &Map::new())
        .unwrap()
        .into_iter()
        .find(|q| q.id == "part.general_tolerance")
        .unwrap();
    assert_eq!(q.default, Some(json!("ISO 2768-m")));
    assert_eq!(q.options.len(), 16);
    for linear in ['f', 'm', 'c', 'v'] {
        for geometric in ['H', 'K', 'L'] {
            let name = format!("ISO 2768-{linear}{geometric}");
            assert!(q.options.contains(&name));
            assert_eq!(
                general::deviation(&name, 24.),
                general::deviation(&format!("ISO 2768-{linear}"), 24.)
            );
            assert_eq!(
                choices::interpret(
                    &q.id,
                    &format!("{linear}{geometric}"),
                    &standards::GENERAL_TOLERANCES,
                    None,
                    None
                )
                .unwrap(),
                name
            );
        }
    }
    let answers = json!({"part.general_tolerance":"ISO 2768-mK"})
        .as_object()
        .unwrap()
        .clone();
    let combined = api::preview(&a, &answers).unwrap();
    let plain = api::preview(
        &a,
        json!({"part.general_tolerance":"ISO 2768-m"})
            .as_object()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(combined.general, plain.general);
    assert_eq!(combined.requirements, plain.requirements);
    let written = write(&p, &answers);
    let read = model(&written);
    let pmi::GeneralTolerance::Class {
        text,
        standard: Some(iso),
    } = &read.general[0]
    else {
        panic!("{:?}", read.general);
    };
    assert_eq!(text, "ISO 2768-mK");
    assert_eq!(iso.geometric, Some(pmi::standards::GeometricClass::K));
    assert_eq!(
        existing::part_settings(&written).unwrap()["general_tolerance"],
        "ISO 2768-mK"
    );
    let resumed = api::analyse(&written).unwrap();
    // ADR P11: the class writes PMI, so the file states it rather than storing the answer.
    let row = resumed["table"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "part.general_tolerance")
        .unwrap();
    assert_eq!(row["state"], "stated", "{row}");
    assert_eq!(row["value"], "ISO 2768-mK", "{row}");
    assert!(
        notation(&pmi_scene::pmi_scene(&written.bytes, None).unwrap())
            .contains("GENERAL TOLERANCES ISO 2768-mK")
    );
    assert_eq!(model(&write(&written, &answers)).general, read.general);
    let plain_answers = json!({"part.general_tolerance":"ISO 2768-m"});
    let before = write(&p, plain_answers.as_object().unwrap());
    let report = revision::diff(&before, &written).unwrap();
    let changed: Vec<_> = report
        .pmi
        .items
        .iter()
        .filter(|c| c.class == "changed")
        .collect();
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert_eq!(changed[0].kind, "general_tolerance");
    assert_eq!(
        changed[0].old.as_deref(),
        Some("GENERAL TOLERANCES ISO 2768-m")
    );
    assert_eq!(
        changed[0].new.as_deref(),
        Some("GENERAL TOLERANCES ISO 2768-mK")
    );
}

#[test]
fn nist_ctc01_imported_limits_survive_analysis_resume_and_rewrite() {
    let p = part::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/nist/nist_ctc_01_asme1_ap242.stp.gz"),
        None,
    )
    .unwrap();
    let check = |p: &part::LoadedPart| {
        let found = existing::read_existing(p).unwrap();
        for faces in [[71, 72], [73, 74]] {
            let d = found
                .iter()
                .find(|d| {
                    d.kind == "dimension"
                        && d.r#type == "Size_Diameter"
                        && d.faces == faces
                        && d.limits.is_some()
                })
                .unwrap();
            assert_eq!(d.limits, Some([34.8, 35.2]));
            assert_eq!((d.upper, d.lower), (Some(0.2), Some(-0.2)));
        }
        assert!(notation(&pmi_scene::pmi_scene(&p.bytes, None).unwrap()).contains("34.8 - ⌀35.2"));
    };
    check(&p);
    let a = api::analyse(&p).unwrap();
    assert_eq!(
        a["existing"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["limits"] == json!([34.8, 35.2]))
            .count(),
        2
    );
    let answers = json!({"part.general_tolerance":"ISO 2768-mK"})
        .as_object()
        .unwrap()
        .clone();
    let written = write(&p, &answers);
    check(&written);
    // Add mode stores the answers beside the foreign PMI: both come back on reopening.
    let reopened = api::analyse(&written).unwrap();
    let resumed = reopened["resume"]["answers"].as_object().unwrap().clone();
    // ADR P11: stored are the answers that leave no PMI behind.
    assert_eq!(resumed, specify_core::feature_table::storable(&answers));
    assert_eq!(
        reopened["existing"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["limits"] == json!([34.8, 35.2]))
            .count(),
        2
    );
    check(&write(&written, &resumed));
}

#[test]
fn conflicting_size_diagnostics_keep_limit_labels_and_fits() {
    let a = api::analyse(&plate()).unwrap();
    let answers = json!({"hole.function:holes:9":"fit:H7",
        "extra.limit":{"kind":"size","faces":[9],"value":"7.8/8.2"}});
    let problems = specify_core::problems::problems(&a, answers.as_object().unwrap()).unwrap();
    assert!(
        problems.iter().any(|p| p["message"]
            .as_str()
            .is_some_and(|text| text.contains("sized twice")
                && text.contains("H7")
                && text.contains("7.8/8.2"))),
        "{problems:?}"
    );
}
