//! What the file states reads back whole (ADR P11: the stated row is all anyone sees of it).
//!
//! - `every_kind_the_writer_writes_reads_back_whole`: each requirement kind the writer writes
//!   (`writer::requirement_kinds`), with each option it takes (fits, deviations, limits,
//!   basic; every tolerance kind plain, diametral, Ⓜ and both, with datums; internal blind and
//!   through and external left-hand threads; knurl, hole note, finish on faces), the datums, and
//!   every part setting the writer writes, is written on the plate, analysed again, and its
//!   stated read-back must carry the same structured values as the requirement and the same
//!   display text. A kind the writer gains without a case here fails.
//! - `other_tools_pmi_reads_back_whole`: on the NIST AP242 files, every semantic fact of each
//!   dimension and tolerance (haecceity's notation of it, its class, limits, modifiers, zone,
//!   datum references and their modifiers) is in the stated text, and an item with a fact the
//!   structured fields do not hold is marked `text_only`. (NIST's presentations are tessellated
//!   graphics with no text literal, so the semantic content is what is visible.)
//! - The spindle Specify 0.2 wrote, and NIST FTC-09 (another tool's fits and Ⓜ), field by field.

mod common;

use std::path::{Path, PathBuf};

use haecceity::pmi::{
    self, DatumModifier, DimTolerance, DimensionModifier, Principle, Qualifier,
    SimpleDatumModifier, ToleranceModifier, ZoneForm,
};
use serde_json::{Map, Value, json};
use specify_pmi::part::{LoadedPart, load};
use specify_pmi::rules::Intent;
use specify_pmi::writer::{requirement_kinds, write};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-pmi-readback-tests")
        .join(format!("{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn plate() -> LoadedPart {
    load(&common::fixtures().join("parts/plate.step"), None).unwrap()
}

fn object(v: Value) -> Map<String, Value> {
    v.as_object().cloned().expect("an object")
}

/// The plate's datums: A the bottom face, B the -x side, C the -y side.
fn datums() -> Vec<Map<String, Value>> {
    vec![
        object(json!({"letter": "A", "faces": [4]})),
        object(json!({"letter": "B", "faces": [0]})),
        object(json!({"letter": "C", "faces": [1]})),
    ]
}

/// One case: a requirement written alone on the plate, the display text its stated row must
/// read, and the requirement's keys whose values the read-back must carry (planner key,
/// read-back key).
struct Case {
    name: String,
    req: Value,
    text: &'static str,
}

/// The planner's key and the read-back's for each structured value of a kind.
fn carried(kind: &str) -> &'static [(&'static str, &'static str)] {
    match kind {
        "size" => &[
            ("nominal", "value"),
            ("fit", "fit"),
            ("upper", "upper"),
            ("lower", "lower"),
            ("limits", "limits"),
        ],
        "location" => &[
            ("distance", "value"),
            ("basic", "basic"),
            ("upper", "upper"),
            ("lower", "lower"),
            ("limits", "limits"),
        ],
        "thread" => &[
            ("side", "side"),
            ("designation", "designation"),
            ("pitch", "pitch"),
            ("spec", "spec"),
            ("class", "class"),
            ("hand", "hand"),
            ("through", "through"),
            ("drill_diameter", "drill_diameter"),
            ("drill_depth", "drill_depth"),
            ("full_thread", "full_thread"),
            ("length", "length"),
        ],
        "knurl" => &[
            ("pattern", "pattern"),
            ("pitch", "pitch"),
            ("diameter", "diameter"),
            ("teeth", "teeth"),
            ("tooth_depth", "tooth_depth"),
            ("root_fillet", "root_fillet"),
            ("helix_angle", "helix_angle"),
        ],
        "note" => &[("text", "text")],
        "finish" => &[("value", "finish")],
        // The geometric tolerances.
        _ => &[
            ("tolerance", "value"),
            ("diametral", "diametral"),
            ("mmc", "mmc"),
            ("datums", "datums"),
        ],
    }
}

/// The symbol a drawing gives each tolerance kind the writer writes.
fn symbol(kind: &str) -> &'static str {
    match kind {
        "position" => "⌖",
        "flatness" => "⏥",
        "perpendicularity" => "⟂",
        "parallelism" => "∥",
        "profile" => "⌓",
        "runout" => "↗",
        "total_runout" => "⌰",
        other => panic!("no symbol for {other}"),
    }
}

fn base(kind: &str, faces: &[usize]) -> Map<String, Value> {
    object(json!({"kind": kind, "feature": format!("{kind} case"), "faces": faces}))
}

fn with(mut req: Map<String, Value>, more: Value) -> Value {
    req.extend(object(more));
    Value::Object(req)
}

/// Every case of `kind`, with each option the writer takes for it.
fn cases(kind: &str) -> Vec<Case> {
    let case = |name: &str, req: Value, text: &'static str| Case {
        name: format!("{kind}: {name}"),
        req,
        text,
    };
    match kind {
        "size" => vec![
            case(
                "H7 fit",
                with(
                    base(kind, &[9]),
                    json!({"nominal": 8.0, "fit": "H7", "upper": 0.015, "lower": 0.0}),
                ),
                "⌀8 H7",
            ),
            case(
                "H11 clearance",
                with(
                    base(kind, &[7]),
                    json!({"nominal": 6.6, "fit": "H11", "upper": 0.09, "lower": 0.0}),
                ),
                "⌀6.6 H11",
            ),
            case(
                "symmetric deviations",
                with(
                    base(kind, &[6]),
                    json!({"nominal": 6.6, "upper": 0.1, "lower": -0.1}),
                ),
                "⌀6.6 ±0.1",
            ),
            case(
                "unequal deviations",
                with(
                    base(kind, &[8]),
                    json!({"nominal": 6.6, "upper": 0.2, "lower": -0.1}),
                ),
                "⌀6.6 +0.2/-0.1",
            ),
            case(
                "limits",
                with(
                    base(kind, &[10]),
                    json!({"nominal": 6.6, "limits": [6.5, 6.7], "upper": 0.1, "lower": -0.1}),
                ),
                "⌀6.5 - ⌀6.7",
            ),
        ],
        "location" => {
            let located = |faces: &[usize], more: Value| {
                let mut req = base(kind, faces);
                req.insert("reference".into(), json!([0]));
                with(req, more)
            };
            vec![
                case(
                    "basic",
                    located(&[11], json!({"distance": 50.0, "basic": true})),
                    "50 (basic)",
                ),
                case(
                    "symmetric deviations",
                    located(
                        &[13],
                        json!({"distance": 80.0, "upper": 0.1, "lower": -0.1}),
                    ),
                    "80 ±0.1",
                ),
                case(
                    "limits",
                    located(
                        &[10],
                        json!({"distance": 20.0, "limits": [19.9, 20.1], "upper": 0.1, "lower": -0.1}),
                    ),
                    "19.9 - 20.1",
                ),
            ]
        }
        "thread" => {
            let internal = json!({
                "side": "internal", "designation": "M5", "pitch": 0.8, "spec": "M5x0.8",
                "class": "6H", "hand": "RH", "drill_diameter": 4.2,
            });
            vec![
                case(
                    "internal, blind",
                    with(
                        object(with(base(kind, &[12]), internal.clone())),
                        json!({"through": false, "drill_depth": 8.0, "full_thread": 6.0}),
                    ),
                    "M5×0.8-6H ↧ 6",
                ),
                case(
                    "internal, through",
                    with(
                        object(with(base(kind, &[12]), internal)),
                        json!({"through": true}),
                    ),
                    "M5×0.8-6H THRU",
                ),
                case(
                    "external, left hand, with a length",
                    with(
                        base(kind, &[9]),
                        json!({
                            "side": "external", "designation": "M8", "pitch": 1.25,
                            "spec": "M8x1.25", "class": "6g", "hand": "LH", "length": 5.0,
                            "nominal": 8.0,
                        }),
                    ),
                    "M8×1.25-6g LH ↧ 5",
                ),
            ]
        }
        "knurl" => vec![case(
            "diamond",
            with(
                base(kind, &[9]),
                json!({
                    "pattern": "diamond", "pitch": 1.0, "diameter": 8.0, "teeth": 25,
                    "tooth_depth": 0.5, "root_fillet": 0.1, "helix_angle": 30.0,
                }),
            ),
            "diamond knurl, pitch 1 mm, major ⌀8 mm",
        )],
        "note" => vec![case(
            "hole note",
            with(base(kind, &[7]), json!({"text": "heat-set insert M3"})),
            "heat-set insert M3",
        )],
        "finish" => vec![case(
            "on faces",
            with(base(kind, &[2]), json!({"value": "Ra 1.6"})),
            "√ Ra 1.6",
        )],
        tolerance => {
            let faces: &[usize] = match tolerance {
                "position" => &[7],
                "runout" | "total_runout" => &[9],
                _ => &[2],
            };
            let datums = if tolerance == "flatness" {
                json!([])
            } else {
                json!(["A", "B", "C"])
            };
            let texts: [&'static str; 4] = match tolerance {
                "position" => [
                    "⌖ 0.1 A B C",
                    "⌖ ⌀0.1 A B C",
                    "⌖ 0.1Ⓜ A B C",
                    "⌖ ⌀0.1Ⓜ A B C",
                ],
                "flatness" => ["⏥ 0.1", "⏥ ⌀0.1", "⏥ 0.1Ⓜ", "⏥ ⌀0.1Ⓜ"],
                "perpendicularity" => [
                    "⟂ 0.1 A B C",
                    "⟂ ⌀0.1 A B C",
                    "⟂ 0.1Ⓜ A B C",
                    "⟂ ⌀0.1Ⓜ A B C",
                ],
                "parallelism" => [
                    "∥ 0.1 A B C",
                    "∥ ⌀0.1 A B C",
                    "∥ 0.1Ⓜ A B C",
                    "∥ ⌀0.1Ⓜ A B C",
                ],
                "profile" => [
                    "⌓ 0.1 A B C",
                    "⌓ ⌀0.1 A B C",
                    "⌓ 0.1Ⓜ A B C",
                    "⌓ ⌀0.1Ⓜ A B C",
                ],
                "runout" => [
                    "↗ 0.1 A B C",
                    "↗ ⌀0.1 A B C",
                    "↗ 0.1Ⓜ A B C",
                    "↗ ⌀0.1Ⓜ A B C",
                ],
                "total_runout" => [
                    "⌰ 0.1 A B C",
                    "⌰ ⌀0.1 A B C",
                    "⌰ 0.1Ⓜ A B C",
                    "⌰ ⌀0.1Ⓜ A B C",
                ],
                other => panic!("no texts for {other}"),
            };
            assert!(texts.iter().all(|t| t.starts_with(symbol(tolerance))));
            [(false, false), (true, false), (false, true), (true, true)]
                .into_iter()
                .zip(texts)
                .map(|((diametral, mmc), text)| {
                    case(
                        &format!("diametral {diametral}, mmc {mmc}"),
                        with(
                            base(tolerance, faces),
                            json!({"tolerance": 0.1, "diametral": diametral, "mmc": mmc, "datums": datums}),
                        ),
                        text,
                    )
                })
                .collect()
        }
    }
}

/// The `existing` items of `analysis` of `kind` on exactly `faces`.
fn stated<'a>(analysis: &'a Value, kind: &str, faces: &Value) -> Vec<&'a Value> {
    let existing_kind = match kind {
        "size" | "location" => "dimension",
        "thread" | "knurl" | "note" | "finish" => "note",
        _ => "geometric_tolerance",
    };
    analysis["existing"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == existing_kind)
        .filter(|e| {
            let mut on = e["faces"].as_array().cloned().unwrap_or_default();
            if kind == "location" {
                on = e["faces2"].as_array().cloned().unwrap_or_default();
            }
            Value::Array(on) == *faces
        })
        .collect()
}

/// Every requirement text the feature table shows: its rows' and its file PMI's.
fn table_texts(analysis: &Value) -> Vec<String> {
    let table = &analysis["table"];
    table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|r| r["requirements"].as_array().cloned().unwrap_or_default())
        .chain(table["file_pmi"].as_array().cloned().unwrap_or_default())
        .filter_map(|r| r["text"].as_str().map(str::to_string))
        .collect()
}

fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => match (a.as_array(), b.as_array()) {
            (Some(x), Some(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y)),
            _ => a == b,
        },
    }
}

/// Whether a planner value is one the read-back leaves out because it says nothing (a false
/// flag, a right hand, no datums).
fn unstated(key: &str, value: &Value) -> bool {
    value.is_null()
        || *value == Value::Bool(false)
        || (key == "hand" && value == "RH")
        || value.as_array().is_some_and(Vec::is_empty)
}

/// Writes `intent` on the plate and analyses the file again.
fn written(dir: &Path, name: &str, intent: &Intent) -> Result<Value, String> {
    let out = dir.join(format!("{}.step", name.replace([':', ' ', ','], "_")));
    write(&plate(), intent, &out, None).map_err(|e| format!("not written: {e:?}"))?;
    let loaded = load(&out, None).unwrap();
    specify_pmi::api::analyse(&loaded).map_err(|e| format!("not analysed: {e:?}"))
}

#[test]
fn every_kind_the_writer_writes_reads_back_whole() {
    let dir = temp("kinds");
    let plate = plate();
    let binding = serde_json::to_value(&plate.binding).unwrap();
    let mut failures = Vec::new();
    for kind in requirement_kinds() {
        let cases = cases(kind);
        assert!(
            !cases.is_empty(),
            "no read-back case for the writer's kind {kind}"
        );
        for case in cases {
            let mut intent = Intent::new(binding.clone());
            intent.datums = datums();
            intent.requirements = vec![object(case.req.clone())];
            let analysis = match written(&dir, &case.name, &intent) {
                Ok(a) => a,
                Err(why) => {
                    failures.push(format!("{}: {why}", case.name));
                    continue;
                }
            };
            let items = stated(&analysis, kind, &case.req["faces"]);
            let [item] = items.as_slice() else {
                failures.push(format!(
                    "{}: {} stated items on its faces",
                    case.name,
                    items.len()
                ));
                continue;
            };
            for (planner, read) in carried(kind) {
                let want = &case.req[*planner];
                let got = &item[*read];
                let ok = if unstated(planner, want) {
                    unstated(planner, got)
                } else {
                    same(want, got)
                };
                if !ok {
                    failures.push(format!(
                        "{}: {planner} {want} reads back as {read} {got}",
                        case.name
                    ));
                }
            }
            if item["text"] != case.text {
                failures.push(format!(
                    "{}: stated text {} is not {:?}",
                    case.name, item["text"], case.text
                ));
            }
            if item.get("text_only").is_some() {
                failures.push(format!("{}: marked text only", case.name));
            }
            if !table_texts(&analysis).iter().any(|t| t == case.text) {
                failures.push(format!(
                    "{}: no feature-table requirement reads {:?} (it has {:?})",
                    case.name,
                    case.text,
                    table_texts(&analysis)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn edit_and_read(loaded: &LoadedPart, edits: Value) -> (Value, Value) {
    let edits = specify_pmi::edits::parse_value(edits).unwrap();
    let result = specify_pmi::writer::apply::apply(loaded, &edits)
        .unwrap_or_else(|failures| panic!("refused {edits:?}: {failures:?}"));
    let report = result.to_dict();
    let reopened = specify_pmi::part::load_bytes("edited.step", result.bytes).unwrap();
    (specify_pmi::api::analyse(&reopened).unwrap(), report)
}

fn existing_except(analysis: &Value, text: &Value) -> Vec<Value> {
    let mut items: Vec<_> = analysis["existing"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["text"] != *text)
        .cloned()
        .collect();
    items.sort_by_cached_key(Value::to_string);
    items
}

#[test]
fn removing_each_writer_kind_preserves_other_requirements_on_its_carrier() {
    let plate = plate();
    let mut intent = Intent::new(serde_json::to_value(&plate.binding).unwrap());
    intent.datums = datums();
    intent.part.insert("material".into(), json!("steel"));
    intent.part.insert("surface_finish".into(), json!("Ra 3.2"));
    let cases: Vec<_> = requirement_kinds()
        .into_iter()
        .map(|kind| {
            let mut case = cases(kind).remove(usize::from(kind == "thread"));
            case.req["faces"] = json!([12]);
            case
        })
        .collect();
    intent.requirements = cases.iter().map(|case| object(case.req.clone())).collect();
    let dir = temp("coexisting-kinds");
    let path = dir.join("all.step");
    write(&plate, &intent, &path, None).unwrap();
    let loaded = load(&path, None).unwrap();
    let before = specify_pmi::api::analyse(&loaded).unwrap();
    let table = &before["table"];
    let all = table["rows"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|row| row["requirements"].as_array().into_iter().flatten())
        .chain(table["file_pmi"].as_array().unwrap());
    for case in &cases {
        let text = json!(case.text);
        let item = all
            .clone()
            .find(|item| item["text"] == text)
            .unwrap_or_else(|| {
                panic!(
                    "{} missing from the shared feature: {}",
                    case.name, before["existing"]
                )
            });
        let removal = match item["number"].as_str() {
            Some(n) => json!(n),
            None => json!({"id": item["id"]}),
        };
        let (after, report) = edit_and_read(&loaded, json!({"items":{"remove":[removal]}}));
        assert_eq!(
            existing_except(&after, &Value::Null),
            existing_except(&before, &text),
            "{}",
            case.name
        );
        assert_eq!(after["part"], before["part"], "{}", case.name);
        let identifier = removal.as_str().or_else(|| removal["id"].as_str()).unwrap();
        assert_eq!(
            report["written"],
            json!([format!("removed {identifier}")]),
            "{}",
            case.name
        );
        assert_eq!(
            report["retired"],
            json!(
                item["number"]
                    .as_str()
                    .map(|n| n[1..].parse::<u64>().unwrap())
                    .into_iter()
                    .collect::<Vec<_>>()
            )
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// Generated PMI with a second requirement on the same feature and a part setting. The
/// named rows exercise every kind they can address. The datum-control and runout rows
/// can replace a control with the kind the question plans. Unaddressed hole notes and
/// face finishes have no change row, so the removal sweep covers those through items.remove.
#[test]
fn changing_each_supported_writer_kind_preserves_its_neighbours() {
    let plate = plate();
    let pin = load(&common::fixtures().join("parts/pin.step"), None).unwrap();
    let fixtures = [
        (
            &plate,
            "size",
            "hole.function:holes:12",
            json!({"value": 8.1}),
            vec![12],
        ),
        (
            &plate,
            "thread",
            "hole.function:holes:12",
            json!("tapped:M5"),
            vec![12],
        ),
        (
            &plate,
            "position",
            "hole.position:holes:12",
            json!("0.1"),
            vec![12],
        ),
        (
            &plate,
            "flatness",
            "datum.A.flatness",
            json!("0.02"),
            vec![4],
        ),
        (
            &plate,
            "perpendicularity",
            "datum.B.control",
            json!("0.02"),
            vec![0],
        ),
        (
            &plate,
            "parallelism",
            "datum.B.control",
            json!("0.02"),
            vec![0],
        ),
        (&plate, "profile", "datum.B.control", json!("0.02"), vec![0]),
        (
            &plate,
            "location",
            "length:planes:0-5",
            json!("100 ±0.3 from B"),
            vec![5],
        ),
        (
            &pin,
            "knurl",
            "diameter.fit:turned_steps:4",
            json!("knurl:straight"),
            vec![4],
        ),
        (
            &pin,
            "runout",
            "diameter.runout:turned_steps:4",
            json!("0.02"),
            vec![4],
        ),
        (
            &pin,
            "total_runout",
            "diameter.runout:turned_steps:4",
            json!("0.02"),
            vec![4],
        ),
    ];
    let remove_only = ["note", "finish"];
    let tested: std::collections::BTreeSet<_> = fixtures
        .iter()
        .map(|(_, kind, ..)| *kind)
        .chain(remove_only)
        .collect();
    assert_eq!(tested, requirement_kinds().into_iter().collect());
    for (part, kind, row_id, to, faces) in fixtures {
        let (expected_text, written) = match kind {
            "size" => ("⌀8.1 H7", "Ø8.1 H7 on holes:12"),
            "thread" => ("M5×0.8-6H THRU", "internal thread M5x0.8-6H on holes:12"),
            "position" => ("⌖ ⌀0.1 A B C", "position 0.1 on holes:12"),
            "flatness" => ("⏥ 0.02", "flatness 0.02 on [4]"),
            "perpendicularity" | "parallelism" | "profile" => {
                ("⟂ 0.02 A", "perpendicularity 0.02 on datum B")
            }
            "location" => ("100 ±0.3", "location 100 ±0.3 to  from faces [0]"),
            "knurl" => (
                "straight knurl, pitch 1 mm, major ⌀6 mm",
                "straight knurl on turned_steps:4",
            ),
            "runout" | "total_runout" => ("↗ 0.02 A", "runout 0.02 on turned_steps:4"),
            _ => unreachable!("no changed value and report for {kind}"),
        };
        let mut intent = Intent::new(serde_json::to_value(&part.binding).unwrap());
        intent.datums = if std::ptr::eq(part, &plate) {
            datums()
        } else {
            vec![object(json!({"letter":"A","faces":[0]}))]
        };
        intent.part.insert("material".into(), json!("steel"));
        let mut target = cases(kind).remove(usize::from(kind == "thread")).req;
        target["faces"] = json!(faces);
        if kind == "thread" {
            target["class"] = json!("5H");
        }
        match kind {
            "perpendicularity" | "parallelism" | "profile" | "runout" | "total_runout" => {
                target["datums"] = json!(["A"])
            }
            "position" => {
                target["datums"] = json!(["A", "B", "C"]);
                target["diametral"] = json!(true);
                target["tolerance"] = json!(0.25);
            }
            _ => {}
        }
        if kind == "location" {
            target["distance"] = json!(100);
            target["reference"] = json!([0]);
            target.as_object_mut().unwrap().remove("basic");
            target["upper"] = json!(0.2);
            target["lower"] = json!(-0.2);
        }
        let companion = with(base("finish", &faces), json!({"value":"Ra 1.6"}));
        let note = with(base("note", &faces), json!({"text":"Keep existing finish"}));
        intent.requirements = vec![object(target), object(companion), object(note)];
        if matches!(kind, "size" | "thread") {
            let frame = with(
                base("position", &faces),
                json!({"tolerance":0.25, "diametral":true, "datums":["A","B","C"]}),
            );
            intent.requirements.push(object(frame));
        }
        if kind == "position" {
            let mut thread = cases("thread").remove(1).req;
            thread["faces"] = json!(faces);
            intent.requirements.push(object(thread));
        }
        let (bytes, _) = specify_pmi::writer::write_bytes(part, &intent, None).unwrap();
        let loaded = specify_pmi::part::load_bytes("change.step", bytes).unwrap();
        let before = specify_pmi::api::analyse(&loaded).unwrap();
        let row = before["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row_id)
            .unwrap_or_else(|| panic!("{kind}: no {row_id}"));
        let parsed: specify_pmi::feature_table::Row = serde_json::from_value(row.clone()).unwrap();
        let expect = specify_pmi::edits::stated_value(&parsed);
        let old = &row["requirements"][0]["text"];
        let (after, report) = edit_and_read(
            &loaded,
            json!({"change":{row_id:{"to":to,"expect":expect}}}),
        );
        let new_row = after["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row_id)
            .unwrap();
        let new = &new_row["requirements"][0]["text"];
        assert_ne!(old, new, "{kind}");
        assert_eq!(new, expected_text, "{kind}");
        assert_eq!(
            existing_except(&after, new),
            existing_except(&before, old),
            "{kind}"
        );
        assert_eq!(after["part"], before["part"], "{kind}");
        assert_eq!(report["written"], json!([written]), "{kind}: {report}");
        assert_eq!(report["not_written"], json!([]), "{kind}: {report}");
        let retired = if matches!(kind, "parallelism" | "profile" | "total_runout") {
            let number = row["requirements"][0]["number"].as_str().unwrap();
            json!([number[1..].parse::<u64>().unwrap()])
        } else {
            json!([])
        };
        assert_eq!(report["retired"], retired, "{kind}: {report}");
    }
}

#[test]
fn a_seeded_thread_shares_the_foreign_ctc04_position_carrier() {
    let loaded = load(
        &common::fixtures().join("nist/nist_ctc_04_asme1_ap242.stp.gz"),
        None,
    )
    .unwrap();
    let foreign = specify_pmi::api::analyse(&loaded).unwrap();
    let position = find(&foreign, "geometric_tolerance", "Position", json!([27, 28]));
    assert_eq!(position["text"], "⌖ ⌀0.35 A B C");
    assert!(position["provenance"].is_null());
    let mut intent = Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    intent.requirements = vec![object(with(
        base("thread", &[27, 28]),
        json!({
            "side":"internal", "designation":"M16", "pitch":2.0, "spec":"M16x2",
            "class":"6H", "hand":"RH", "drill_diameter":14.0, "through":true
        }),
    ))];
    let (seeded, _) = specify_pmi::writer::write_bytes(&loaded, &intent, None).unwrap();
    let seeded = specify_pmi::part::load_bytes("ctc04.step", seeded).unwrap();
    let before = specify_pmi::api::analyse(&seeded).unwrap();
    let thread = find(&before, "note", "internal thread", json!([27, 28]));
    assert_eq!(thread["text"], "M16×2-6H THRU");
    let number = thread["number"].as_u64().unwrap();
    let removal = format!("#{number}");
    let list = specify_pmi::edits::parse_value(json!({"items":{"remove":[removal]}})).unwrap();
    let saved = specify_pmi::writer::apply::apply(&seeded, &list).unwrap();
    let report = saved.to_dict();
    assert_eq!(report["written"], json!([format!("removed {removal}")]));
    assert_eq!(report["not_written"], json!([]));
    assert_eq!(report["retired"], json!([number]));
    let reopened = specify_pmi::part::load_bytes("ctc04.saved.step", saved.bytes).unwrap();
    let after = specify_pmi::api::analyse(&reopened).unwrap();
    assert_eq!(
        existing_except(&after, &Value::Null),
        existing_except(&before, &thread["text"])
    );
    assert_eq!(after["part"], before["part"]);

    // Verify every entity carrying a foreign tolerance is unchanged after both saves.
    let original = haecceity::p21::Document::parse(loaded.bytes.to_vec()).unwrap();
    let defs = haecceity::step::read_part_definitions(original.bytes()).unwrap();
    let pmi = pmi::read(&original, &defs).unwrap();
    for bytes in [&seeded.bytes, &reopened.bytes] {
        let now = haecceity::p21::Document::parse(bytes.to_vec()).unwrap();
        for &id in pmi.provenance.parts[0].tolerances.iter().flatten() {
            assert_eq!(
                &original.bytes()[original.span(id).unwrap()],
                &now.bytes()[now.span(id).unwrap()],
                "foreign tolerance entity #{id}"
            );
        }
    }
}

#[test]
fn changing_or_removing_the_spindles_tapped_hole_position_preserves_its_thread() {
    let path =
        common::fixtures().join("../../ui/test/fixtures/19-spindle-semantic-links-only.step.gz");
    let spindle = load(&path, None).unwrap();
    let before = specify_pmi::api::analyse(&spindle).unwrap();
    let previous = json!("⌖ ⌀0.2 A");
    let current = json!("⌖ ⌀0.1 A");
    let (changed, report) = edit_and_read(
        &spindle,
        json!({"change": {
            "hole.position:holes:15.16": {"to": "0.1", "expect": "0.2"}
        }}),
    );
    assert_eq!(
        existing_except(&changed, &current),
        existing_except(&before, &previous)
    );
    assert_eq!(changed["part"], before["part"]);
    assert_eq!(report["written"], json!(["position 0.1 on holes:15.16"]));
    assert_eq!(report["not_written"], json!([]));
    assert_eq!(
        find(&changed, "geometric_tolerance", "Position", json!([15]))["text"],
        current
    );
    assert_eq!(report["retired"], json!([]));

    let (removed, report) = edit_and_read(&spindle, json!({"items":{"remove":["#12"]}}));
    assert_eq!(
        existing_except(&removed, &Value::Null),
        existing_except(&before, &previous)
    );
    assert_eq!(removed["part"], before["part"]);
    assert_eq!(report["written"], json!(["removed #12"]));
    assert_eq!(report["not_written"], json!([]));
    assert_eq!(report["retired"], json!([12]));

    // Both basics already locate the tapped hole. Changing only its frame must not report
    // those implicit planner rows, even when one is shared with the clearance pattern.
    let plate = plate();
    let given = specify_pmi::edits::parse_value(json!({
        "fill": {
            "hole.function:holes:12": "tapped:M5",
            "hole.position:holes:12": "0.25",
            "datum.A": [2], "datum.B": [1], "datum.C": [0]
        },
        "accept_suggestions": [
            "hole.function:hole_patterns.grid:6.7.8.10.11.13",
            "hole.position:hole_patterns.grid:6.7.8.10.11.13"
        ]
    }))
    .unwrap();
    let seeded = specify_pmi::writer::apply::apply(&plate, &given).unwrap();
    let seeded = specify_pmi::part::load_bytes("plate.step", seeded.bytes).unwrap();
    let (after, report) = edit_and_read(
        &seeded,
        json!({"change":{"hole.position:holes:12":{"to":"0.1","expect":"0.25"}}}),
    );
    assert_eq!(report["written"], json!(["position 0.1 on holes:12"]));
    assert_eq!(report["not_written"], json!([]));
    assert_eq!(
        find(&after, "geometric_tolerance", "Position", json!([12]))["value"],
        0.1
    );
}

#[test]
fn removing_a_position_preserves_the_nist_ctc04_thread() {
    let Some(corpus) = common::corpus_dir() else {
        return;
    };
    // The corpus STEP has bare geometry. Add the semantic M16 thread and a position on
    // one of its tapped bores, deliberately making them use the same STEP carrier.
    let loaded = load(&corpus.join("nist/nist_ctc_04_asme1_rd.stp"), None).unwrap();
    let mut intent = Intent::new(serde_json::to_value(&loaded.binding).unwrap());
    intent.requirements = vec![
        object(with(
            base("thread", &[11]),
            json!({
                "side":"internal", "designation":"M16", "pitch":2.0, "spec":"M16x2",
                "class":"6H", "hand":"RH", "drill_diameter":14.0, "through":true,
            }),
        )),
        object(with(
            base("position", &[11]),
            json!({"tolerance":0.25, "diametral":true}),
        )),
    ];
    let dir = temp("nist-tapped-hole");
    let path = dir.join("ctc04.step");
    write(&loaded, &intent, &path, None).unwrap();
    let loaded = load(&path, None).unwrap();
    let before = specify_pmi::api::analyse(&loaded).unwrap();
    let thread = find(&before, "note", "internal thread", json!([11]));
    assert_eq!(thread["text"], "M16×2-6H THRU");
    let position = find(&before, "geometric_tolerance", "Position", json!([11]));
    let number = format!("#{}", position["number"].as_u64().unwrap());
    let (after, report) = edit_and_read(&loaded, json!({"items":{"remove":[number]}}));
    assert_eq!(
        existing_except(&after, &Value::Null),
        existing_except(&before, &position["text"])
    );
    assert_eq!(after["part"], before["part"]);
    assert_eq!(report["written"], json!([format!("removed {number}")]));
    std::fs::remove_dir_all(dir).unwrap();
}

/// The datums and every part setting the writer writes: the stated rows read their values.
/// The general tolerance's class is here; its block form (a table of ranges) is on another
/// branch: HOOK, add its case to `settings` when it lands.
#[test]
fn the_datums_and_part_settings_read_back_whole() {
    let dir = temp("settings");
    let plate = plate();
    let mut failures = Vec::new();
    let settings: Vec<(&str, Value, Value)> = vec![
        ("material", json!("steel"), json!("steel")),
        (
            "general_tolerance",
            json!("ISO 2768-mK"),
            json!("ISO 2768-mK"),
        ),
        ("surface_finish", json!("Ra 3.2"), json!("Ra 3.2")),
        ("coating", json!("Anodise, clear"), json!("Anodise, clear")),
        (
            "heat_treatment",
            json!("Case harden 0.5"),
            json!("Case harden 0.5"),
        ),
        (
            "edges",
            json!("Break sharp edges 0.2 max"),
            json!("Break sharp edges 0.2 max"),
        ),
        (
            "notes",
            json!(["first", "second"]),
            json!(["first", "second"]),
        ),
    ];
    for key in specify_pmi::rules::NOTES {
        assert!(
            settings.iter().any(|(k, _, _)| *k == key),
            "no read-back case for the part note {key}"
        );
    }
    let mut intent = Intent::new(serde_json::to_value(&plate.binding).unwrap());
    intent.datums = datums();
    intent.requirements = vec![object(with(
        base("position", &[7]),
        json!({"tolerance": 0.1, "diametral": true, "datums": ["A", "B", "C"]}),
    ))];
    for (key, value, _) in &settings {
        intent.part.insert((*key).into(), value.clone());
    }
    let analysis = written(&dir, "settings", &intent).unwrap();
    for (key, _, want) in &settings {
        if analysis["part"][key] != *want {
            failures.push(format!(
                "part.{key}: {want} reads back as {}",
                analysis["part"][key]
            ));
        }
        let row = analysis["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == format!("part.{key}"));
        match row {
            Some(r)
                if r["state"] == "stated"
                    && (r["value"] == *want
                        || r["value"]
                            == want
                                .as_array()
                                .map(|a| {
                                    json!(
                                        a.iter()
                                            .filter_map(Value::as_str)
                                            .collect::<Vec<_>>()
                                            .join("; ")
                                    )
                                })
                                .unwrap_or(Value::Null)) => {}
            other => failures.push(format!("part.{key}: row {other:?}")),
        }
    }
    // The ± form of the general tolerance (a note on the part).
    let mut plus_minus = intent.clone();
    plus_minus
        .part
        .insert("general_tolerance".into(), json!("±0.1"));
    let analysis_pm = written(&dir, "plus-minus", &plus_minus).unwrap();
    if analysis_pm["part"]["general_tolerance"] != "±0.1" {
        failures.push(format!(
            "± general tolerance reads back as {}",
            analysis_pm["part"]["general_tolerance"]
        ));
    }
    for (letter, faces) in [("A", json!([4])), ("B", json!([0])), ("C", json!([1]))] {
        let found = analysis["existing"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == "datum" && e["type"] == letter);
        match found {
            Some(d) if d["faces"] == faces && d["text"] == format!("datum {letter}") => {}
            other => failures.push(format!("datum {letter}: {other:?}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------------------------
// Other tools' files
// ---------------------------------------------------------------------------------------------

fn nist_files() -> Vec<PathBuf> {
    let dir = common::fixtures().join("nist");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(".stp.gz"))
        .collect();
    files.sort();
    assert_eq!(
        files.len(),
        6,
        "the six NIST AP242 files in {}",
        dir.display()
    );
    files
}

fn decompressed(gz: &Path) -> PathBuf {
    let bytes = std::fs::read(gz).unwrap();
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&bytes[..]), &mut out).unwrap();
    common::decompressed(gz, &common::sha256(&out))
}

/// A value as the file states it, in its own unit (as haecceity's notation writes it).
fn stated_value(v: &pmi::Value) -> String {
    haecceity::presentation::tolerance::number(v.decimal())
}

/// A tolerance modifier as a drawing shows it.
fn tolerance_modifier(m: ToleranceModifier) -> String {
    match m {
        ToleranceModifier::MaximumMaterialRequirement => "Ⓜ".into(),
        ToleranceModifier::LeastMaterialRequirement => "Ⓛ".into(),
        ToleranceModifier::FreeState => "Ⓕ".into(),
        ToleranceModifier::TangentPlane => "Ⓣ".into(),
        ToleranceModifier::ReciprocityRequirement => "Ⓡ".into(),
        ToleranceModifier::StatisticalTolerance => "⟨ST⟩".into(),
        other => serde_json::to_value(other)
            .unwrap()
            .as_str()
            .unwrap()
            .replace('_', " "),
    }
}

fn datum_modifier(m: &DatumModifier) -> String {
    match m {
        DatumModifier::Simple(SimpleDatumModifier::MaximumMaterialRequirement) => "Ⓜ".into(),
        DatumModifier::Simple(SimpleDatumModifier::LeastMaterialRequirement) => "Ⓛ".into(),
        DatumModifier::Simple(SimpleDatumModifier::FreeState) => "Ⓕ".into(),
        DatumModifier::Simple(other) => serde_json::to_value(other)
            .unwrap()
            .as_str()
            .unwrap()
            .replace('_', " "),
        DatumModifier::WithValue { value, .. } => stated_value(value),
    }
}

/// What a dimension states, each as text its stated row must contain, and whether any of it is
/// beyond the structured fields.
fn dimension_facts(d: &pmi::Dimension) -> (Vec<String>, bool) {
    // With no value, haecceity's notation is its bare prefix (`⌀`): the kind is said in words.
    let mut facts: Vec<String> = match d.nominal {
        Some(_) => haecceity::presentation::dimension_text(d)
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        None => vec!["no value stated".into()],
    };
    let mut beyond = false;
    if let DimTolerance::Fit { class, limits } = &d.tolerance {
        facts.push(class.to_string());
        if let Some(b) = limits {
            facts.push(stated_value(b.lower()));
            facts.push(stated_value(b.upper()));
        }
    }
    if matches!(d.tolerance, DimTolerance::Basic) {
        facts.push("basic".into());
    }
    if let Some(q) = &d.qualifier {
        beyond = true;
        facts.push(match q {
            Qualifier::Maximum => "MAX".into(),
            Qualifier::Minimum => "MIN".into(),
            Qualifier::Average => "AVG".into(),
            Qualifier::Other(s) => s.clone(),
        });
    }
    for m in &d.modifiers {
        beyond = true;
        facts.push(match m {
            DimensionModifier::Reference => "(".into(),
            other => serde_json::to_value(other)
                .unwrap()
                .as_str()
                .unwrap_or("?")
                .replace('_', " "),
        });
    }
    if d.principle == Some(Principle::EnvelopeRequirement) {
        beyond = true;
        facts.push("Ⓔ".into());
    }
    (facts, beyond)
}

/// What a tolerance states, as [`dimension_facts`].
fn tolerance_facts(t: &pmi::GeometricTolerance, p: &pmi::PartPmi) -> (Vec<String>, bool) {
    let mut facts: Vec<String> = haecceity::presentation::tolerance(t, p)
        .cells
        .iter()
        .flat_map(|c| c.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .collect();
    let mut beyond = false;
    for m in &t.modifiers {
        facts.push(tolerance_modifier(*m));
    }
    if let Some(zone) = &t.zone {
        if !zone.form.is_diametral() && !matches!(zone.form, ZoneForm::Unknown | ZoneForm::Other(_))
        {
            beyond = true;
            if zone.form.is_spherical() {
                facts.push("S⌀".into());
            }
        }
        if let Some(projected) = &zone.projected {
            beyond = true;
            facts.push("Ⓟ".into());
            facts.push(stated_value(&projected.length));
        }
        if zone.non_uniform.is_some() {
            beyond = true;
            facts.push("UF".into());
        }
    }
    if let Some(basis) = &t.unit_basis {
        beyond = true;
        facts.push(format!("/{}", stated_value(&basis.size)));
    }
    if let Some(max) = &t.maximum {
        beyond = true;
        facts.push(stated_value(max));
    }
    if let Some(u) = &t.unequal {
        beyond = true;
        facts.push(format!("UZ{}", stated_value(u)));
    }
    if !t.auxiliary.is_empty() {
        beyond = true;
    }
    for c in t.datums.iter().flat_map(|s| s.compartments()) {
        if c.references.len() > 1 || !c.modifiers.is_empty() {
            beyond = true;
        }
        for m in &c.modifiers {
            facts.push(datum_modifier(m));
        }
        for r in &c.references {
            facts.push(p.datums[r.datum.0].label().to_string());
            for m in &r.modifiers {
                beyond = true;
                facts.push(datum_modifier(m));
            }
        }
    }
    (facts, beyond)
}

#[test]
fn other_tools_pmi_reads_back_whole() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for gz in nist_files() {
        let name = gz.file_name().unwrap().to_string_lossy().to_string();
        let path = decompressed(&gz);
        let count = specify_pmi::part::load_all(&path)
            .map(|p| p.len())
            .unwrap_or(1);
        for part in 0..count {
            let Ok(loaded) = load(&path, (count > 1).then_some(part)) else {
                continue;
            };
            let reading = specify_pmi::existing::read(&loaded).unwrap();
            let p = reading.model();
            let existing: Vec<Value> = reading
                .existing(&loaded)
                .unwrap()
                .iter()
                .map(|e| serde_json::to_value(e).unwrap())
                .collect();
            let of = |kind: &str| {
                existing
                    .iter()
                    .filter(|e| e["kind"] == kind)
                    .collect::<Vec<_>>()
            };
            let (dims, tols) = (of("dimension"), of("geometric_tolerance"));
            assert_eq!(dims.len(), p.dimensions.len(), "{name}#{part}: dimensions");
            assert_eq!(tols.len(), p.tolerances.len(), "{name}#{part}: tolerances");
            let facts = p
                .dimensions
                .iter()
                .map(dimension_facts)
                .zip(dims)
                .chain(p.tolerances.iter().map(|t| tolerance_facts(t, p)).zip(tols));
            for ((facts, beyond), item) in facts {
                checked += 1;
                let text = item["text"].as_str().unwrap_or("");
                // Numbers as the display shows them: float noise and trailing zeros trimmed.
                let facts: Vec<String> = facts
                    .iter()
                    .map(|f| specify_pmi::existing::clean_numbers(f))
                    .collect();
                for fact in &facts {
                    if !text.contains(fact.as_str()) {
                        failures.push(format!(
                            "{name}#{part}: {fact:?} is not in {text:?} ({item})"
                        ));
                    }
                }
                if beyond && item.get("text_only") != Some(&Value::Bool(true)) {
                    failures.push(format!("{name}#{part}: not marked text only: {item}"));
                }
            }
        }
    }
    assert!(checked > 100, "only {checked} items checked");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------------------------
// The two files of the brief, field by field
// ---------------------------------------------------------------------------------------------

fn analysed(path: &Path) -> Value {
    let loaded = load(path, None).unwrap();
    specify_pmi::api::analyse(&loaded).unwrap()
}

fn find<'a>(analysis: &'a Value, kind: &str, r#type: &str, faces: Value) -> &'a Value {
    analysis["existing"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == kind && e["type"] == r#type && e["faces"] == faces)
        .unwrap_or_else(|| panic!("no {kind} {type} on {faces}: {}", analysis["existing"]))
}

/// The spindle Specify 0.2 wrote (semantic PMI linked to graphical PMI) states ⌀3.3 H11,
/// ⌖ ⌀0.3 Ⓜ A and M2×0.4-6H: 0.2.1 showed all three; each read back whole.
#[test]
fn the_spindles_stated_pmi_reads_back_whole() {
    let gz = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("ui/test/fixtures/19-spindle-semantic-links-only.step.gz");
    let analysis = analysed(&decompressed(&gz));
    let bore = find(&analysis, "dimension", "Size_Diameter", json!([14]));
    assert_eq!(bore["fit"], "H11", "{bore}");
    assert!(
        same(&bore["upper"], &json!(0.075)) && same(&bore["lower"], &json!(0.0)),
        "{bore}"
    );
    assert_eq!(bore["text"], "⌀3.3 H11", "{bore}");
    let position = find(&analysis, "geometric_tolerance", "Position", json!([14]));
    assert_eq!(position["diametral"], true, "{position}");
    assert_eq!(position["mmc"], true, "{position}");
    assert_eq!(
        position["modifiers"],
        json!(["maximum_material_requirement"]),
        "{position}"
    );
    assert_eq!(position["text"], "⌖ ⌀0.3Ⓜ A", "{position}");
    let thread = find(&analysis, "note", "internal thread", json!([15]));
    for (key, want) in [
        ("designation", json!("M2")),
        ("spec", json!("M2x0.4")),
        ("pitch", json!(0.4)),
        ("class", json!("6H")),
        ("side", json!("internal")),
        ("full_thread", json!(2.0)),
    ] {
        assert!(same(&thread[key], &want), "{key}: {thread}");
    }
    assert_eq!(thread["text"], "M2×0.4-6H ↧ 2", "{thread}");
    // The feature table's rows read the same.
    let row = |id: &str| {
        analysis["table"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap_or_else(|| panic!("no row {id}"))["value"]
            .clone()
    };
    assert_eq!(row("hole.function:holes:14"), "⌀3.3 H11");
    assert_eq!(row("hole.position:holes:14"), "⌖ ⌀0.3Ⓜ A");
    assert_eq!(row("hole.function:holes:15.16"), "M2×0.4-6H ↧ 2");
}

/// NIST FTC-09 (another tool's AP242, in inches): its Ⓜ position zones, a projected zone,
/// datum references with Ⓜ, and its toleranced sizes, read back with their fields and full
/// text. No NIST file states an ISO 286 class (none has a LIMITS_AND_FITS): the fits are the
/// spindle's and the round trip's.
#[test]
fn another_tools_modifiers_and_zones_read_back_whole() {
    let path = decompressed(&common::fixtures().join("nist/nist_ftc_09_asme1_ap242-e1.stp.gz"));
    let analysis = specify_pmi::api::analyse(&load(&path, Some(0)).unwrap()).unwrap();
    let on = |kind: &str, faces: Value| find(&analysis, "geometric_tolerance", kind, faces);
    let mmc = on("Position", json!([18, 19, 22, 23]));
    assert_eq!(mmc["text"], "⌖ ⌀0.016Ⓜ A B C", "{mmc}");
    assert_eq!(
        (&mmc["diametral"], &mmc["mmc"]),
        (&json!(true), &json!(true)),
        "{mmc}"
    );
    assert_eq!(
        mmc["modifiers"],
        json!(["maximum_material_requirement"]),
        "{mmc}"
    );
    assert!(mmc.get("text_only").is_none(), "{mmc}");
    // Datum references with Ⓜ, and a separate requirement: in the text, and text only.
    let separate = on("Position", json!([26, 27, 28, 29]));
    assert_eq!(
        separate["text"], "⌖ ⌀0.025Ⓜ separate requirement A DⓂ EⓂ",
        "{separate}"
    );
    assert_eq!(separate["text_only"], true, "{separate}");
    let profile = on("ProfileOfSurface", json!((56..=71).collect::<Vec<_>>()));
    assert_eq!(profile["text"], "⌓ 0.02 A FⓂ", "{profile}");
    assert_eq!(profile["text_only"], true, "{profile}");
    // A projected zone (Ⓟ and its length, as the file states it).
    let projected = analysis["existing"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["text"].as_str().is_some_and(|t| t.contains('Ⓟ')))
        .expect("a projected zone");
    assert_eq!(projected["text_only"], true, "{projected}");
    // A size with unequal deviations, in the file's inches; its fields in millimetres.
    let size = find(
        &analysis,
        "dimension",
        "Size_Diameter",
        json!([30, 31, 32, 33, 106, 107]),
    );
    assert_eq!(size["text"], "⌀0.25 +0.003/0", "{size}");
    assert!(
        same(&size["upper"], &json!(0.0762)) && same(&size["lower"], &json!(0.0)),
        "{size}"
    );
    // The feature table's requirements read the same text.
    let texts = table_texts(&analysis);
    for item in [mmc, separate, profile, size] {
        assert!(
            texts.iter().any(|t| *t == item["text"]),
            "{item} not in {texts:?}"
        );
    }
}

/// The display reads as a drawing does: no float noise, no trailing zeros, and a dimension
/// with no value in words, never a bare `⌀` or a type name (NIST CTC-01 and CTC-03).
#[test]
fn stated_text_reads_as_a_drawing_does() {
    let texts = |name: &str| -> Vec<(Value, String)> {
        let path = decompressed(&common::fixtures().join(format!("nist/{name}.stp.gz")));
        let analysis = specify_pmi::api::analyse(&load(&path, None).unwrap()).unwrap();
        analysis["existing"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e.clone(), e["text"].as_str().unwrap_or("").to_string()))
            .collect()
    };
    let noise = regex::Regex::new(r"\d\.\d*0\b|\d{11,}").unwrap();
    for name in ["nist_ctc_01_asme1_ap242", "nist_ctc_03_asme1_ap242"] {
        let all = texts(name);
        for (item, text) in &all {
            assert!(!text.is_empty(), "{name}: {item}");
            assert!(!noise.is_match(text), "{name}: {text:?} ({item})");
            assert!(
                !text.contains('_') && text != "⌀",
                "{name}: {text:?} ({item})"
            );
            if item["kind"] == "dimension" && item.get("value").is_none() {
                assert!(text.ends_with(", no value stated"), "{name}: {text:?}");
            }
        }
        if name.contains("ctc_01") {
            let words: Vec<&str> = all.iter().map(|(_, t)| t.as_str()).collect();
            assert!(words.contains(&"Diameter, no value stated"), "{words:?}");
            assert!(
                words.contains(&"Linear distance, no value stated"),
                "{words:?}"
            );
        } else {
            assert!(all.iter().any(|(_, t)| t == "(0.75)"), "{all:?}");
        }
    }
}
