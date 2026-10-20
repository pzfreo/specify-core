//! Specify's STEP-only authorship contract; see docs/pmi-provenance.md.
use haecceity::p21::{Attribute, Document, Edit, RawEntity};
use haecceity::pmi::{
    AttributeSet, AttributeValue, DimensionId, NoteOwner, PartPmi, PartProvenance,
};

pub const MARKER: &str = "specify-core:written:v1";
pub const PROPERTY: &str = "Specify provenance";

pub fn dimension_attribute(index: usize) -> AttributeSet {
    AttributeSet {
        name: PROPERTY.into(),
        on: Some(NoteOwner::Dimension(DimensionId(index))),
        items: vec![("writer".into(), AttributeValue::Text(MARKER.into()))],
        description: None,
        general_property: None,
    }
}

pub fn dimension(pmi: &PartPmi, index: usize) -> bool {
    pmi.attributes.iter().any(|a| {
        a.name == PROPERTY
            && a.on == Some(NoteOwner::Dimension(DimensionId(index)))
            && a.items
                .contains(&("writer".into(), AttributeValue::Text(MARKER.into())))
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
            "material" => name == "REPRESENTATION" && string(attributes.first(), MARKER),
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
            "REPRESENTATION" if string(attributes.first(), "material name") => Some(0),
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
    p.dimensions
        .iter()
        .enumerate()
        .all(|(i, _)| dimension(p, i))
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
        && (p.material.is_none() || marked(doc, &prov.material, "material"))
        && p.holes.is_empty()
        && p.notes.is_empty()
        && p.datum_targets.is_empty()
        && p.tolerance_relations.is_empty()
        && p.standards.is_empty()
        && p.geometry.is_empty()
        && p.decimal_places.is_none()
}
