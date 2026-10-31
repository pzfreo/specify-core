//! quiddity-rust#137 emits an invalid empty tessellated curve for a blank note row.
//! Render an explicit `(blank)` row, then restore its exact literal semantic text.
use super::*;

const BLANK: &str = "(blank)";

type Restore = (PartId, AttributeSet, String);

pub(super) fn prepare(items: &mut [(PartId, PartPmi)]) -> Vec<Restore> {
    let mut restore = Vec::new();
    for (part, pmi) in items {
        for set in &mut pmi.attributes {
            if set.name != SEMANTIC_TEXT || set.on.is_some() {
                continue;
            }
            let mut original = None;
            for (key, value) in &mut set.items {
                if key == "general note"
                    && let AttributeValue::Text(text) = value
                    && text.trim().is_empty()
                {
                    original = Some(std::mem::replace(text, BLANK.into()));
                }
            }
            if let Some(original) = original {
                restore.push((*part, set.clone(), original));
            }
        }
    }
    restore
}

pub(super) fn restore(
    before: &Document,
    after: Document,
    defs: &[PartDefinition],
    restore: &[Restore],
) -> Result<Document, WriteError> {
    if restore.is_empty() {
        return Ok(after);
    }
    let read = pmi::read(&after, defs).map_err(|e| WriteError::Failed(e.to_string()))?;
    let mut edit = p21::Edit::new();
    let mut used = std::collections::BTreeSet::new();
    for (part, rendered, literal) in restore {
        let i = read.parts[part.0]
            .attributes
            .iter()
            .enumerate()
            .find_map(|(i, a)| {
                (a.name == rendered.name
                    && a.on == rendered.on
                    && a.items == rendered.items
                    && !used.contains(&(part.0, i))
                    && read.provenance.parts[part.0].attributes[i]
                        .iter()
                        .any(|id| before.get(*id).is_none()))
                .then_some(i)
            })
            .ok_or_else(|| WriteError::Failed("cannot find the rendered blank note".into()))?;
        used.insert((part.0, i));
        let mut found = false;
        for &id in &read.provenance.parts[part.0].attributes[i] {
            if before.get(id).is_some() {
                continue;
            }
            let Some(RawEntity::Simple {
                name, attributes, ..
            }) = after.get(id)
            else {
                continue;
            };
            if name == "DESCRIPTIVE_REPRESENTATION_ITEM"
                && attributes
                    == &vec![
                        Attribute::String("general note".into()),
                        Attribute::String(BLANK.into()),
                    ]
            {
                let mut entity = after.get(id).unwrap().clone();
                if let RawEntity::Simple { attributes, .. } = &mut entity {
                    attributes[1] = Attribute::String(literal.clone());
                }
                edit.replace(id, entity);
                found = true;
            }
        }
        if !found {
            return Err(WriteError::Failed(
                "cannot restore the blank note's semantic text".into(),
            ));
        }
    }
    let applied = after
        .apply(&edit)
        .map_err(|e| WriteError::Failed(e.to_string()))?;
    Document::parse(applied.bytes).map_err(|e| WriteError::Failed(e.to_string()))
}
