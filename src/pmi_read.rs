//! haecceity's whole-file PMI read ([`haecceity::pmi::read`]), remembered for the last few
//! files read.
//!
//! One save reads the same STEP bytes many times over (the analysis of a part, its feature
//! table, the writer's source, its checks), and the read walks the whole file each time. The
//! read is a function of the document's bytes and the part definitions alone, so a read of
//! bytes already read is the one made before. A hit is confirmed against the stored bytes and
//! definitions, never by hash alone, and an error is never kept.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, PoisonError};

use haecceity::p21::Document;
use haecceity::pmi::{PmiRead, ReadError};
use haecceity::step::PartDefinition;

/// How many files' reads are kept: a save works on the file as it was, as each part's edit
/// leaves it, and as saved.
const KEPT: usize = 6;

struct Entry {
    hash: u64,
    bytes: Vec<u8>,
    definitions: String,
    read: PmiRead,
}

static KEPT_READS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

/// [`haecceity::pmi::read`] of `doc` and `parts` (the part definitions of the same bytes).
pub(crate) fn read(doc: &Document, parts: &[PartDefinition]) -> Result<PmiRead, ReadError> {
    // Not `{parts:?}`: a definition's two lookup tables (derived from its faces and edges)
    // print in no fixed order.
    let definitions = parts
        .iter()
        .map(|d| {
            format!(
                "{} {} {:?} {:?} {:?} {:?} {:?} {:?}",
                d.product_definition,
                d.shape,
                d.name,
                d.product_id,
                d.declared_length_unit,
                d.faces,
                d.edges,
                d.placements
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut hasher = DefaultHasher::new();
    doc.bytes().hash(&mut hasher);
    definitions.hash(&mut hasher);
    let hash = hasher.finish();
    {
        let kept = KEPT_READS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = kept
            .iter()
            .find(|e| e.hash == hash && e.definitions == definitions && e.bytes == doc.bytes())
        {
            return Ok(entry.read.clone());
        }
    }
    let read = haecceity::pmi::read(doc, parts)?;
    let mut kept = KEPT_READS.lock().unwrap_or_else(PoisonError::into_inner);
    if kept.len() >= KEPT {
        kept.remove(0);
    }
    kept.push(Entry {
        hash,
        bytes: doc.bytes().to_vec(),
        definitions,
        read: read.clone(),
    });
    Ok(read)
}
