//! One STEP file loaded once, its parts told apart and its faces numbered (port of `load.py`).
//!
//! Everything downstream (recognition, existing PMI, intent, the writer and the browser's
//! picking) refers to faces by their position in one part's face list. A part is numbered on its
//! own shape, not on a placed instance, in specify-core's order (OpenCascade's
//! `TopExp::MapShapes`), which haecceity's [`PartDefinition`] reproduces: face `i` is the face
//! read from `definition.faces[i]`. Both engines therefore number the same file's faces alike
//! and the binding keeps [`LOADER_VERSION`] 2 (docs/design.md, "Face numbering is the same in
//! both engines"; question Q1).
//!
//! A file of one part placed once (including a part stored as an assembly holding one solid, as
//! the NIST test parts are) is read with [`haecceity::read_step`] and that `Part` is used as it
//! is. A part of an assembly, or a part placed away from the origin, has no `Part` of its own in
//! haecceity yet (upstream need U1): it is read placed back by the inverse of its first
//! placement, and when that read holds other parts or instances too, this part's faces are taken
//! out of it and rebuilt into a `Part` of their own ([`LoadedPart::rebuilt`]). A rebuilt `Part`
//! loses haecceity's record of unresolved faces and edges, so [`LoadedPart`] carries them itself
//! and [`LoadedPart::degraded`] says when a later step must refuse or report rather than trust
//! the rebuilt part's solids.
//!
//! What differs from Python: a gzipped file is refused like any unreadable one (specify-core
//! never reads gzip); a file's parts are haecceity's product definitions, so a product whose
//! shape is a solid plus surface models (NIST CTC-02, 04, 05) is one part here where
//! OpenCascade's XCAF splits it into a part per shape (see `parts`, question Q7); a file with
//! several root shapes, which Python refuses ("expected one part"), is read; part names follow
//! Python's rule over `NEXT_ASSEMBLY_USAGE_OCCURRENCE`s, not over XCAF's labels (see
//! `part_names`, upstream need U9).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use haecceity::Part;
use haecceity::brep::{Face, Loop, Solid};
use haecceity::p21::{Attribute, Document, RawEntity};
use haecceity::step::{self, PartDefinition, Placement, StepError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Bumped whenever a change here could renumber faces for the same bytes. 2 is specify-core's
/// (an assembly holding one solid is numbered on the solid), kept because haecceity numbers faces
/// as specify-core does (question Q1).
pub const LOADER_VERSION: u32 = 2;

/// What face indices are only valid against (Python's `Binding`): the file's bytes, the part's
/// face count, the loader that numbered them and which of the file's parts it is. Serialises in
/// `Binding.to_dict`'s key order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    /// SHA-256 of the file's bytes, lowercase hex.
    pub source_sha256: String,
    /// How many faces the part has.
    pub face_count: usize,
    /// [`LOADER_VERSION`] of the loader that numbered the faces.
    pub loader_version: u32,
    /// Which of the file's parts (see [`parts`]); 0 for a file of one part.
    pub part: usize,
}

/// One part of a STEP file, loaded (Python's `LoadedPart`). Not `Clone`, as `haecceity::Part`
/// is not; everything downstream takes it by reference.
#[derive(Debug)]
pub struct LoadedPart {
    /// The file it was read from.
    pub path: PathBuf,
    /// The file's bytes, read once and shared by every part of the file.
    pub bytes: Arc<Vec<u8>>,
    pub binding: Binding,
    /// This part's own solid(s) in its own frame, face `i` read from `definition.faces[i]`.
    pub part: Part,
    /// haecceity's account of the part: its product definition, face and edge `#N`s, placements.
    pub definition: PartDefinition,
    /// How many parts the file holds; this is part `binding.part` of them.
    pub part_count: usize,
    /// The part's name in the file (Python's `_part_name`; see [`parts`]).
    pub name: String,
    /// Where each instance of the part is placed in the file, rows of `[R | t]` in millimetres.
    pub placements: Vec<Placement>,
    /// The part was rebuilt from a read of the whole file by the U1 stand-in, rather than being
    /// haecceity's own `Part` of the read; its `Part` then has no face or edge sources and no
    /// unresolved lists (use `unresolved_faces` and `unresolved_edges` here).
    pub rebuilt: bool,
    /// The faces (this part's indices) whose surface haecceity could not resolve: the source
    /// read's `Part::unresolved_faces`, re-indexed into this part's numbering.
    pub unresolved_faces: Vec<usize>,
    /// The edges (indices into `part.edges`) whose curve haecceity could not resolve, re-indexed
    /// likewise.
    pub unresolved_edges: Vec<usize>,
}

impl LoadedPart {
    /// The `#N` of the `ADVANCED_FACE` (or `FACE_SURFACE`) face `index` was read from (Python's
    /// `face_ranks` and `requirements.face_entities_of`). Panics if `index` is not a face of the
    /// part, as indexing does.
    pub fn face_entity(&self, index: usize) -> u64 {
        self.definition.faces[index]
    }

    /// Why the part cannot be trusted as a whole solid, if it cannot: it was rebuilt by the U1
    /// stand-in and has unresolved faces or edges. haecceity's `Part::new` drops the unresolved
    /// lists, so the rebuilt part's `solid_is_valid` and the recognisers that skip unresolved
    /// faces would take it for a valid solid. Recognition, meshing and anchors consult this and
    /// `unresolved_faces`, and refuse or report rather than run on such a part.
    pub fn degraded(&self) -> Option<String> {
        if !self.rebuilt || (self.unresolved_faces.is_empty() && self.unresolved_edges.is_empty()) {
            return None;
        }
        Some(format!(
            "part {} ({}) was taken out of the file's assembly and rebuilt without haecceity's \
             record of what did not resolve (upstream need U1); unresolved faces {:?}, \
             unresolved edges {:?}",
            self.binding.part, self.name, self.unresolved_faces, self.unresolved_edges
        ))
    }
}

/// One distinct part of a file as `parts` lists it: Python's `{part, name, faces, instances,
/// placements}` in that key order (the API adds `written`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartSummary {
    pub part: usize,
    pub name: String,
    pub faces: usize,
    pub instances: usize,
    /// Each instance's placement, rows of `[R | t]`, every value rounded to 9 decimals.
    pub placements: Vec<Placement>,
}

/// Why a file or part could not be loaded. The messages are specify-core's where it has one.
#[derive(Debug)]
pub enum LoadError {
    /// The file could not be read or parsed as STEP (Python: `cannot read STEP file {path}`);
    /// the reason follows.
    Unreadable { path: PathBuf, reason: String },
    /// The file holds no shape with faces.
    NoFaces { path: PathBuf },
    /// haecceity refused the file's geometry or structure, for the reason given.
    Step { path: PathBuf, reason: String },
    /// A file of several parts was loaded without saying which; the parts' names, in order.
    Assembly { path: PathBuf, names: Vec<String> },
    /// The part asked for is not one of the file's `count`.
    NoSuchPart {
        path: PathBuf,
        count: usize,
        part: usize,
    },
    /// The part could not be taken out of the file's assembly (the U1 stand-in).
    Extract {
        path: PathBuf,
        part: usize,
        name: String,
        reason: String,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Unreadable { path, reason } => {
                write!(f, "cannot read STEP file {}: {reason}", path.display())
            }
            LoadError::NoFaces { path } => write!(f, "{}: no shape with faces", path.display()),
            LoadError::Step { path, reason } => write!(f, "{}: {reason}", path.display()),
            LoadError::Assembly { path, names } => {
                let names: Vec<String> = names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| format!("{i}: {name}"))
                    .collect();
                write!(
                    f,
                    "{} is an assembly of {} parts; choose one ({})",
                    path.display(),
                    names.len(),
                    names.join(", ")
                )
            }
            LoadError::NoSuchPart { path, count, part } => write!(
                f,
                "{} has {count} part(s); there is no part {part}",
                path.display()
            ),
            LoadError::Extract {
                path,
                part,
                name,
                reason,
            } => write!(
                f,
                "{}: cannot take part {part} ({name}) out of the file: {reason}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// Load `path` and take its `part`-th part; a file of several parts must say which (Python's
/// `load`). Only the part asked for is built.
pub fn load(path: &Path, part: Option<usize>) -> Result<LoadedPart, LoadError> {
    let file = File::read(path)?;
    let index = match part {
        Some(index) => index,
        None if file.definitions.len() > 1 => {
            return Err(LoadError::Assembly {
                path: path.to_path_buf(),
                names: file.names,
            });
        }
        None => 0,
    };
    if index >= file.definitions.len() {
        return Err(LoadError::NoSuchPart {
            path: path.to_path_buf(),
            count: file.definitions.len(),
            part: index,
        });
    }
    file.load(index)
}

/// Every part of `path`, loaded, in [`parts`]' order (Python's `load_all`).
pub fn load_all(path: &Path) -> Result<Vec<LoadedPart>, LoadError> {
    let file = File::read(path)?;
    (0..file.definitions.len())
        .map(|index| file.load(index))
        .collect()
}

/// The distinct parts of `path` (Python's `parts`): each one's name, face count and where each
/// of its instances is placed. A part placed twice is one part with two placements. No geometry
/// is built.
///
/// The parts are haecceity's: one per product definition whose shape has faces, in
/// specify-core's order. Where OpenCascade's XCAF makes a part of each shape representation of
/// one product (a solid with surface models beside it, as in NIST CTC-02, 04 and 05), this lists
/// the product once with all its faces, the solid's first.
pub fn parts(path: &Path) -> Result<Vec<PartSummary>, LoadError> {
    let file = File::read(path)?;
    Ok(file
        .definitions
        .iter()
        .zip(&file.names)
        .enumerate()
        .map(|(index, (definition, name))| PartSummary {
            part: index,
            name: name.clone(),
            faces: definition.faces.len(),
            instances: definition.placements.len(),
            placements: definition
                .placements
                .iter()
                .map(|p| rounded(&p.placement))
                .collect(),
        })
        .collect())
}

/// A file read once: its bytes, their hash, its parts and their names.
struct File {
    path: PathBuf,
    bytes: Arc<Vec<u8>>,
    sha256: String,
    definitions: Vec<PartDefinition>,
    names: Vec<String>,
}

impl File {
    fn read(path: &Path) -> Result<File, LoadError> {
        let unreadable = |reason: String| LoadError::Unreadable {
            path: path.to_path_buf(),
            reason,
        };
        let bytes = std::fs::read(path).map_err(|e| unreadable(e.to_string()))?;
        // specify-core reads STEP text only; a gzipped file is refused as unreadable, not
        // decompressed behind the caller's back.
        if bytes.starts_with(&[0x1f, 0x8b]) {
            return Err(unreadable(
                "the file is gzip-compressed; decompress it first".into(),
            ));
        }
        let definitions = step::read_part_definitions(&bytes).map_err(|e| step_error(path, e))?;
        if definitions.is_empty() {
            return Err(LoadError::NoFaces {
                path: path.to_path_buf(),
            });
        }
        let names = part_names(&bytes, &definitions).map_err(|e| step_error(path, e))?;
        let sha256 = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok(File {
            path: path.to_path_buf(),
            bytes: Arc::new(bytes),
            sha256,
            definitions,
            names,
        })
    }

    /// Builds part `index`'s [`LoadedPart`].
    fn load(&self, index: usize) -> Result<LoadedPart, LoadError> {
        let definition = &self.definitions[index];
        let extract = |reason: String| LoadError::Extract {
            path: self.path.clone(),
            part: index,
            name: self.names[index].clone(),
            reason,
        };
        let Some(first) = definition.placements.first() else {
            return Err(extract("the part has no placement".into()));
        };
        // The file read as if its roots were placed by the inverse of this part's first
        // placement, so this part's first instance lands in the part's own frame. For a part at
        // the origin that is the plain read.
        let read = if first.placement == step::IDENTITY {
            step::read_step(&self.bytes)
        } else {
            step::read_step_placed(&self.bytes, &inverse(&first.placement))
        }
        .map_err(|e| extract(e.to_string()))?;
        let (part, rebuilt, unresolved_faces, unresolved_edges) =
            if same_faces(&read, &definition.faces) {
                // The read holds this part alone, faces already in the definition's order: use
                // haecceity's own `Part`, sources and unresolved lists intact.
                let faces = read.unresolved_faces().to_vec();
                let edges = read.unresolved_edges().to_vec();
                (read, false, faces, edges)
            } else {
                let extracted =
                    extract_part(&read, definition, &first.instances).map_err(extract)?;
                (
                    extracted.part,
                    true,
                    extracted.unresolved_faces,
                    extracted.unresolved_edges,
                )
            };
        Ok(LoadedPart {
            path: self.path.clone(),
            bytes: Arc::clone(&self.bytes),
            binding: Binding {
                source_sha256: self.sha256.clone(),
                face_count: definition.faces.len(),
                loader_version: LOADER_VERSION,
                part: index,
            },
            part,
            definition: definition.clone(),
            part_count: self.definitions.len(),
            name: self.names[index].clone(),
            placements: definition.placements.iter().map(|p| p.placement).collect(),
            rebuilt,
            unresolved_faces,
            unresolved_edges,
        })
    }
}

/// A haecceity refusal as a [`LoadError`]: a file that does not parse is unreadable (Python's
/// `cannot read STEP file`), a file with no shape with faces is [`LoadError::NoFaces`], anything
/// else is haecceity's own reason.
fn step_error(path: &Path, error: StepError) -> LoadError {
    let path = path.to_path_buf();
    match error {
        StepError::Parse(_) => LoadError::Unreadable {
            path,
            reason: error.to_string(),
        },
        // read_step's refusal of a file with no faces at all (step.rs, `read_placed`).
        StepError::Incomplete(ref why) if why == "no solid or shell with faces" => {
            LoadError::NoFaces { path }
        }
        _ => LoadError::Step {
            path,
            reason: error.to_string(),
        },
    }
}

/// Whether `part`'s faces are exactly `faces` (by the `#N` each was read from), in order.
fn same_faces(part: &Part, faces: &[u64]) -> bool {
    part.faces.len() == faces.len()
        && faces
            .iter()
            .enumerate()
            .all(|(i, &entity)| part.face_source(i).is_some_and(|s| s.entity == entity))
}

/// The inverse of a rigid placement: `[Rᵀ | -Rᵀt]`.
fn inverse(p: &Placement) -> Placement {
    let mut out = [[0.0; 4]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = p[j][i];
        }
        out[i][3] = -(0..3).map(|k| p[k][i] * p[k][3]).sum::<f64>();
    }
    out
}

/// A placement with every value rounded to 9 decimals, as Python's `_matrix` does
/// (`round(x, 9)`): Rust's `{:.9}` rounds the exact binary value correctly, as Python's `round`
/// does, so the two agree.
fn rounded(p: &Placement) -> Placement {
    p.map(|row| row.map(|x| format!("{x:.9}").parse().unwrap_or(x)))
}

/// A part taken out of a read of the whole file.
struct Extracted {
    part: Part,
    unresolved_faces: Vec<usize>,
    unresolved_edges: Vec<usize>,
}

/// The U1 stand-in: the faces of `read` that belong to `instances` (one placement of the part),
/// in `definition.faces` order, rebuilt into a `Part` of their own with edges, loops, pcurves and
/// solids re-indexed, and `read`'s unresolved faces and edges carried over in the new numbering
/// (those of other instances or parts dropped). Refused if a face of the definition is not found
/// exactly once among the instances' faces, or a solid of the instances holds a face the
/// definition lacks.
fn extract_part(
    read: &Part,
    definition: &PartDefinition,
    instances: &[usize],
) -> Result<Extracted, String> {
    let ours = |source: Option<haecceity::brep::Source>| {
        source.is_some_and(|s| instances.contains(&s.instance))
    };
    // Face #N → the read's faces of these instances read from it.
    let mut by_entity: HashMap<u64, Vec<usize>> = HashMap::new();
    for f in 0..read.faces.len() {
        if let Some(s) = read
            .face_source(f)
            .filter(|s| instances.contains(&s.instance))
        {
            by_entity.entry(s.entity).or_default().push(f);
        }
    }
    let mut old_faces = Vec::with_capacity(definition.faces.len());
    for (index, entity) in definition.faces.iter().enumerate() {
        match by_entity.get(entity).map(Vec::as_slice) {
            Some(&[f]) => old_faces.push(f),
            found => {
                return Err(format!(
                    "face {index} (#{entity}) is found {} times in its instance, not once",
                    found.map_or(0, <[usize]>::len)
                ));
            }
        }
    }
    let new_face: HashMap<usize, usize> = old_faces
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect();
    if let Some(stray) =
        (0..read.faces.len()).find(|f| ours(read.face_source(*f)) && !new_face.contains_key(f))
    {
        return Err(format!(
            "face #{} of the part's instance is not one of the part's faces",
            read.face_source(stray).map_or(0, |s| s.entity)
        ));
    }

    // Solids in the read's order, each holding the new indices of its faces.
    let mut new_solid: BTreeMap<usize, usize> = BTreeMap::new();
    for &old in &old_faces {
        if let Some(s) = read.faces[old].solid {
            new_solid.insert(s, 0);
        }
    }
    for (new, slot) in new_solid.values_mut().enumerate() {
        *slot = new;
    }
    let mut solids = Vec::with_capacity(new_solid.len());
    for &old in new_solid.keys() {
        let faces = read.solids[old]
            .faces
            .iter()
            .map(|f| {
                new_face.get(f).copied().ok_or_else(|| {
                    format!("solid {old} of the read holds face {f}, which is not the part's")
                })
            })
            .collect::<Result<Vec<usize>, String>>()?;
        solids.push(Solid { faces });
    }

    // Edges in the order the part's faces' loops first use them.
    let mut new_edge: HashMap<usize, usize> = HashMap::new();
    let mut edges = Vec::new();
    for &old in &old_faces {
        for &(e, _) in read.faces[old].loops.iter().flat_map(|l| &l.edges) {
            new_edge.entry(e).or_insert_with(|| {
                edges.push(read.edges[e].clone());
                edges.len() - 1
            });
        }
    }
    let faces = old_faces
        .iter()
        .map(|&old| {
            let face = &read.faces[old];
            let pcurves = face
                .pcurves
                .iter()
                .map(|(e, pc)| {
                    new_edge.get(e).map(|&n| (n, pc.clone())).ok_or_else(|| {
                        format!("face {old} has a pcurve for edge {e} it does not use")
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(Face {
                surface: face.surface.clone(),
                reversed: face.reversed,
                loops: face
                    .loops
                    .iter()
                    .map(|l| Loop {
                        edges: l.edges.iter().map(|&(e, d)| (new_edge[&e], d)).collect(),
                        vertex: l.vertex,
                    })
                    .collect(),
                solid: face.solid.map(|s| new_solid[&s]),
                pcurves,
            })
        })
        .collect::<Result<Vec<Face>, String>>()?;

    // The read's unresolved faces and edges in the new numbering; those of other instances or
    // parts are not this part's and are dropped. One of this part's instances that has no new
    // index would be lost silently, so it is refused.
    let mut unresolved_faces = Vec::new();
    for &f in read.unresolved_faces() {
        match new_face.get(&f) {
            Some(&n) => unresolved_faces.push(n),
            None if ours(read.face_source(f)) => {
                return Err(format!(
                    "unresolved face {f} of the part has no place in it"
                ));
            }
            None => {}
        }
    }
    let mut unresolved_edges = Vec::new();
    for &e in read.unresolved_edges() {
        match new_edge.get(&e) {
            Some(&n) => unresolved_edges.push(n),
            None if ours(read.edge_source(e)) => {
                return Err(format!(
                    "unresolved edge {e} of the part has no place in it"
                ));
            }
            None => {}
        }
    }
    unresolved_faces.sort_unstable();
    unresolved_edges.sort_unstable();
    Ok(Extracted {
        part: Part::new(faces, edges, solids),
        unresolved_faces,
        unresolved_edges,
    })
}

/// Each part's name (Python's `_part_name`): that of an assembly holding the part alone (an
/// exporter may wrap a named part round a solid it calls SOLID), else the part's own product
/// name (its id if the name is empty). Assemblies are the product definitions
/// `NEXT_ASSEMBLY_USAGE_OCCURRENCE`s relate, visited depth first from the roots as Python visits
/// XCAF's assembly labels, components in file order; the first assembly that holds the part as
/// its only component and has a name wins.
///
/// XCAF also makes components of a product's several shape representations (NIST files); those
/// are not assemblies here, so where Python names such a part by OpenCascade's label for its
/// shape (`SOLID`), this names it by its product.
fn part_names(bytes: &[u8], definitions: &[PartDefinition]) -> Result<Vec<String>, StepError> {
    let doc = Document::parse(bytes.to_vec()).map_err(|e| StepError::Parse(e.to_string()))?;
    let graph = doc.graph();
    let reference = |a: Option<&Attribute>| match a {
        Some(Attribute::EntityRef(n)) => Some(*n),
        _ => None,
    };
    // Assembly → its components' product definitions, in file order (BTreeMap: ids ascending).
    let mut components: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut related = HashSet::new();
    for entity in graph.entities.values() {
        if let RawEntity::Simple {
            name, attributes, ..
        } = entity
            && name == "NEXT_ASSEMBLY_USAGE_OCCURRENCE"
            && let (Some(relating), Some(child)) =
                (reference(attributes.get(3)), reference(attributes.get(4)))
        {
            components.entry(relating).or_default().push(child);
            related.insert(child);
        }
    }
    // A product definition's name, as OpenCascade names its label: PRODUCT_DEFINITION.formation
    // → PRODUCT_DEFINITION_FORMATION.of_product → PRODUCT.name, or PRODUCT.id when the name is
    // empty (NIST FTC-08 and CTC-04 write their title as the id and an empty name, and Python
    // reports the id).
    let attribute = |id: u64, index: usize| match graph.entities.get(&id) {
        Some(RawEntity::Simple { attributes, .. }) => attributes.get(index).cloned(),
        _ => None,
    };
    let product_name = |definition: u64| -> String {
        let product = reference(attribute(definition, 2).as_ref())
            .and_then(|formation| reference(attribute(formation, 2).as_ref()));
        let string = |index: usize| match product.and_then(|p| attribute(p, index)) {
            Some(Attribute::String(raw)) => text(&raw),
            _ => String::new(),
        };
        let name = string(1);
        if name.is_empty() { string(0) } else { name }
    };
    let mut assemblies = Vec::new();
    let mut seen = HashSet::new();
    let mut stack: Vec<u64> = components
        .keys()
        .copied()
        .filter(|k| !related.contains(k))
        .rev()
        .collect();
    while let Some(definition) = stack.pop() {
        let Some(children) = components.get(&definition) else {
            continue;
        };
        if !seen.insert(definition) {
            continue;
        }
        assemblies.push(definition);
        stack.extend(children.iter().rev());
    }
    Ok(definitions
        .iter()
        .map(|d| {
            assemblies
                .iter()
                .find(|a| components[a] == [d.product_definition] && !product_name(**a).is_empty())
                .map_or_else(|| product_name(d.product_definition), |&a| product_name(a))
        })
        .collect())
}

/// A STEP string's text with its escapes resolved, as OpenCascade gives names; the file's own
/// text if an escape cannot be resolved (a name is never refused for that).
fn text(raw: &str) -> String {
    haecceity::p21::decode(raw).unwrap_or_else(|_| raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_undoes_a_placement() {
        let (c, s) = (0.6_f64, 0.8_f64);
        let p: Placement = [[c, -s, 0.0, 10.0], [s, c, 0.0, -3.0], [0.0, 0.0, 1.0, 5.0]];
        let q = inverse(&p);
        let x = [1.0, 2.0, 3.0];
        let apply = |m: &Placement, v: [f64; 3]| {
            [0, 1, 2].map(|i| (0..3).map(|k| m[i][k] * v[k]).sum::<f64>() + m[i][3])
        };
        let back = apply(&q, apply(&p, x));
        for i in 0..3 {
            assert!((back[i] - x[i]).abs() < 1e-12);
        }
    }

    #[test]
    fn placements_round_to_nine_decimals() {
        let p: Placement = [
            [1.0, 2e-17, 0.0, -30.0000000004],
            [0.0, 1.0, 0.0, 0.1234567896],
            [0.0, 0.0, 1.0, -1e-12],
        ];
        let r = rounded(&p);
        assert_eq!(r[0], [1.0, 0.0, 0.0, -30.0]);
        assert_eq!(r[1][3], 0.12345679);
        assert!(
            r[2][3] == 0.0 && r[2][3].is_sign_negative(),
            "Python: round(-1e-12, 9) is -0.0"
        );
    }
}
