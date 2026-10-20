//! Specify's STEP-only authorship contract; see docs/pmi-provenance.md.
use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use haecceity::pmi::{AttributeSet, AttributeValue, PartPmi, PartProvenance};

pub const MARKER: &str = "specify-core:written:v1";
pub const PROPERTY: &str = "Specify provenance";

pub fn material_attribute() -> AttributeSet {
    AttributeSet {
        name: PROPERTY.into(),
        on: None,
        items: vec![("material".into(), AttributeValue::Text(MARKER.into()))],
        description: None,
        general_property: None,
    }
}

pub fn material(pmi: &PartPmi) -> bool {
    pmi.attributes.iter().any(|a| {
        a.name == PROPERTY
            && a.on.is_none()
            && a.items
                .contains(&("material".into(), AttributeValue::Text(MARKER.into())))
    })
}

fn is_dimension(entity: &RawEntity) -> bool {
    let is_dimension_type = |name: &str| {
        let name = name.to_ascii_lowercase();
        haecceity::express::is_a(&name, "dimensional_size")
            || haecceity::express::is_a(&name, "dimensional_location")
    };
    match entity {
        RawEntity::Simple { name, .. } => is_dimension_type(name),
        RawEntity::Complex { parts, .. } => parts.iter().any(|p| is_dimension_type(&p.name)),
    }
}

pub fn dimension(doc: &Document, ids: &[u64]) -> bool {
    ids.iter().any(|&id| {
        matches!(doc.get(id), Some(RawEntity::Simple { name, attributes, .. })
            if name == "ID_ATTRIBUTE" && string(attributes.first(), MARKER)
                && matches!(attributes.get(1), Some(Attribute::EntityRef(root))
                    if ids.contains(root) && doc.get(*root).is_some_and(is_dimension)))
    })
}

fn string(a: Option<&Attribute>, expected: &str) -> bool {
    matches!(a, Some(Attribute::String(s)) if s == expected)
}

/// Only the item's own root records count. A shared feature or referenced datum must not
/// transfer authorship to a foreign requirement.
pub fn marked(doc: &Document, ids: &[u64], kind: &str) -> bool {
    ids.iter().any(|&id| {
        let Some(RawEntity::Simple {
            name, attributes, ..
        }) = doc.get(id)
        else {
            return false;
        };
        match kind {
            "feature" => {
                matches!(name.as_str(), "SHAPE_ASPECT" | "DATUM_FEATURE")
                    && string(attributes.get(1), MARKER)
            }
            "datum" => name == "DATUM" && string(attributes.get(1), MARKER),
            "thread" => name == "THREAD" && string(attributes.get(1), MARKER),
            "knurl" => name == "TURNED_KNURL" && string(attributes.first(), MARKER),
            "general" | "surface texture" | "attribute" => {
                name == "PROPERTY_DEFINITION"
                    && (kind == "attribute"
                        || string(
                            attributes.first(),
                            if kind == "general" {
                                "default tolerances"
                            } else {
                                "surface texture"
                            },
                        ))
                    && string(attributes.get(1), MARKER)
            }
            _ => false,
        }
    })
}

/// Annotate only records added by this haecceity edit. Input records, including reused
/// datums/features and other assembly parts, are never edited. Reserved AP242 vocabulary
/// (dimension kind, knurl pattern, material property kind) is left intact.
pub fn mark_new(before: &Document, after: &Document) -> Edit {
    let mut edit = Edit::new();
    for (&id, entity) in &after.graph().entities {
        if before.get(id).is_some() {
            continue;
        }
        if is_dimension(entity) {
            edit.add(haecceity::p21::simple(
                "ID_ATTRIBUTE",
                vec![Attribute::String(MARKER.into()), Attribute::EntityRef(id)],
            ));
        }
        let RawEntity::Simple {
            name, attributes, ..
        } = entity
        else {
            continue;
        };
        let slot = match name.as_str() {
            "THREAD" | "DATUM" => Some(1),
            "SHAPE_ASPECT" | "DATUM_FEATURE" if string(attributes.get(1), "") => Some(1),
            "TURNED_KNURL" => Some(0),
            "PROPERTY_DEFINITION" if !string(attributes.get(1), "material name") => Some(1),
            _ => None,
        };
        if let Some(slot) = slot {
            let mut record = entity.clone();
            if let RawEntity::Simple { attributes, .. } = &mut record {
                attributes[slot] = Attribute::String(MARKER.into());
            }
            edit.replace(id, record);
        }
    }
    edit
}

/// Check the no-op shortcut only after every semantic item has gained its marker.
pub fn all(p: &PartPmi, prov: &PartProvenance, doc: &Document) -> bool {
    prov.dimensions.iter().all(|ids| dimension(doc, ids))
        && p.tolerances
            .iter()
            .all(|t| t.description.as_deref() == Some(MARKER))
        && [
            (&prov.features, "feature"),
            (&prov.datums, "datum"),
            (&prov.general, "general"),
            (&prov.threads, "thread"),
            (&prov.knurls, "knurl"),
            (&prov.surface_textures, "surface texture"),
            (&prov.attributes, "attribute"),
        ]
        .iter()
        .all(|(items, kind)| items.iter().all(|ids| marked(doc, ids, kind)))
        && (p.material.is_none() || material(p))
        && p.holes.is_empty()
        && p.notes.is_empty()
        && p.datum_targets.is_empty()
        && p.tolerance_relations.is_empty()
        && p.standards.is_empty()
        && p.geometry.is_empty()
        && p.decimal_places.is_none()
}
