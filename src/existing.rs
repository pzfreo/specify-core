//! Semantic PMI already in the file, keyed by face index (port of `existing.py`).
//!
//! The interview must never ask for something the file already states, so each dimension,
//! geometric tolerance and datum is resolved to the faces it references. The PMI is
//! haecceity's ([`haecceity::pmi::read`]): it reads each part's PMI semantically, anchored to
//! the part's own face numbering, so PMI of other parts and of the assembly is never this
//! part's, and no text scan is needed for what OpenCascade loses.
//!
//! What differs from Python, by design:
//!
//! - **Values in millimetres, as stated.** Python reports a dimension as OpenCascade gives it,
//!   in the file's own unit (an inch NIST file's nominal stays in inches); here lengths are
//!   millimetres, the unit specify-core works in, and angles degrees (as Python reads them).
//!   Deviations keep their stated sign (Python's reader loses the lower deviation's sign and
//!   garbles limits below nominal).
//! - **One datum per letter and set of faces**, including datums no tolerance references
//!   (OpenCascade's reader builds datums only through the tolerances that cite them).
//! - **No OpenCascade artefacts.** Presentation read as a dimension (`DimensionPresentation`,
//!   specify-core's own thread callouts) is not reported; it states no size.
//!
//! The type names are OpenCascade's (`XCAFDimTolObjects_DimensionType` and
//! `XCAFDimTolObjects_GeomToleranceType` without their prefixes), because they are part of the
//! analysis JSON specify-core's rules read (`Size_*`, `Location*`, `Position`, ...).
//!
//! Notes: threads and knurls haecceity reads semantically (`thread`, `turned_knurl`), and the
//! notes specify-core (Python) writes by its own text route, which haecceity does not report as
//! anchored items (a `SHAPE_ASPECT` named for its kind and its `GEOMETRIC_ITEM_SPECIFIC_USAGE`s,
//! linked to nothing semantic): those are read from the Part 21 document as Python reads them
//! (upstream need U10, a documented stand-in).

use std::collections::BTreeSet;
use std::fmt;

use haecceity::p21::{Attribute, Document, RawEntity};
use haecceity::pmi::{
    self, Anchor, AngleUnit, DimTolerance, DimensionKind, Feature, FeatureId, Finding,
    GeneralTolerance, LengthUnit, LocationKind, PartPmi, Quantity, SizeKind, TargetShape,
    ToleranceKind, ToleranceTarget, Value as PmiValue,
};
use haecceity::step;
use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::{Map, Value};

use crate::part::LoadedPart;
use crate::provenance;
use crate::pyfmt::round_half_even;

/// The notes specify-core writes on faces by its text route, by the name of their
/// `GEOMETRIC_ITEM_SPECIFIC_USAGE` (Python's `existing._NOTE`).
pub const NOTE_KINDS: [&str; 5] = [
    "internal thread",
    "external thread",
    "knurl",
    "surface finish",
    "surface texture",
];

/// The part's notes by the key `part_settings` reports them under: Python's four
/// `requirements.PART_NOTES`, plus ordered free-text `notes` (draftwright-rust #123).
pub const PART_NOTES: [(&str, &str); 5] = [
    ("surface_finish", "surface texture"),
    ("coating", "surface treatment"),
    ("heat_treatment", "heat treatment"),
    ("edges", "edge condition"),
    ("notes", "general note"),
];

/// One item of PMI the file states (Python's `ExistingPmi`).
#[derive(Clone, Debug, PartialEq)]
pub struct ExistingPmi {
    /// `dimension`, `geometric_tolerance`, `datum` or `note`.
    pub kind: String,
    /// OpenCascade's type name without its prefix (`Size_Diameter`, `Position`), the datum's
    /// letter, or the note's kind.
    pub r#type: String,
    /// The faces it is on, ascending.
    pub faces: Vec<usize>,
    /// The second reference set of a location dimension.
    pub faces2: Vec<usize>,
    /// Nominal (mm or degrees) of a dimension, magnitude (mm) of a tolerance.
    pub value: Option<f64>,
    /// Signed deviations from nominal, mm (degrees for an angle).
    pub upper: Option<f64>,
    pub lower: Option<f64>,
    /// Absolute lower/upper bounds, only for a limit dimension. Deviations above
    /// remain available to tolerance checks and older consumers.
    pub limits: Option<[f64; 2]>,
    /// A basic (theoretically exact) dimension: no tolerance of its own.
    pub basic: bool,
    /// The datum letters a tolerance cites, primary first.
    pub datums: Vec<String>,
    /// STEP authorship token on this requirement; absent for unmarked input PMI.
    pub provenance: Option<String>,
    /// Stable inspection number read from this requirement's AP242 id, when present.
    pub number: Option<u64>,
    pub origin: Option<crate::pmi_origin::Origin>,
    pub source: Option<crate::pmi_origin::Source>,
    /// On reopening a file Specify wrote, the answered question whose stored answer this
    /// item contradicts (ADR P9): answering that question gives it way.
    pub question: Option<String>,
    /// The item as a drawing states it, in full: `⌀3.3 H11`, `⌖ ⌀0.3Ⓜ A`, `M2×0.4-6H ↧ 2`,
    /// `datum A` (ADR P11: the stated row is all anyone sees of what the file says, so nothing
    /// it states is left out of this text).
    pub text: String,
    /// The item states something [`Self::fields`] and the common fields do not hold (a datum
    /// reference's modifier, a common datum, a projected zone, a dimension's qualifier ...):
    /// `text` is the whole of it, and it cannot be changed field by field.
    pub text_only: bool,
    /// What the item states beyond the common fields, by the planner's names where it has them
    /// (docs/answers.md, "What the file states"): a size's `fit` (its ISO 286 class); a
    /// tolerance's `diametral`, `mmc`, `zone` (its zone form) and `modifiers` (as stated); a
    /// thread's `side`, `designation`, `spec`, `pitch`, `class`, `hand`, `through`,
    /// `drill_diameter`, `drill_depth`, `full_thread`, `length`; a knurl's `pattern`, `pitch`,
    /// `diameter`, `teeth`, `tooth_depth`, `root_fillet`, `helix_angle`, `hand`; a finish on
    /// faces' `finish`. Serialised beside the common fields.
    pub fields: Map<String, Value>,
}

impl ExistingPmi {
    fn new(kind: &str, r#type: &str, faces: Vec<usize>) -> ExistingPmi {
        ExistingPmi {
            kind: kind.into(),
            r#type: r#type.into(),
            faces,
            faces2: Vec::new(),
            value: None,
            upper: None,
            lower: None,
            limits: None,
            basic: false,
            datums: Vec::new(),
            provenance: None,
            number: None,
            origin: None,
            source: None,
            question: None,
            text: String::new(),
            text_only: false,
            fields: Map::new(),
        }
    }
}

/// As `ExistingPmi.to_dict`: empty and absent fields are left out, but `faces` is always there
/// (a datum the file attaches to no face still has faces: none), appended last when empty as
/// Python's `setdefault` does.
impl Serialize for ExistingPmi {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("kind", &self.kind)?;
        map.serialize_entry("type", &self.r#type)?;
        if !self.faces.is_empty() {
            map.serialize_entry("faces", &self.faces)?;
        }
        if !self.faces2.is_empty() {
            map.serialize_entry("faces2", &self.faces2)?;
        }
        for (key, value) in [
            ("value", self.value),
            ("upper", self.upper),
            ("lower", self.lower),
        ] {
            if let Some(value) = value {
                map.serialize_entry(key, &value)?;
            }
        }
        if let Some(limits) = self.limits {
            map.serialize_entry("limits", &limits)?;
        }
        if self.basic {
            map.serialize_entry("basic", &true)?;
        }
        if !self.datums.is_empty() {
            map.serialize_entry("datums", &self.datums)?;
        }
        if let Some(provenance) = &self.provenance {
            map.serialize_entry("provenance", provenance)?;
        }
        if let Some(number) = self.number {
            map.serialize_entry("number", &number)?;
        }
        if let Some(origin) = &self.origin {
            map.serialize_entry("origin", origin)?;
        }
        if let Some(source) = &self.source {
            map.serialize_entry("source", source)?;
        }
        if let Some(question) = &self.question {
            map.serialize_entry("question", question)?;
        }
        if !self.text.is_empty() {
            map.serialize_entry("text", &self.text)?;
        }
        if self.text_only {
            map.serialize_entry("text_only", &true)?;
        }
        for (key, value) in &self.fields {
            map.serialize_entry(key, value)?;
        }
        if self.faces.is_empty() {
            map.serialize_entry("faces", &self.faces)?;
        }
        map.end()
    }
}

/// Why a file's PMI could not be read.
#[derive(Clone, Debug, PartialEq)]
pub enum ExistingError {
    /// The file does not parse as Part 21, or its parts cannot be listed.
    Unreadable(String),
    /// haecceity's PMI reader refused the file.
    Pmi(String),
}

impl fmt::Display for ExistingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExistingError::Unreadable(why) => write!(f, "cannot read the file's PMI: {why}"),
            ExistingError::Pmi(why) => write!(f, "cannot read the file's PMI: {why}"),
        }
    }
}

impl std::error::Error for ExistingError {}

/// Which of a reading's items to leave out of [`Reading::existing_except`], and which to tag
/// with a question, by haecceity collection and index (`("dimensions", 2)`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Selection {
    pub skip: BTreeSet<(String, usize)>,
    pub questions: std::collections::BTreeMap<(String, usize), String>,
    pub retained: BTreeSet<(String, usize)>,
}

impl Selection {
    fn skips(&self, collection: &str, index: usize) -> bool {
        self.skip.contains(&(collection.to_string(), index))
    }

    fn question(&self, collection: &str, index: usize) -> Option<String> {
        self.questions
            .get(&(collection.to_string(), index))
            .cloned()
    }
}

/// The part's PMI as haecceity reads it, the findings that concern it (its own and those of
/// no part), and the parsed document: the file is parsed and read once, and
/// [`Reading::existing`], [`Reading::settings`] and [`Reading::findings`] all answer from it
/// (the free functions below are each one reading and one answer).
pub struct Reading {
    doc: Document,
    pmi: PartPmi,
    provenance: pmi::PartProvenance,
    findings: Vec<Finding>,
}

/// Reads the part's PMI (see [`Reading`]).
pub fn read(loaded: &LoadedPart) -> Result<Reading, ExistingError> {
    let bytes = loaded.bytes.as_ref();
    let doc =
        Document::parse(bytes.clone()).map_err(|e| ExistingError::Unreadable(e.to_string()))?;
    // The reader takes every part of the file, in the order `binding.part` indexes.
    let parts =
        step::read_part_definitions(bytes).map_err(|e| ExistingError::Unreadable(e.to_string()))?;
    read_parsed(loaded, doc, &parts)
}

/// [`read`] from the file already parsed: `doc`, `loaded.bytes` as a Part 21 document, and
/// `parts`, every part of the file in the order `binding.part` indexes (a [`crate::part::File`]'s
/// definitions). Neither is checked against the bytes; a caller holding both saves parsing the
/// file and listing its parts again.
pub fn read_parsed(
    loaded: &LoadedPart,
    doc: Document,
    parts: &[step::PartDefinition],
) -> Result<Reading, ExistingError> {
    let mut read = pmi::read(&doc, parts).map_err(|e| ExistingError::Pmi(e.to_string()))?;
    let index = loaded.binding.part;
    if index >= read.parts.len() {
        return Err(ExistingError::Pmi(format!(
            "the reader has {} parts; there is no part {index}",
            read.parts.len()
        )));
    }
    let findings = read
        .findings
        .into_iter()
        .filter(|f| f.part.is_none_or(|p| p.0 == index))
        .collect();
    Ok(Reading {
        pmi: read.parts.swap_remove(index),
        provenance: read.provenance.parts.swap_remove(index),
        findings,
        doc,
    })
}

/// The semantic PMI the file states on the part (Python's `read_existing`): dimensions, then
/// geometric tolerances, then datums, then notes on faces, each group in haecceity's order.
pub fn read_existing(loaded: &LoadedPart) -> Result<Vec<ExistingPmi>, ExistingError> {
    read(loaded)?.existing(loaded)
}

/// What haecceity reported while reading the file that concerns this part (its findings and
/// those of no part): Python's OpenCascade reader reports nothing of the kind, so the sweeps
/// record each finding kind they meet.
pub fn findings(loaded: &LoadedPart) -> Result<Vec<Finding>, ExistingError> {
    Ok(read(loaded)?.findings)
}

/// The material and general tolerance the file states, and the part's notes (see
/// [`Reading::settings`]).
pub fn part_settings(loaded: &LoadedPart) -> Result<Map<String, Value>, ExistingError> {
    Ok(read(loaded)?.settings(loaded))
}

/// Authorship of the values returned by [`part_settings`], without changing their shape.
/// Scalar settings map to the token; `notes` maps to an aligned array of tokens or nulls.
/// Unmarked settings are omitted. See `docs/pmi-provenance.md`.
pub fn part_settings_provenance(loaded: &LoadedPart) -> Result<Map<String, Value>, ExistingError> {
    Ok(read(loaded)?.settings_provenance(loaded))
}

impl Reading {
    /// The part's PMI as read.
    pub fn model(&self) -> &PartPmi {
        &self.pmi
    }

    /// The part's process in the file, including its authorship.
    pub(crate) fn process(&self) -> Option<crate::bom::Field> {
        crate::process::file_pmi(&self.doc, &self.pmi, &self.provenance).map(|(_, field)| field)
    }

    /// The semantic PMI the file states on the part, as [`read_existing`]; `loaded` is the part
    /// this reading is of. A tolerance citing a datum the reader does not hold is refused, not
    /// listed without it.
    pub fn existing(&self, loaded: &LoadedPart) -> Result<Vec<ExistingPmi>, ExistingError> {
        self.existing_except(loaded, &Selection::default())
    }

    /// Every requirement the file states (ADR P11: whoever wrote it), including the
    /// dimensions Specify wrote under the general tolerance, which [`Self::existing`] leaves
    /// out.
    pub(crate) fn existing_all(
        &self,
        loaded: &LoadedPart,
    ) -> Result<Vec<ExistingPmi>, ExistingError> {
        let selection = Selection {
            retained: (0..self.pmi.dimensions.len())
                .map(|i| ("dimensions".to_string(), i))
                .collect(),
            ..Selection::default()
        };
        self.existing_except(loaded, &selection)
    }

    /// [`Self::existing`] without the items `selection` leaves out, and with the question it
    /// gives each tagged item (ADR P9: a reopened file's PMI less what its answers explain).
    pub(crate) fn existing_except(
        &self,
        loaded: &LoadedPart,
        selection: &Selection,
    ) -> Result<Vec<ExistingPmi>, ExistingError> {
        let Reading {
            doc,
            pmi,
            provenance: prov,
            ..
        } = self;
        let mut out = Vec::new();

        for (i, dimension) in pmi.dimensions.iter().enumerate() {
            if selection.skips("dimensions", i) {
                continue;
            }
            let (r#type, first, second) = match &dimension.kind {
                DimensionKind::Size {
                    feature,
                    kind,
                    path,
                    angle,
                } => (
                    size_type(kind, path.is_some(), angle.is_some()),
                    faces_of(pmi, &[*feature]),
                    Vec::new(),
                ),
                DimensionKind::Location {
                    from,
                    to,
                    kind,
                    path,
                    angle,
                    ..
                } => (
                    location_type(kind, path.is_some(), angle.is_some()),
                    faces_of(pmi, &[*from]),
                    faces_of(pmi, &[*to]),
                ),
            };
            let owned = provenance::dimension(doc, &prov.dimensions[i]);
            // One Specify wrote with no tolerance of its own is a dimension the general
            // tolerance covers (#63): it states nothing the geometry does not, so it settles no
            // question, read back without the answers it was written from.
            if owned
                && matches!(dimension.tolerance, DimTolerance::None)
                && !selection.retained.contains(&("dimensions".into(), i))
            {
                continue;
            }
            let mut item = ExistingPmi::new("dimension", r#type, first);
            item.provenance = owned.then(|| provenance::MARKER.into());
            let metadata =
                crate::pmi_origin::read(pmi, pmi::NoteOwner::Dimension(pmi::DimensionId(i)));
            item.origin = metadata.origin;
            item.source = metadata.source;
            item.number = crate::characteristics::on(
                doc,
                &prov.dimensions[i],
                if matches!(dimension.kind, DimensionKind::Size { .. }) {
                    "dimensional_size"
                } else {
                    "dimensional_location"
                },
            );
            item.faces2 = second;
            item.question = selection.question("dimensions", i);
            let nominal = dimension.nominal.as_ref();
            item.value = nominal.map(value);
            // Preserve absolute limits, and derive offsets for existing tolerance checks.
            if let DimTolerance::Limits(b) = &dimension.tolerance {
                item.limits = Some([value(b.lower()), value(b.upper())]);
            }
            item.basic = matches!(dimension.tolerance, DimTolerance::Basic);
            let offsets = |upper: &PmiValue, lower: &PmiValue| match nominal {
                Some(n) => (
                    Some(round_half_even(value(upper) - value(n), 9)),
                    Some(round_half_even(value(lower) - value(n), 9)),
                ),
                None => (None, None),
            };
            (item.upper, item.lower) = match &dimension.tolerance {
                DimTolerance::Deviations(b) => (Some(value(b.upper())), Some(value(b.lower()))),
                DimTolerance::Limits(b) => offsets(b.upper(), b.lower()),
                DimTolerance::Fit {
                    limits: Some(b), ..
                } => offsets(b.upper(), b.lower()),
                // A class alone states its deviations through the ISO 286 tables.
                DimTolerance::Fit {
                    class,
                    limits: None,
                } => match (nominal, &dimension.kind) {
                    (Some(n), DimensionKind::Size { angle: None, .. }) => {
                        crate::standards::fit_limits(&class.to_string(), value(n))
                            .map_or((None, None), |(u, l)| (Some(u), Some(l)))
                    }
                    _ => (None, None),
                },
                DimTolerance::None | DimTolerance::Basic => (None, None),
            };
            if let DimTolerance::Fit { class, .. } = &dimension.tolerance {
                item.fields
                    .insert("fit".into(), Value::from(class.to_string()));
            }
            (item.text, item.text_only) = dimension_notation(dimension);
            out.push(item);
        }

        for (i, tolerance) in pmi.tolerances.iter().enumerate() {
            if selection.skips("tolerances", i) {
                continue;
            }
            let faces = match &tolerance.target {
                ToleranceTarget::Feature(f) => faces_of(pmi, &[*f]),
                ToleranceTarget::Dimension(d) => match pmi.dimensions.get(d.0).map(|d| &d.kind) {
                    Some(DimensionKind::Size { feature, .. }) => faces_of(pmi, &[*feature]),
                    Some(DimensionKind::Location { from, to, .. }) => faces_of(pmi, &[*from, *to]),
                    None => Vec::new(),
                },
                ToleranceTarget::Relation {
                    relating, related, ..
                } => faces_of(pmi, &[*relating, *related]),
                ToleranceTarget::WholePart => Vec::new(),
            };
            let mut item =
                ExistingPmi::new("geometric_tolerance", tolerance_type(tolerance.kind), faces);
            item.provenance = (tolerance.description.as_deref() == Some(provenance::MARKER))
                .then(|| provenance::MARKER.into());
            item.number =
                crate::characteristics::on(doc, &prov.tolerances[i], "geometric_tolerance");
            item.value = tolerance.magnitude.as_ref().map(value);
            item.question = selection.question("tolerances", i);
            if let Some(zone) = &tolerance.zone {
                if zone.form.is_diametral() {
                    item.fields.insert("diametral".into(), Value::Bool(true));
                }
                item.fields.insert("zone".into(), json_name(&zone.form));
            }
            if tolerance
                .modifiers
                .contains(&pmi::ToleranceModifier::MaximumMaterialRequirement)
            {
                item.fields.insert("mmc".into(), Value::Bool(true));
            }
            if !tolerance.modifiers.is_empty() {
                item.fields.insert(
                    "modifiers".into(),
                    Value::Array(tolerance.modifiers.iter().map(json_name).collect()),
                );
            }
            item.datums = tolerance
                .datums
                .iter()
                .flat_map(|system| system.compartments())
                .flat_map(|c| &c.references)
                .map(|r| {
                    pmi.datums
                        .get(r.datum.0)
                        .map(|d| d.label().as_str().to_string())
                        .ok_or_else(|| {
                            ExistingError::Pmi(format!(
                                "a {} tolerance cites datum {}, which the reader does not hold",
                                tolerance_type(tolerance.kind),
                                r.datum.0
                            ))
                        })
                })
                .collect::<Result<_, _>>()?;
            // After the datums: every one it cites is held.
            (item.text, item.text_only) = tolerance_notation(tolerance, pmi);
            out.push(item);
        }

        let mut seen = Vec::new();
        for (i, datum) in pmi.datums.iter().enumerate() {
            if selection.skips("datums", i) {
                continue;
            }
            // The datum feature's faces, and those of the features its targets are or lie on.
            let mut features: Vec<FeatureId> = datum.feature().into_iter().collect();
            for target in datum.targets() {
                if let Some(t) = pmi.datum_targets.get(target.0) {
                    if let TargetShape::Area(f) | TargetShape::Curve(f) = t.shape() {
                        features.push(*f);
                    }
                    features.extend(t.on());
                }
            }
            let owned = provenance::marked(doc, &prov.datums[i], "datum");
            let key = (
                datum.label().as_str().to_string(),
                faces_of(pmi, &features),
                owned,
            );
            if !seen.contains(&key) {
                let mut item = ExistingPmi::new("datum", &key.0, key.1.clone());
                item.text = format!("datum {}", key.0);
                item.provenance = owned.then(|| provenance::MARKER.into());
                item.number = crate::characteristics::on(doc, &prov.datums[i], "datum");
                item.question = selection.question("datums", i);
                out.push(item);
                seen.push(key);
            }
        }

        out.extend(notes(loaded, doc, pmi, prov, selection));
        Ok(out)
    }

    /// Authorship alongside [`Self::settings`]; values and selection order are unchanged.
    pub fn settings_provenance(&self, loaded: &LoadedPart) -> Map<String, Value> {
        let mut out = Map::new();
        let p = &self.pmi;
        let prov = &self.provenance;
        let owned = |ids: &[u64], kind: &str| provenance::marked(&self.doc, ids, kind);
        let mut notes = Vec::new();
        for (i, set) in p.attributes.iter().enumerate() {
            if set.on.is_some() || set.name != crate::writer::SEMANTIC_TEXT {
                continue;
            }
            let index = set.items.iter().find_map(|(name, v)| match v {
                pmi::AttributeValue::Integer(n) if name == crate::writer::NOTE_INDEX && *n > 0 => {
                    Some(*n)
                }
                _ => None,
            });
            for (name, v) in &set.items {
                if name == "general note" && matches!(v, pmi::AttributeValue::Text(_)) {
                    notes.push((index, owned(&prov.attributes[i], "attribute")));
                }
            }
        }
        notes.sort_by_key(|(index, _)| (index.is_none(), *index));
        if notes.iter().any(|(_, owned)| *owned) {
            out.insert(
                "notes".into(),
                Value::Array(
                    notes
                        .into_iter()
                        .map(|(_, yes)| {
                            if yes {
                                Value::from(provenance::MARKER)
                            } else {
                                Value::Null
                            }
                        })
                        .collect(),
                ),
            );
        }
        if loaded.part_count > 1 {
            return out;
        }
        if p.material.is_some() && owned(&prov.material, "material") {
            out.insert("material".into(), Value::from(provenance::MARKER));
        }
        if let Some(i) = p
            .general
            .iter()
            .position(|g| matches!(g, GeneralTolerance::Class { .. }))
        {
            if owned(&prov.general[i], "general") {
                out.insert("general_tolerance".into(), Value::from(provenance::MARKER));
            }
        } else if let Some(i) = p.attributes.iter().position(is_general_tolerance_note)
            && owned(&prov.attributes[i], "attribute")
        {
            out.insert("general_tolerance".into(), Value::from(provenance::MARKER));
        }
        // Legacy note routes have precedence, and carry no Specify marker.
        let mut selected = BTreeSet::new();
        for (key, kind) in PART_NOTES {
            if key == "notes" {
                continue;
            }
            if p.notes.iter().any(|n| {
                (n.kind == kind || (key == "surface_finish" && n.kind == "general surface texture"))
                    && (key != "surface_finish" || default_finish(&n.text).is_some())
            }) {
                selected.insert(key);
            }
        }
        for (i, texture) in p.surface_textures.iter().enumerate() {
            if texture.on.is_none()
                && finish_words(texture).is_some()
                && selected.insert("surface_finish")
                && owned(&prov.surface_textures[i], "surface texture")
            {
                out.insert("surface_finish".into(), Value::from(provenance::MARKER));
            }
        }
        for (i, set) in p.attributes.iter().enumerate() {
            if set.on.is_some() || set.name != crate::writer::SEMANTIC_TEXT {
                continue;
            }
            for (name, v) in &set.items {
                if !matches!(v, pmi::AttributeValue::Text(_)) {
                    continue;
                }
                if let Some((key, _)) = PART_NOTES.iter().find(|(key, kind)| {
                    *key != "surface_finish" && *key != "notes" && *kind == name
                }) && selected.insert(key)
                    && owned(&prov.attributes[i], "attribute")
                {
                    out.insert((*key).into(), Value::from(provenance::MARKER));
                }
            }
        }
        out
    }

    /// The findings of this reading (see [`findings`]).
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// The material and general tolerance the file states, and the part's notes (surface finish
    /// default, coating, heat treatment, edges, ordered free text). The four legacy settings
    /// stay empty for a part of an assembly, as Python's `part_settings`: its text scan cannot
    /// tell whose they are. Free-text notes are read by their semantic owner in all files.
    pub fn settings(&self, loaded: &LoadedPart) -> Map<String, Value> {
        let mut out = Map::new();
        self.each_setting(loaded.part_count > 1, |key, value| {
            out.entry(key).or_insert(value);
        });
        out
    }

    /// Visit the file's values in display precedence order, including additional
    /// entries that the scalar settings view cannot show.
    pub(crate) fn each_setting(&self, assembly_view: bool, mut visit: impl FnMut(&str, Value)) {
        // Unlike Python's legacy text scan, these are resolved to their owning part, also
        // in assemblies. Explicit indices carry order; unindexed notes follow in read order.
        let notes = general_notes(&self.pmi);
        if !notes.is_empty() {
            visit("notes", Value::from(notes));
        }
        if let Some(text) = self.pmi.notes.iter().find_map(|note| {
            (note.on.is_none()
                && note.kind == crate::writer::SEMANTIC_TEXT
                && edge_break_note(&note.text))
            .then_some(&note.text)
        }) {
            // A foreign part-level edge instruction answers the question as written.
            // Feature notes cannot establish a default for every edge of the part.
            visit("edges", Value::String(text.clone()));
        }
        if assembly_view {
            return;
        }
        let pmi = &self.pmi;
        if let Some(material) = &pmi.material {
            visit("material", Value::String(material.id.clone()));
        }
        for g in &pmi.general {
            if let GeneralTolerance::Class { text, .. } = g {
                visit("general_tolerance", Value::String(text.clone()));
            }
        }
        // Only Specify's own ± note supplies the general tolerance in this form.
        if let Some(text) = pmi
            .attributes
            .iter()
            .position(is_general_tolerance_note)
            .filter(|&i| provenance::marked(&self.doc, &self.provenance.attributes[i], "attribute"))
            .and_then(|_| general_tolerance_note(pmi))
        {
            visit("general_tolerance", Value::String(text));
        }
        for (key, kind) in PART_NOTES {
            if key == "notes" {
                continue; // Free-text part notes have only the semantic attribute form.
            }
            // The finish as specify-core writes it now, and as it was written before it took
            // Draftwright's form.
            let kinds: &[&str] = if key == "surface_finish" {
                &[kind, "general surface texture"]
            } else {
                &[kind]
            };
            for kind in kinds {
                for note in pmi.notes.iter().filter(|n| n.kind == *kind) {
                    let words = if key == "surface_finish" {
                        // Only the part's default; a finish on faces, written before it was
                        // 'surface finish', is not.
                        match default_finish(&note.text) {
                            Some(words) => words.to_string(),
                            None => continue,
                        }
                    } else {
                        note.text.clone()
                    };
                    visit(key, Value::String(words));
                }
            }
        }
        // The same notes in the standard forms the Rust writer writes (question Q3): the
        // default finish as a surface texture on the part, the rest as 'semantic text' on it,
        // its item named by the note's kind.
        for t in pmi.surface_textures.iter().filter(|t| t.on.is_none()) {
            if let Some(words) = finish_words(t) {
                visit("surface_finish", Value::String(words));
            }
        }
        for a in &pmi.attributes {
            if a.on.is_some() || a.name != crate::writer::SEMANTIC_TEXT {
                continue;
            }
            for (item, v) in &a.items {
                let key = PART_NOTES
                    .iter()
                    .find(|(key, kind)| *key != "surface_finish" && *key != "notes" && kind == item)
                    .map(|(key, _)| *key);
                if let (Some(key), pmi::AttributeValue::Text(words)) = (key, v) {
                    visit(key, Value::String(words.clone()));
                }
            }
        }
    }
}

/// Whether `set` is a ± general tolerance written as a note on the part (`docs/process.md`).
fn is_general_tolerance_note(set: &pmi::AttributeSet) -> bool {
    general_tolerance_text(set).is_some()
}

/// The text of the semantic part attribute used for a printed tolerance.
pub(crate) fn general_tolerance_text(set: &pmi::AttributeSet) -> Option<&str> {
    if set.on.is_some() || set.name != crate::writer::SEMANTIC_TEXT {
        return None;
    }
    set.items.iter().find_map(|(name, value)| match value {
        pmi::AttributeValue::Text(text) if name == crate::writer::GENERAL_TOLERANCE_NOTE => {
            Some(text.as_str())
        }
        _ => None,
    })
}

/// The part's ± general tolerance written as a note, if it has one.
pub(crate) fn general_tolerance_note(pmi: &PartPmi) -> Option<String> {
    pmi.attributes
        .iter()
        .find_map(general_tolerance_text)
        .map(str::to_owned)
}

/// Only explicit edge-breaking prose, not a general manufacturing-note classifier.
pub(crate) fn edge_break_note(text: &str) -> bool {
    let words: Vec<_> = text
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    words.iter().enumerate().any(|(i, word)| {
        if !matches!(word.as_str(), "break" | "breaking") {
            return false;
        }
        let rest = &words[i + 1..];
        let rest = rest.strip_prefix(&["all".into()]).unwrap_or(rest);
        let rest = rest.strip_prefix(&["sharp".into()]).unwrap_or(rest);
        rest.first()
            .is_some_and(|word| matches!(word.as_str(), "edge" | "edges"))
    })
}

/// Part-owned semantic text, PMI practice §7.4. Each note has its own set, a `general note`
/// text item and a one-based integer `note index`. Stable sorting keeps duplicate indices in
/// read order; missing/nonpositive indices follow indexed notes in read order. No legacy
/// note or drawing text is migrated into this list.
pub(crate) fn general_notes(pmi: &PartPmi) -> Vec<String> {
    let mut notes = Vec::new();
    for set in &pmi.attributes {
        if set.on.is_some() || set.name != crate::writer::SEMANTIC_TEXT {
            continue;
        }
        let index = set.items.iter().find_map(|(name, value)| match value {
            pmi::AttributeValue::Integer(i) if name == crate::writer::NOTE_INDEX && *i > 0 => {
                Some(*i)
            }
            _ => None,
        });
        for (name, value) in &set.items {
            if name == "general note"
                && let pmi::AttributeValue::Text(text) = value
            {
                notes.push((index, text.clone()));
            }
        }
    }
    notes.sort_by_key(|(index, _)| (index.is_none(), *index));
    notes.into_iter().map(|(_, text)| text).collect()
}

/// The notes on the part's faces (Python's `existing.notes`), one per kind with every face it
/// is on: specify-core's text route read from the document (in file order), then the threads
/// and knurls haecceity reads semantically.
fn notes(
    loaded: &LoadedPart,
    doc: &Document,
    pmi: &PartPmi,
    prov: &pmi::PartProvenance,
    selection: &Selection,
) -> Vec<ExistingPmi> {
    /// One note: its kind, authorship, number, question, what it states, and its faces.
    struct Found {
        kind: String,
        owned: bool,
        number: Option<u64>,
        question: Option<String>,
        said: Said,
        faces: BTreeSet<usize>,
    }
    let mut found: Vec<Found> = Vec::new();
    let mut add = |kind: &str,
                   faces: &[usize],
                   owned: bool,
                   number: Option<u64>,
                   item: Option<(&str, usize)>,
                   said: Said| {
        if faces.is_empty() || item.is_some_and(|(c, i)| selection.skips(c, i)) {
            return;
        }
        let question = item.and_then(|(c, i)| selection.question(c, i));
        // Notes of one kind that state the same are one item on all their faces; two threads
        // of other sizes stay two.
        match found.iter_mut().find(|f| {
            f.kind == kind
                && f.owned == owned
                && f.number == number
                && f.question == question
                && f.said == said
        }) {
            Some(f) => f.faces.extend(faces),
            None => found.push(Found {
                kind: kind.to_string(),
                owned,
                number,
                question,
                said,
                faces: faces.iter().copied().collect(),
            }),
        }
    };
    let graph = doc.graph();
    for entity in graph.entities.values() {
        let RawEntity::Simple {
            name, attributes, ..
        } = entity
        else {
            continue;
        };
        if name != "GEOMETRIC_ITEM_SPECIFIC_USAGE" {
            continue;
        }
        let (Some(Attribute::String(kind)), Some(Attribute::EntityRef(face))) =
            (attributes.first(), attributes.get(4))
        else {
            continue;
        };
        if NOTE_KINDS.contains(&kind.as_str())
            && let Some(index) = loaded.definition.face_index(*face)
        {
            // Python's text route: the note's kind is all the file links to the face.
            add(kind, &[index], false, None, None, Said::text(kind));
        }
    }
    for (i, thread) in pmi.threads.iter().enumerate() {
        let kind = match thread.side {
            pmi::ThreadSide::Internal => "internal thread",
            pmi::ThreadSide::External => "external thread",
        };
        add(
            kind,
            &faces_of(pmi, &[thread.feature]),
            provenance::marked(doc, &prov.threads[i], "thread"),
            crate::characteristics::on(doc, &prov.threads[i], "product_definition_shape"),
            Some(("threads", i)),
            thread_said(thread, pmi),
        );
    }
    for (i, knurl) in pmi.knurls.iter().enumerate() {
        add(
            "knurl",
            &faces_of(pmi, &[knurl.feature]),
            provenance::marked(doc, &prov.knurls[i], "knurl"),
            crate::characteristics::on(doc, &prov.knurls[i], "product_definition_shape"),
            Some(("knurls", i)),
            knurl_said(knurl),
        );
    }
    // A hole note as the Rust writer writes it (a heat-set insert or printed thread,
    // docs/process.md): semantic text on the hole, read back so the hole is not asked again.
    for (i, set) in pmi.attributes.iter().enumerate() {
        let Some(pmi::NoteOwner::Feature(f)) = set.on else {
            continue;
        };
        if set.name != crate::writer::SEMANTIC_TEXT {
            continue;
        }
        for (name, v) in &set.items {
            if let pmi::AttributeValue::Text(words) = v
                && name == crate::writer::HOLE_NOTE
            {
                add(
                    words,
                    &faces_of(pmi, &[f]),
                    provenance::marked(doc, &prov.attributes[i], "attribute"),
                    None,
                    Some(("attributes", i)),
                    Said::text(words),
                );
            }
        }
    }
    // A finish on faces as the Rust writer writes it: a surface texture on them (question Q3).
    for (i, t) in pmi.surface_textures.iter().enumerate() {
        if let Some(f) = t.on {
            add(
                "surface finish",
                &faces_of(pmi, &[f]),
                provenance::marked(doc, &prov.surface_textures[i], "surface texture"),
                crate::characteristics::texture_number(doc, &prov.surface_textures[i]),
                Some(("surface_textures", i)),
                finish_said(t),
            );
        }
    }
    found
        .into_iter()
        .map(|f| {
            let mut item = ExistingPmi::new("note", &f.kind, f.faces.into_iter().collect());
            item.provenance = f.owned.then(|| provenance::MARKER.into());
            item.number = f.number;
            item.question = f.question;
            item.text = f.said.text;
            item.text_only = f.said.text_only;
            item.fields = f.said.fields;
            item
        })
        .collect()
}

/// What a note states: its text in full, whether only the text holds all of it, and its
/// structured values (see [`ExistingPmi::fields`]).
#[derive(Clone, Debug, Default, PartialEq)]
struct Said {
    text: String,
    text_only: bool,
    fields: Map<String, Value>,
}

impl Said {
    fn text(text: &str) -> Said {
        Said {
            text: text.to_string(),
            ..Said::default()
        }
    }
}

/// A length in millimetres as a JSON number.
fn mm_value(l: &pmi::Length) -> Value {
    Value::from(l.mm())
}

/// A thread as the planner states it (`rules/holes.rs`, `rules/turned.rs`): its designation,
/// pitch, class and hand from the thread, its depths from its `thread manufacturing`
/// attributes, and haecceity's notation of it. What the planner has no field for (minor or
/// pitch diameter, crest, runout, a second class, a count of threads other than one) is added
/// to the text, and marks it text only.
fn thread_said(t: &pmi::Thread, p: &PartPmi) -> Said {
    let mut said = Said::text(&clean_numbers(
        &haecceity::presentation::thread(t, p).notation(),
    ));
    let f = &mut said.fields;
    f.insert(
        "side".into(),
        Value::from(match t.side {
            pmi::ThreadSide::Internal => "internal",
            pmi::ThreadSide::External => "external",
        }),
    );
    let spec = t.qualifier.clone().filter(|q| !q.is_empty());
    let designation = spec
        .as_deref()
        .and_then(|q| q.split(['x', '×']).next())
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "{}{}",
                t.form,
                haecceity::presentation::tolerance::number(&t.major_diameter.value)
            )
        });
    f.insert("designation".into(), Value::from(designation));
    if let Some(spec) = spec {
        f.insert("spec".into(), Value::from(spec));
    }
    if !t.fit_class.is_empty() {
        f.insert("class".into(), Value::from(t.fit_class.clone()));
    }
    f.insert(
        "hand".into(),
        Value::from(if t.hand == pmi::Hand::Left {
            "LH"
        } else {
            "RH"
        }),
    );
    for set in p.attributes.iter().filter(|a| {
        a.name == "thread manufacturing"
            && matches!(a.on, Some(pmi::NoteOwner::Feature(on)) if p.same_target(
                &ToleranceTarget::Feature(on), &ToleranceTarget::Feature(t.feature)))
    }) {
        for (name, v) in &set.items {
            let key = match name.as_str() {
                "pitch" => "pitch",
                "tapping drill diameter" => "drill_diameter",
                "tapping drill depth" => "drill_depth",
                "minimum full thread" => "full_thread",
                "thread length" => "length",
                "through" => {
                    if matches!(v, pmi::AttributeValue::Text(s) if s == "true")
                        || matches!(v, pmi::AttributeValue::Boolean(true))
                    {
                        f.insert("through".into(), Value::Bool(true));
                    }
                    continue;
                }
                _ => continue,
            };
            if let pmi::AttributeValue::Measure(m) = v {
                f.insert(key.into(), Value::from(value(m)));
            }
        }
    }
    let mut extra = Vec::new();
    if let Some(d) = &t.minor_diameter {
        extra.push(format!("minor ⌀{}", crate::pyfmt::g(d.mm())));
    }
    if let Some(d) = &t.pitch_diameter {
        extra.push(format!("pitch ⌀{}", crate::pyfmt::g(d.mm())));
    }
    if let Some(c) = &t.crest {
        extra.push(format!("crest {}", crate::pyfmt::g(c.mm())));
    }
    if t.runout.is_some() {
        extra.push("with runout".into());
    }
    if t.number_of_threads.0.to_f64() != 1.0 {
        extra.push(format!(
            "{} threads",
            haecceity::presentation::tolerance::number(&t.number_of_threads.0)
        ));
    }
    if !extra.is_empty() {
        said.text = format!("{} ({})", said.text, extra.join(", "));
        said.text_only = true;
    }
    said
}

/// A knurl's parameters, by the planner's names, and its text as the PMI scene words it.
fn knurl_said(k: &pmi::Knurl) -> Said {
    let pattern = match k.pattern {
        pmi::KnurlPattern::Straight => "straight",
        pmi::KnurlPattern::Diamond => "diamond",
        pmi::KnurlPattern::Diagonal => "diagonal",
    };
    let mut said = Said::text(&format!(
        "{pattern} knurl, pitch {} mm, major ⌀{} mm",
        crate::pyfmt::g(k.diametral_pitch.mm()),
        crate::pyfmt::g(k.major_diameter.mm())
    ));
    let f = &mut said.fields;
    f.insert("pattern".into(), Value::from(pattern));
    f.insert("pitch".into(), mm_value(&k.diametral_pitch));
    f.insert("diameter".into(), mm_value(&k.nominal_diameter));
    if let Some(n) = k.number_of_teeth {
        f.insert("teeth".into(), Value::from(n.0));
    }
    if let Some(d) = &k.tooth_depth {
        f.insert("tooth_depth".into(), mm_value(d));
    }
    if let Some(r) = &k.root_fillet {
        f.insert("root_fillet".into(), mm_value(r));
    }
    if let Some(a) = &k.helix_angle {
        f.insert("helix_angle".into(), Value::from(a.rad().to_degrees()));
    }
    if let Some(h) = k.helix_hand {
        f.insert(
            "hand".into(),
            Value::from(if h == pmi::Hand::Left { "LH" } else { "RH" }),
        );
    }
    // A major diameter other than the nominal (the planner states one diameter for both).
    if k.major_diameter.mm() != k.nominal_diameter.mm() {
        said.text = format!(
            "{}, nominal ⌀{} mm",
            said.text,
            crate::pyfmt::g(k.nominal_diameter.mm())
        );
        said.text_only = true;
    }
    said
}

/// A finish on faces: `√ Ra 1.6` as the planner states it; a texture of other parameters is
/// its parameters in full, text only.
fn finish_said(t: &pmi::SurfaceTexture) -> Said {
    match finish_words(t) {
        Some(words) => {
            let mut said = Said::text(&format!("√ {words}"));
            said.fields.insert("finish".into(), Value::from(words));
            said
        }
        None => Said {
            text: format!(
                "√ {}",
                haecceity::presentation::notes::texture_parameters(t).join(", ")
            ),
            text_only: true,
            fields: Map::new(),
        },
    }
}

/// A value of haecceity's model by its schema name (`maximum_material_requirement`).
fn json_name<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// A dimension as a drawing states it, and whether it states something beyond the structured
/// fields. haecceity's notation (`⌀6.6 ±0.1`, `⌀3.3 H11`, `⌀6.5 - ⌀6.7`, MAX/MIN, the
/// reference's brackets), with what it leaves out added: a basic dimension's `(basic)`, a
/// fit's class beside its stated limits, the other modifiers and the envelope requirement.
pub(crate) fn dimension_notation(d: &pmi::Dimension) -> (String, bool) {
    let mut text = clean_numbers(&haecceity::presentation::dimension_text(d));
    if d.nominal.is_none() {
        // No value stated (NIST CTC-01's presentation-only sizes): its kind in words.
        let r#type = match &d.kind {
            DimensionKind::Size {
                kind, path, angle, ..
            } => size_type(kind, path.is_some(), angle.is_some()),
            DimensionKind::Location {
                kind, path, angle, ..
            } => location_type(kind, path.is_some(), angle.is_some()),
        };
        text = format!("{}, no value stated", readable_type(r#type));
    }
    let mut text_only = false;
    match &d.tolerance {
        DimTolerance::Basic => text.push_str(" (basic)"),
        DimTolerance::Fit {
            class,
            limits: Some(_),
        } => text.push_str(&format!(" ({class})")),
        _ => {}
    }
    if let Some(q) = &d.qualifier {
        text_only = true;
        if let pmi::Qualifier::Other(words) = q {
            text.push_str(&format!(" {words}"));
        }
    }
    for m in &d.modifiers {
        text_only = true;
        if *m != pmi::DimensionModifier::Reference {
            let name = json_name(m);
            text.push_str(&format!(
                " ({})",
                name.as_str().unwrap_or("modifier").replace('_', " ")
            ));
        }
    }
    if d.principle == Some(pmi::Principle::EnvelopeRequirement) {
        text_only = true;
        text.push_str(" Ⓔ");
    }
    (text, text_only)
}

/// A tolerance modifier as a drawing shows it: a symbol glued to the value, or its name.
fn modifier_symbol(m: pmi::ToleranceModifier) -> Option<&'static str> {
    use pmi::ToleranceModifier as M;
    Some(match m {
        M::MaximumMaterialRequirement => "Ⓜ",
        M::LeastMaterialRequirement => "Ⓛ",
        M::FreeState => "Ⓕ",
        M::TangentPlane => "Ⓣ",
        M::ReciprocityRequirement => "Ⓡ",
        M::StatisticalTolerance => "⟨ST⟩",
        _ => return None,
    })
}

fn datum_modifier(m: &pmi::DatumModifier) -> String {
    use pmi::SimpleDatumModifier as S;
    match m {
        pmi::DatumModifier::Simple(S::MaximumMaterialRequirement) => "Ⓜ".into(),
        pmi::DatumModifier::Simple(S::LeastMaterialRequirement) => "Ⓛ".into(),
        pmi::DatumModifier::Simple(S::FreeState) => "Ⓕ".into(),
        pmi::DatumModifier::Simple(other) => format!(
            "[{}]",
            json_name(other)
                .as_str()
                .unwrap_or("modifier")
                .replace('_', " ")
        ),
        pmi::DatumModifier::WithValue { kind, value: v } => format!(
            "[{} {}]",
            json_name(kind)
                .as_str()
                .unwrap_or("modifier")
                .replace('_', " "),
            haecceity::presentation::tolerance::number(v.decimal())
        ),
    }
}

/// A geometric tolerance as its frame states it, cell by cell (`⌖ ⌀0.3Ⓜ A B C`), and whether
/// it states something beyond the structured fields: every modifier, the zone (⌀, S⌀,
/// projected, non-uniform), unit basis, maximum, unequal zone, all over, and each datum
/// reference with its modifiers (a common datum as `A-B`).
pub(crate) fn tolerance_notation(t: &pmi::GeometricTolerance, p: &PartPmi) -> (String, bool) {
    // Values as the file states them, in its own unit, as haecceity's notation gives them.
    let number = |v: &PmiValue| haecceity::presentation::tolerance::number(v.decimal());
    let frame = haecceity::presentation::tolerance(t, p);
    let symbol = frame.cells.first().cloned().unwrap_or_default();
    let mut text_only = false;
    let mut zone = String::new();
    let mut after = Vec::new();
    if let Some(z) = &t.zone {
        if z.form.is_diametral() {
            zone.push('⌀');
        } else if z.form.is_spherical() {
            zone.push_str("S⌀");
        }
        if let Some(projected) = &z.projected {
            text_only = true;
            after.push(format!("Ⓟ{}", number(&projected.length)));
        }
        if z.non_uniform.is_some() {
            text_only = true;
            after.push("UF".into());
        }
        if z.affected_plane.is_some() {
            text_only = true;
            after.push("(affected plane)".into());
        }
        if let Some(angle) = &z.runout_angle {
            text_only = true;
            after.push(format!("(runout at {}°)", number(angle)));
        }
    }
    let mut cell = format!(
        "{zone}{}",
        t.magnitude
            .as_ref()
            .map(|m| haecceity::presentation::tolerance::number(m.decimal()))
            .unwrap_or_default()
    );
    if let Some(basis) = &t.unit_basis {
        text_only = true;
        cell.push_str(&format!("/{}", number(&basis.size)));
        if let Some(second) = basis.area.as_ref().and_then(|a| a.second.as_ref()) {
            cell.push_str(&format!("×{}", number(second)));
        }
    }
    for m in &t.modifiers {
        match modifier_symbol(*m) {
            Some(s) => cell.push_str(s),
            None => after.push(
                json_name(m)
                    .as_str()
                    .unwrap_or("modifier")
                    .replace('_', " "),
            ),
        }
    }
    if let Some(u) = &t.unequal {
        text_only = true;
        after.push(format!("UZ{}", number(u)));
    }
    if let Some(max) = &t.maximum {
        text_only = true;
        after.push(format!("MAX {}", number(max)));
    }
    let mut cells = vec![symbol, cell];
    cells.extend(after);
    for c in t.datums.iter().flat_map(|s| s.compartments()) {
        if c.references.len() > 1 || !c.modifiers.is_empty() {
            text_only = true;
        }
        let mut compartment = c
            .references
            .iter()
            .map(|r| {
                let letter = p
                    .datums
                    .get(r.datum.0)
                    .map(|d| d.label().to_string())
                    .unwrap_or_default();
                if !r.modifiers.is_empty() {
                    text_only = true;
                }
                letter + &r.modifiers.iter().map(datum_modifier).collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("-");
        compartment.extend(c.modifiers.iter().map(datum_modifier));
        cells.push(compartment);
    }
    for a in &t.auxiliary {
        text_only = true;
        cells.push(
            match a {
                pmi::AuxiliaryClassification::AllOver => "all over",
                pmi::AuxiliaryClassification::UnlessOtherwiseSpecified => {
                    "unless otherwise specified"
                }
            }
            .into(),
        );
    }
    (
        clean_numbers(
            &cells
                .into_iter()
                .filter(|c| !c.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        ),
        text_only,
    )
}

/// The significant digits a display keeps: more is a file's float noise (`1.06500000000426`,
/// a value converted and written back), never a stated precision.
const DISPLAY_DIGITS: usize = 10;

/// Every decimal number in `text` as a drawing reads it: float noise beyond
/// [`DISPLAY_DIGITS`] significant digits rounded away (`⌀1.06500000000426` → `⌀1.065`,
/// `0.0500000000002` → `0.05`), then trailing zeros after the point trimmed (`0.016000` →
/// `0.016`). A value with fewer digits is never rounded. The structured fields keep the
/// value as read.
pub fn clean_numbers(text: &str) -> String {
    static NUMBER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let number = NUMBER.get_or_init(|| regex::Regex::new(r"\d+\.\d+").expect("a regex"));
    number
        .replace_all(text, |c: &regex::Captures| clean_number(&c[0]))
        .into_owned()
}

fn clean_number(s: &str) -> String {
    let digits = s.chars().filter(char::is_ascii_digit).collect::<String>();
    let significant = digits.trim_start_matches('0').len();
    let s = if significant > DISPLAY_DIGITS
        && let Ok(v) = s.parse::<f64>()
    {
        let rounded: f64 = format!("{v:.*e}", DISPLAY_DIGITS - 1).parse().unwrap_or(v);
        let decimals = (DISPLAY_DIGITS as i32 - 1 - rounded.abs().log10().floor() as i32).max(0);
        format!("{rounded:.*}", decimals as usize)
    } else {
        s.to_string()
    };
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// An OpenCascade type name in words: `Size_Diameter` → `Diameter`,
/// `Location_LinearDistance` → `Linear distance`.
pub fn readable_type(r#type: &str) -> String {
    let name = r#type
        .strip_prefix("Size_")
        .or_else(|| r#type.strip_prefix("Location_"))
        .unwrap_or(r#type);
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c == '_' {
            out.push(' ');
        } else if c.is_ascii_uppercase() && i > 0 && !out.ends_with(' ') {
            out.push(' ');
            out.push(c.to_ascii_lowercase());
        } else if i > 0 {
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A surface texture of one roughness in micrometres as specify-core states a finish
/// (`Ra 3.2`); `None` for any other.
fn finish_words(t: &pmi::SurfaceTexture) -> Option<String> {
    let [p] = t.parameters.as_slice() else {
        return None;
    };
    (p.value.unit == LengthUnit::Micrometre).then(|| {
        format!(
            "{} {}",
            p.characteristic,
            crate::pyfmt::g(p.value.value.to_f64())
        )
    })
}

/// `X` of "X µm unless otherwise specified" (or "stated"), the part's default finish (Python's
/// `_DEFAULT_FINISH.fullmatch`: the whole text must end so, so `X` is all before the ending).
fn default_finish(text: &str) -> Option<&str> {
    [
        " µm unless otherwise specified",
        " µm unless otherwise stated",
    ]
    .iter()
    .find_map(|tail| text.strip_suffix(tail))
}

/// The faces of `features`, following groups and derived features to what they are made of
/// (as Python's `datum_faces` follows shape aspect relationships), ascending.
pub(crate) fn faces_of(pmi: &PartPmi, features: &[FeatureId]) -> Vec<usize> {
    faces_in_arena(&pmi.features, features)
}

/// The same support resolution while the writer still owns the feature arena.
pub(crate) fn faces_in_arena(arena: &[Feature], features: &[FeatureId]) -> Vec<usize> {
    fn walk(
        arena: &[Feature],
        f: FeatureId,
        seen: &mut BTreeSet<usize>,
        out: &mut BTreeSet<usize>,
    ) {
        if !seen.insert(f.0) {
            return;
        }
        match arena.get(f.0) {
            Some(Feature::Items(items)) => out.extend(items.iter().filter_map(|a| match a {
                Anchor::Face(face) => Some(face.0),
                Anchor::Edge(_) | Anchor::Geometry(_) => None,
            })),
            Some(Feature::Group { members, .. }) => {
                for m in members {
                    walk(arena, *m, seen, out);
                }
            }
            Some(Feature::Derived { from, .. }) => {
                for m in from {
                    walk(arena, *m, seen, out);
                }
            }
            None => {}
        }
    }
    let (mut seen, mut out) = (BTreeSet::new(), BTreeSet::new());
    for f in features {
        walk(arena, *f, &mut seen, &mut out);
    }
    out.into_iter().collect()
}

/// A value in millimetres or degrees. A value stated in that unit is its decimal as written;
/// anything else is converted.
pub(crate) fn value(v: &PmiValue) -> f64 {
    match &v.quantity {
        Quantity::Length(l) if l.unit == LengthUnit::Millimetre => l.value.to_f64(),
        Quantity::Length(l) => l.mm(),
        Quantity::Angle(a) if a.unit == AngleUnit::Degree => a.value.to_f64(),
        Quantity::Angle(a) => a.rad().to_degrees(),
    }
}

/// OpenCascade's names for the size kinds (`STEPCAFControl_GDTProperty::GetDimType`).
const SIZE_TYPES: [(SizeKind, &str); 14] = [
    (SizeKind::CurveLength, "Size_CurveLength"),
    (SizeKind::Diameter, "Size_Diameter"),
    (SizeKind::SphericalDiameter, "Size_SphericalDiameter"),
    (SizeKind::Radius, "Size_Radius"),
    (SizeKind::SphericalRadius, "Size_SphericalRadius"),
    (
        SizeKind::ToroidalMinorDiameter,
        "Size_ToroidalMinorDiameter",
    ),
    (
        SizeKind::ToroidalMajorDiameter,
        "Size_ToroidalMajorDiameter",
    ),
    (SizeKind::ToroidalMinorRadius, "Size_ToroidalMinorRadius"),
    (SizeKind::ToroidalMajorRadius, "Size_ToroidalMajorRadius"),
    (
        SizeKind::ToroidalHighMajorDiameter,
        "Size_ToroidalHighMajorDiameter",
    ),
    (
        SizeKind::ToroidalLowMajorDiameter,
        "Size_ToroidalLowMajorDiameter",
    ),
    (
        SizeKind::ToroidalHighMajorRadius,
        "Size_ToroidalHighMajorRadius",
    ),
    (
        SizeKind::ToroidalLowMajorRadius,
        "Size_ToroidalLowMajorRadius",
    ),
    (SizeKind::Thickness, "Size_Thickness"),
];

/// OpenCascade's names for the location kinds. Its table has no 'linear distance centre',
/// 'inner' or 'outer' (PMI practice Table 2's one-sided forms): those, like any name outside
/// its table, are `Location_None`, its default type.
const LOCATION_TYPES: [(LocationKind, &str); 10] = [
    (LocationKind::CurvedDistance, "Location_CurvedDistance"),
    (LocationKind::LinearDistance, "Location_LinearDistance"),
    (
        LocationKind::LinearDistanceCentreOuter,
        "Location_LinearDistance_FromCenterToOuter",
    ),
    (
        LocationKind::LinearDistanceCentreInner,
        "Location_LinearDistance_FromCenterToInner",
    ),
    (
        LocationKind::LinearDistanceOuterCentre,
        "Location_LinearDistance_FromOuterToCenter",
    ),
    (
        LocationKind::LinearDistanceOuterOuter,
        "Location_LinearDistance_FromOuterToOuter",
    ),
    (
        LocationKind::LinearDistanceOuterInner,
        "Location_LinearDistance_FromOuterToInner",
    ),
    (
        LocationKind::LinearDistanceInnerCentre,
        "Location_LinearDistance_FromInnerToCenter",
    ),
    (
        LocationKind::LinearDistanceInnerOuter,
        "Location_LinearDistance_FromInnerToOuter",
    ),
    (
        LocationKind::LinearDistanceInnerInner,
        "Location_LinearDistance_FromInnerToInner",
    ),
];

/// The type name OpenCascade gives a dimensional (or angular) size: angular first, then the
/// name's kind, then a size with a path; otherwise its default, `Location_None`.
pub(crate) fn size_type(kind: &SizeKind, path: bool, angular: bool) -> &'static str {
    if angular {
        return "Size_Angular";
    }
    SIZE_TYPES
        .iter()
        .find(|(k, _)| k == kind)
        .map(|(_, name)| *name)
        .unwrap_or(if path {
            "Size_WithPath"
        } else {
            "Location_None"
        })
}

/// The type name OpenCascade gives a dimensional (or angular) location, as [`size_type`].
pub(crate) fn location_type(kind: &LocationKind, path: bool, angular: bool) -> &'static str {
    if angular {
        return "Location_Angular";
    }
    LOCATION_TYPES
        .iter()
        .find(|(k, _)| k == kind)
        .map(|(_, name)| *name)
        .unwrap_or(if path {
            "Location_WithPath"
        } else {
            "Location_None"
        })
}

/// OpenCascade's name for a tolerance kind (`XCAFDimTolObjects_GeomToleranceType`).
pub fn tolerance_type(kind: ToleranceKind) -> &'static str {
    use ToleranceKind as K;
    match kind {
        K::Angularity => "Angularity",
        K::CircularRunout => "CircularRunout",
        K::Coaxiality => "Coaxiality",
        K::Concentricity => "Concentricity",
        K::Cylindricity => "Cylindricity",
        K::Flatness => "Flatness",
        K::LineProfile => "ProfileOfLine",
        K::Parallelism => "Parallelism",
        K::Perpendicularity => "Perpendicularity",
        K::Position => "Position",
        K::Roundness => "CircularityOrRoundness",
        K::Straightness => "Straightness",
        K::SurfaceProfile => "ProfileOfSurface",
        K::Symmetry => "Symmetry",
        K::TotalRunout => "TotalRunout",
    }
}

impl Reading {
    /// BOM metadata from this already parsed/read part; no extra STEP or PMI read.
    pub(crate) fn bom(&self, loaded: &LoadedPart) -> Result<crate::bom::Item, String> {
        crate::bom::read_rows(
            &self.doc,
            std::slice::from_ref(&loaded.definition),
            loaded.binding.part,
            std::slice::from_ref(&self.pmi),
        )?
        .into_iter()
        .next()
        .ok_or("missing BOM row".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_are_always_present_and_last_when_empty() {
        let mut d = ExistingPmi::new("dimension", "Size_Diameter", Vec::new());
        d.value = Some(8.0);
        assert_eq!(
            serde_json::to_string(&d).unwrap(),
            r#"{"kind":"dimension","type":"Size_Diameter","value":8.0,"faces":[]}"#
        );
        let datum = ExistingPmi::new("datum", "A", vec![3]);
        assert_eq!(
            serde_json::to_string(&datum).unwrap(),
            r#"{"kind":"datum","type":"A","faces":[3]}"#
        );
    }

    #[test]
    fn display_numbers_lose_float_noise_and_trailing_zeros_only() {
        for (raw, shown) in [
            ("⌀1.06500000000426", "⌀1.065"),
            ("⌖ 0.0500000000002 A", "⌖ 0.05 A"),
            ("(0.750000000002999)", "(0.75)"),
            ("⌖ ⌀0.016000Ⓜ A", "⌖ ⌀0.016Ⓜ A"),
            ("0.020000", "0.02"),
            ("⌀0.250000 +0.003000/0", "⌀0.25 +0.003/0"),
            ("10.0", "10"),
            ("0.123456789", "0.123456789"),
            ("1234.56789012", "1234.56789"),
            ("25 ±0.1", "25 ±0.1"),
        ] {
            assert_eq!(clean_numbers(raw), shown, "{raw}");
        }
    }

    #[test]
    fn type_names_read_as_words() {
        assert_eq!(readable_type("Size_Diameter"), "Diameter");
        assert_eq!(readable_type("Location_LinearDistance"), "Linear distance");
        assert_eq!(readable_type("Location_None"), "None");
    }

    #[test]
    fn a_default_finish_is_read_back_to_its_value() {
        assert_eq!(
            default_finish("Ra 1.6 µm unless otherwise specified"),
            Some("Ra 1.6")
        );
        assert_eq!(
            default_finish("Ra 3.2 µm unless otherwise stated"),
            Some("Ra 3.2")
        );
        assert_eq!(default_finish("Ra 0.8 µm"), None);
    }

    #[test]
    fn edge_break_recognition_is_only_an_explicit_instruction() {
        for text in [
            "Remove all burrs and break sharp edges",
            "BREAK ALL SHARP EDGES 0.2 MAX",
            "Break edges",
            "Breaking all edges",
        ] {
            assert!(edge_break_note(text), "{text}");
        }
        for text in [
            "Mask these features before coating with primer",
            "Inspect edges",
            "Edge finish Ra 3.2",
            "Break coating before inspection",
            "Keep the sharp edges clean",
        ] {
            assert!(!edge_break_note(text), "{text}");
        }
    }

    #[test]
    fn every_tolerance_kind_has_an_opencascade_name() {
        for (_, kind) in ToleranceKind::ALL {
            assert!(!tolerance_type(kind).is_empty());
        }
        assert_eq!(
            size_type(&SizeKind::Other("x".into()), false, false),
            "Location_None"
        );
        assert_eq!(
            location_type(&LocationKind::LinearDistanceCentre, false, false),
            "Location_None"
        );
    }
}
