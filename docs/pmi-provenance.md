# PMI written by Specify

For #22, the exact, case-sensitive authorship token is `specify-core:written:v1`
(`writer::PMI_PROVENANCE`). It identifies Specify as the writer of a requirement, not its
original designer, the source of its nominal geometry, or a security/authenticity claim.
Unmarked PMI is of unknown authorship. Do not infer authorship from a shared face, datum,
part, file name or the presence of stored interview answers.

The writer uses standard AP242 fields and a standard user defined attribute (UDA), retaining
reserved vocabulary such as dimension kinds, knurl patterns and material-property kinds:

| Semantic item | Exact marker location |
|---|---|
| Size or location dimension | Dimension-owned `property_definition` named `Specify provenance`, associated with a same-named `general_property`, whose representation contains `descriptive_representation_item('writer','specify-core:written:v1')` |
| Geometric tolerance | `geometric_tolerance.description` (including complex tolerance instances) |
| Datum | `datum`'s inherited `shape_aspect.description`; `identification` remains the datum letter |
| Thread | `thread`'s inherited `characterized_object.description` |
| Knurl | `turned_knurl`'s inherited `characterized_object.name`; its description remains its pattern |
| General tolerance | `property_definition('default tolerances', <token>, ...)` |
| Face or default surface finish | `property_definition('surface texture', <token>, ...)` (also its parameter property definitions) |
| Part manufacturing notes, thread manufacturing data, caller attributes and stored answers | Each set's `property_definition.description` |
| Material | The material property's `representation.name`; `property_definition.description` stays `material name` for standard interpretation |

New ordinary feature and datum-feature shape aspects with an empty description also receive
the token in that description. Helper links, parameter values, presentation and geometry do
not independently express manufacturing requirements and need no authorship token.

These locations preserve the semantic values. An ordinary dimension has no free description
field: the UDA gives it an explicit relationship to its author, without changing its dimension
kind or its tolerance principle. The other fields avoid extra relationship graphs where a
free standard field already fits. They are assignments, not appended suffixes; repeated writes
cannot produce doubled markers. Only records newly emitted in the current edit are marked.
Reused input datums/features and every imported requirement remain unchanged, even in a mixed
assembly or when new PMI refers to the same face.

## Reading with quiddity v0.1.0-alpha.8

Checked against the pinned source, commit `c62081c6d999683a12bbe7091adae413a61a126c`, in
`crates/haecceity/src/pmi/{model.rs,mod.rs,read.rs,write.rs}`. No upstream change is needed.
`pmi::read` exposes the dimension UDA as `PartPmi.attributes`, with
`on = Some(NoteOwner::Dimension(DimensionId(index)))`, name `Specify provenance`, and text
item `writer` equal to the token. Tolerances expose it as `GeometricTolerance.description`.
Attribute sets expose it as `AttributeSet.description`.

For the remaining kinds the semantic model omits these free fields. Use
`PmiRead.provenance.parts[part]`: `datums`, `threads`, `knurls`, `general`, `surface_textures`
and `material` contain, index for index, the STEP instance IDs consumed for each requirement.
Inspect the corresponding root record in the same `p21::Document` at the location above.
For example, a thread is Specify-authored only when its consumed `THREAD` record's
description equals the token. A marker on a referenced feature or datum does not count.
Do not search arbitrary strings anywhere in the consumed graph. `tests/pmi_provenance.rs`
checks this STEP-only readback, EXPRESS validity and input preservation.

## Specify's existing-PMI path and rewriting

Previously `existing::read_existing` reported Specify-written semantic PMI just like imported
PMI; stored matching answers were the only ownership signal used by the writer and by
`api::analyse`'s resume path. Older unmarked output still follows that legacy behaviour.

Now each marked `ExistingPmi` additionally serializes
`"provenance":"specify-core:written:v1"`. Unmarked items omit the field. Notes aggregated
by kind and faces retain separate groups for marked and unmarked requirements. Existing
values, face indices, datum letters and settings are retained. The interview's ordinary
resume path still uses stored answers and returns its usual empty `existing` list.

Writing modes retain their existing contract: a part with matching stored answers is
replaced; a part with PMI and no matching stored answers is added to. Item authorship remains
recognisable in both modes. The marker alone does not grant permission to delete PMI.
Unchanged resumed output retains its bytes only if it already carries the markers, so an
older unmarked resumed file gains provenance on its next write. A subsequent resumed write
is byte-identical. An empty additive edit preserves the marked records byte for byte.
