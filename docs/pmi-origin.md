# Dimension origin and source (For #77)

This is Specify's producer convention for readers, including Draftwright. It describes
stored intent, independently of [writer authorship](pmi-provenance.md) and
[inspection numbers](characteristic-numbers.md). No drawing planner or `pmiScene` is involved.

## AP242 storage

Every newly written size and location dimension has one CAx-IF UDA `AttributeSet` named
**`specify pmi origin`**, attached to the **dimension entity itself**. Its
`PROPERTY_DEFINITION.definition` references `DIMENSIONAL_SIZE` (including subtypes) or
`DIMENSIONAL_LOCATION` (including subtypes), never their shared shape aspect or representation.
Its representation contains these case-sensitive `DESCRIPTIVE_REPRESENTATION_ITEM`s:

| Item name | Text value |
| --- | --- |
| `origin` | Exactly `general_tolerance`, `requirement`, or `imported` |
| `source` | Optional compact JSON, described below |

The associated `GENERAL_PROPERTY.name` equals `PROPERTY_DEFINITION.name`, with an empty
`GENERAL_PROPERTY_ASSOCIATION.name`. Haecceity writes the property description as
`user defined attribute`; consumers identify it by its name and associated general property.
This is the same `AttributeSet` path as characteristic-number history, through
`writer::write_bytes_with` / `write_parts_bytes_with` and haecceity's lossless append.
The property name is reserved against caller attributes.

Checked against the pinned **quiddity v0.2.6 / haecceity b9addd9** EXPRESS tables:
`property_definition.definition` selects `characterized_definition`, whose `shape_definition`
branch includes `shape_aspect_relationship`. Locations inherit that relationship and are
valid owners in both available editions. Sizes do **not** inherit `shape_aspect`.
Edition 4 additionally selects `characterized_item`, which explicitly includes
`dimensional_size` and `geometric_tolerance`. Edition 1 lacks that branch. A write adding
these attributes to an edition-1 file upgrades only `FILE_SCHEMA` to haecceity's edition-4
target through `Document::apply`; the original entity records stay byte-identical.
Non-AP242 inputs already gain the writer's edition-4 header. Reads never upgrade a file.
Geometric tolerances are schema-eligible but are outside this dimension-only convention's
current implementation; no origin/source is newly attached to them.

The vocabulary/structure was checked against
[CAx-IF UDA practice v1.8, §§5–7](https://www.mbx-if.org/home/wp-content/uploads/2024/05/rec_prac_user_def_attributes_v18.pdf)
and [NIST SFA's UDA checks in `sfa-valprop.tcl`](https://github.com/usnistgov/SFA/blob/master/source/sfa-valprop.tcl).
SFA checks the general-property association and equal property names and reads descriptive
representation items. The Specify property/item names above are a producer convention,
not names prescribed by CAx-IF or NIST. No Windows SFA executable is required by the tests.

The v0.2.6 writer (quiddity-rust#137) splits a sized datum's feature and size, uses
composite multi-face aspects, and gives locations their own aspects. Origin/source UDAs
still belong to the dimensional-size/location root, never the datum feature or any aspect.

## Meaning and absence

- `general_tolerance`: a nominal-only size/location emitted from the general-tolerance
  list, with no individual requirement. The part's general class governs its limits.
- `requirement`: a dimension emitted for a stated requirement, including a fit, toleranced
  distance or basic location. Explicitly accepted defaults count exactly like manually
  answered requirements. Accepting a plain/general choice does not invent an individual
  tolerance: its resulting general-list dimension remains `general_tolerance`.
- `imported`: reserved for readers/producers recording PMI kept from the input. Specify
  **never modifies kept foreign PMI**, so it does not add even this attribute to it.

For a **kept foreign entity**, absence means imported. A consumer can map general/requirement/
kept foreign to its future statuses `general` / `requirement` / `existing`. For legacy
Specify-authored PMI without the attribute, origin is unknown; do not infer an individual
requirement or general tolerance just from nominal/tolerance values. The old authorship
marker can distinguish that legacy case. Unknown or duplicate origin values are not interpreted as requirements. Malformed source
data is omitted independently; duplicate attribute sets are ignored. Native and WASM JSON retain absence instead of adding a
synthetic `imported` field to old files. This preserves old analysis and PMI serialization.

## Source grammar

`source` is a UTF-8 JSON object serialized compactly into one text item, for example:

```json
{"schema":2,"faces":"<64 lowercase hex SHA-256 digits>","support":[9],"parameter":"bore.diameter"}
```

`schema` is the resume fingerprint schema, currently exactly `2`. `faces` is
`resume::fingerprint` of the part: the same face namespace used by characteristic-number
history. `support` is the sorted, unique, nonempty, zero-based **measured target face** set
used in a characteristic-number key (the target of a location, excluding its reference).
Together `faces` and `support` identify the feature geometrically; neither recognition IDs
such as `holes:9` nor STEP entity numbers belong in the source. Different parameters on
that support are different measurements. Sources remain valid when recognition IDs or
feature measurements change; face indices are meaningful only within their fingerprint.

`parameter` follows Draftwright's measurement paths: dot-separated nonempty lowercase
ASCII/digit/underscore components. Current diameter mappings, checked in Draftwright's
`src/dims/{holes,bosses,turned}.rs`, are `bore.diameter` for holes/patterns,
`boss.diameter`, `step.diameter`, and `turned_body.diameter`. These are the recognised diameter families the writer currently produces. Recognised locations use `location.length.x`, `.y`, or `.z`,
chosen by the reference plane normal's largest absolute component in stored part coordinates
(ties X, then Y, then Z). A future producer may add paths using the same grammar.

The writer resolves a requirement's recognised feature during planning, preferring its
explicit feature association, then a unique exact support match, then a unique containing
feature, within the diameter families above or hole/pattern/boss location families. Aggregate
bodies and plane-level recognition alone do not give a generic plane distance a feature
parameter. Ambiguous/unrecognised supports get **origin only**. It stores the measured support,
not the recogniser's full defining-face list, which can grow after a kernel upgrade.
An existing source is preserved through the characteristic allocator's unique support-growth
match on unchanged geometry, including when recognition no longer names that feature. Revision to different geometry
creates a source in the new fingerprint's namespace. Readers do not rerun recognition.

## Read APIs and compatibility

`pmi_origin::read(&PartPmi, NoteOwner::Dimension(DimensionId(i)))` returns optional typed
`origin` and `source`, directly from haecceity's read attributes. `characteristics::read`
and `api::analyse(...).characteristics` add those same optional fields to per-dimension
records, including nominal-only general dimensions and resumed files. `ExistingPmi` in
`existing::read_existing` / `Reading::existing` adds them to the dimension rows it already
exposes. Locations retain the legacy `ExistingPmi` convention: `faces` is the reference
and `faces2` is the target, whereas `source.support` always identifies the target. The
interview continues to omit general dimensions (#63).

WASM `IntentEngine.snapshot().analysis.characteristics` exposes the identical stored
metadata. `writePmi` uses the native planner/writer. JavaScript signatures are unchanged.
Files without attributes add no fields; their old analysis and PMI outputs are unchanged.
The writer verifies these UDAs through haecceity's ordinary full semantic readback comparison.
