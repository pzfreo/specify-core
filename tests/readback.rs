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
use specify_core::part::{LoadedPart, load};
use specify_core::rules::Intent;
use specify_core::writer::{requirement_kinds, write};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join("specify-core-readback-tests")
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
    specify_core::api::analyse(&loaded).map_err(|e| format!("not analysed: {e:?}"))
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
    for key in specify_core::rules::NOTES {
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
    let mut facts: Vec<String> = haecceity::presentation::dimension_text(d)
        .split_whitespace()
        .map(str::to_string)
        .collect();
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
        let count = specify_core::part::load_all(&path)
            .map(|p| p.len())
            .unwrap_or(1);
        for part in 0..count {
            let Ok(loaded) = load(&path, (count > 1).then_some(part)) else {
                continue;
            };
            let reading = specify_core::existing::read(&loaded).unwrap();
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
    specify_core::api::analyse(&loaded).unwrap()
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
    let analysis = specify_core::api::analyse(&load(&path, Some(0)).unwrap()).unwrap();
    let on = |kind: &str, faces: Value| find(&analysis, "geometric_tolerance", kind, faces);
    let mmc = on("Position", json!([18, 19, 22, 23]));
    assert_eq!(mmc["text"], "⌖ ⌀0.016000Ⓜ A B C", "{mmc}");
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
        separate["text"], "⌖ ⌀0.025000Ⓜ separate requirement A DⓂ EⓂ",
        "{separate}"
    );
    assert_eq!(separate["text_only"], true, "{separate}");
    let profile = on("ProfileOfSurface", json!((56..=71).collect::<Vec<_>>()));
    assert_eq!(profile["text"], "⌓ 0.020000 A FⓂ", "{profile}");
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
    assert_eq!(size["text"], "⌀0.250000 +0.003000/0", "{size}");
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
