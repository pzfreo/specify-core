//! Shared assertion for successful carry tests, including assembly-part removal.

use std::collections::BTreeMap;

use haecceity::p21::Document;
use serde_json::{Value, json};
use specify_pmi::part::{File, LoadedPart};

pub fn numbered_requirements_are_in_output_or_reported(old: &File, out: &File, report: &Value) {
    let numbered = |part: &LoadedPart| {
        use haecceity::pmi::{self, DimTolerance, Quantity};
        let readback = json!(specify_pmi::existing::read_existing(part).unwrap());
        let mut values: BTreeMap<_, _> = readback
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| {
                let mut fields = item.as_object().unwrap().clone();
                let number = fields.remove("number")?.as_u64()?;
                for key in [
                    "faces",
                    "faces2",
                    "provenance",
                    "origin",
                    "source",
                    "question",
                ] {
                    fields.remove(key);
                }
                Some((number, Value::Object(fields)))
            })
            .collect();
        let doc = Document::parse(part.bytes.to_vec()).unwrap();
        let defs = haecceity::step::read_part_definitions(doc.bytes()).unwrap();
        let read = pmi::read(&doc, &defs).unwrap();
        let k = part.binding.part;
        let mut uses = BTreeMap::new();
        for c in specify_pmi::characteristics::read(part).unwrap() {
            let n = c.number;
            // General dimensions can be hidden from the questions' read-back but still
            // state a numbered nominal. Resolve their exact semantic entity using the
            // kernel provenance instead of identifying them by an approximate face match.
            let nominal = read.provenance.parts[k]
                .dimensions
                .iter()
                .position(|ids| ids.contains(&c.entity))
                .map(|i| &read.parts[k].dimensions[i])
                .filter(|d| matches!(d.tolerance, DimTolerance::None))
                .map(|d| {
                    let raw = d.nominal.as_ref().map(|v| match &v.quantity {
                        Quantity::Length(l) => l.mm(),
                        Quantity::Angle(a) => a.rad().to_degrees(),
                    });
                    json!({"nominal": raw, "qualifier": format!("{:?}", d.qualifier),
                            "modifiers": format!("{:?}", d.modifiers),
                            "principle": format!("{:?}", d.principle)})
                });
            let texture = read.provenance.parts[k].surface_textures.iter().position(|ids| ids.contains(&c.entity))
                .map(|i| &read.parts[k].surface_textures[i])
                .map(|t| json!({"removal": format!("{:?}", t.material_removal),
                    "parameters": t.parameters.iter().map(|v| (&v.characteristic, v.value.mm())).collect::<Vec<_>>() }));
            let value = nominal
                .or(texture)
                .or_else(|| values.remove(&n))
                .unwrap_or_else(|| panic!("#{n} {c:?} of {} has no value in read-back", part.name));
            uses.insert(n, (c, value));
        }
        uses
    };
    let assembly = old.definitions().len() > 1;
    let after: Vec<_> = out
        .load_all()
        .unwrap()
        .iter()
        .map(|part| (part.name.clone(), numbered(part)))
        .collect();
    for part in old.load_all().unwrap() {
        let name = &part.name;
        let output = after.iter().find(|(other, _)| !assembly || other == name);
        for (n, (item, value)) in numbered(&part) {
            let same = |new: u64| {
                output
                    .and_then(|(_, items)| items.get(&new))
                    .is_some_and(|(c, v)| {
                        c.kind == item.kind && c.r#type == item.r#type && *v == value
                    })
            };
            let renumbered = report["renumbered"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|entry| {
                    entry["old"] == n
                        && entry["type"] == item.r#type
                        && entry["new"].as_u64().is_some_and(same)
                });
            let reported = report["not_carried"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| {
                    entry["number"] == format!("#{n}") && (!assembly || entry["part"] == *name)
                });
            assert!(
                same(n) || renumbered || reported,
                "{name} #{n} {} {:?} missing from OUT with its original value and from not_carried",
                item.r#type,
                value
            );
        }
    }
}
