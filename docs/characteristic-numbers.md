# Stable characteristic numbers (For #65)

The STEP file is the authority for inspection numbers. CSV and drawing consumers read
these numbers; they must not allocate their own or use recognition feature IDs as numbers.
There is no dependency on the editor or draftwright.

## AP242 storage

Specify uses the **standard derived `id` slot**, populated by
`ID_ATTRIBUTE('<positive decimal integer>',#characteristic)`. It does not put numbers in
`name` (which carries dimension type or other semantics) or in the authorship description.
The integer is canonical decimal, starts at 1, and is unique within each part.

This choice follows CAx-IF [PMI Recommended Practices v4.1, §5.1, Figure 4](https://www.mbx-if.org/home/wp-content/uploads/2024/06/rec_pracs_pmi_v41.pdf),
which recommends `id_attribute` and describes its uniqueness constraints. The pinned
haecceity AP242 EXPRESS table's `id_attribute_select` includes `dimensional_size`,
`geometric_tolerance`, `shape_aspect`, `shape_aspect_relationship` and `property_definition`.
`dimensional_location` inherits `shape_aspect_relationship`; `datum` inherits `shape_aspect`.
The implementation was checked against quiddity v0.2.3's `docs/step-ap242.md`, its PMI writer,
and its schema table, rather than assuming the visible names were free identification slots.

| Characteristic | Exact entity carrying `ID_ATTRIBUTE` |
| --- | --- |
| Size or location dimension | `DIMENSIONAL_SIZE` or `DIMENSIONAL_LOCATION` (and subtypes) |
| Geometric tolerance | The simple or complex `GEOMETRIC_TOLERANCE` root |
| Datum feature requirement | `DATUM`; follow `SHAPE_ASPECT_RELATIONSHIP` from its `DATUM_FEATURE` to this datum |
| Surface texture, including a part default | `PROPERTY_DEFINITION('surface texture',...)`, not a parameter property |
| Thread or knurl | Its exclusive `PRODUCT_DEFINITION_SHAPE` whose definition is `THREAD` or `TURNED_KNURL` |

Only one entity carries each number. In particular, putting the same number on both
`DATUM` and `DATUM_FEATURE` would violate AP242 shape-aspect id uniqueness. A datum feature
may also be shared by several datum labels or a dimension; using `DATUM` keeps those
requirements distinct. Threads and knurls are `characterized_object`s, outside
`id_attribute_select`; their definition shapes provide a standard property-definition id
slot without assigning a requirement's identity to shared geometry.

The part-level **user-defined property name is `specify characteristic numbers`**.
It uses the existing CAx-IF UDA `AttributeSet` route, on that part's product definition, with
one text item named **`history`**. Its compact JSON is:

```json
{"schema":1,"faces":"<resume schema-2 face fingerprint>","high_water":7,"entries":[{"key":{"specification":"<opaque disambiguator>","kind":"dimension","type":"Size_Diameter","faces":[9],"reference":[]},"number":3}]}
```

`high_water` is the greatest number ever allocated, including retired requirements. Entries
are the current requirements, not a list of reusable numbers. The key's specification is an
internal disambiguator for coexisting requirements; readers use `ID_ATTRIBUTE` instead of
interpreting it. A replace carries the history even when all requirements are removed.
An additive write retains earlier snapshots; the last snapshot is current and its counter
must be at least every preceding snapshot's counter. The property name is reserved from
caller attributes. A corrupt history or exhausted counter refuses the write, without output.

## Allocation and carry

Numbers use **allocation order**, not inspection/display order. Inspection order would
renumber requirements when an earlier row is inserted, contradicting the stability rule.
On the first write the deterministic order is datums, dimensions, geometric tolerances,
surface textures, threads, then knurls, each in semantic model order. Subsequent writes
match all existing requirements before allocating new ones above `high_water`. Reordering,
adding or removing another requirement does not compact the numbers. A removed number is
retired permanently; reintroducing that requirement later receives a new number.

Numbers are read from the actual STEP entities before planning a rewrite. Identity uses
requirement kind/type and geometric face support (including the reference support of a
location), never recognition feature IDs or entity IDs. Existing exact requirements match
first, including their specifications so an added control on the same face cannot steal an
existing control's number. A uniquely matched requirement can change its value and retain
its number. Unique support growth follows the same conservative geometric-support rule as
answer resume: this accommodates quiddity adding defining faces after an upgrade. Ambiguous
or missing correspondence is not guessed; those numbers are retired, while the high-water
mark survives.

`revision::diff` uses its existing quiddity fingerprints/face correspondence to map the old
numbered supports to the revised part. Its returned `answers` (including CLI `answers.json`
and WASM `diff.report.answers`) contains an opaque `_specify_characteristic_numbers` member
when the old file has number history. **Keep that member with the returned answers** when
applying/writing the revision. The ordinary writer verifies its target geometry fingerprint,
uses the carried history and consumes the member; it does not store it among user answers.
The next resume reads the freshly written entity numbers and current high-water mark.
No separate number-matching operation is needed in a drawing or CSV consumer.

An Add-mode write preserves imported PMI byte for byte. Existing numeric characteristic ids
reserve their numbers; newly written characteristics receive higher ones. A foreign datum
that haecceity reuses is not written or numbered again. Material, general-tolerance classes,
text notes and caller metadata are settings rather than individual inspection characteristics.

## Read APIs and compatibility

- `characteristics::read(&LoadedPart)` returns numbered requirements, each with `number`,
  `entity` (the exact numeric-id carrier), `kind`, `type`, `faces`, and optional `reference`.
  `characteristics::number(&Document, entity)` reads that entity's numeric id directly.
- `api::analyse` adds `characteristics` only when the file has numbered requirements. It is
  available even on a resumed file, where interview `existing` remains empty.
- `existing::read_existing` / `Reading::existing` add `ExistingPmi.number: Option<u64>`;
  JSON omits `number` when absent. Numbered thread/knurl/finish rows are kept distinct instead
  of aggregating different requirements of the same kind. A default part texture is in the
  authoritative `characteristics` list even though it has no face-note row.
- WASM `IntentEngine.snapshot().analysis` exposes the same list, and `writePmi` and `diff`
  use the same writer and carry path. JavaScript signatures are unchanged.

Files without numeric characteristic ids have no extra analysis or PMI JSON fields. Their
legacy serialization and note aggregation/order are unchanged. No write adds a history
property to a part that has never allocated a characteristic number. No-op resumed writes
retain their original bytes; lossless Add keeps every original entity record unchanged.

## General ISO 2768 rows

General CSV rows derived from ISO 2768 have **no number**: the underlying general class does
not state individual dimension or location requirements. Specify does not invent PMI
entities or numbers for these inferred limits. Consumers should leave the number cell empty
or explicitly mark it unnumbered. An explicitly authored size/location/FCF has its own PMI
entity and is numbered even if its value agrees with a general limit.

## Verification and upstream boundary

The writer verifies both semantic PMI and the numbers read back from their actual entities,
including number uniqueness and the history. Schema and EXPRESS-rule tests cover the standard
slots. `tests/characteristic_numbers.rs` covers all characteristic kinds, repeat writes,
additions, value edits, removal/re-addition/empty writes, foreign append, old-file compatibility,
an older-quiddity fixture and the `hole_removed` revision pair. Numberless analysis, PMI
serialization and settings-only writes are also checked against SHA-256 snapshots captured
from the unmodified parent commit in a separate checkout. The cross-engine suite preserves
its old specimens and verifies that new writer outputs differ only by numbering metadata,
comparing every other entity and reference. The older-engine test adds
only `ID_ATTRIBUTE` records to the committed alpha.6 specimen; it never regenerates its old
PMI with the new writer. Requirements newer rules stop emitting are retired.

No haecceity API change is required: Specify uses its existing PMI/AttributeSet writer and
lossless `Document::apply`/`Edit` APIs. The pinned reader consumes ids on dimensions, datums,
tolerances and surface textures. It does not consume ids on thread/knurl definition shapes,
so Specify removes just those numeric inverse annotations before Replace. For upstream
first-class support, haecceity could expose optional ids alongside each semantic root, and
consume definition-shape `ID_ATTRIBUTE`s in the thread/knurl provenance; that is not required
for this implementation and no quiddity files are edited here.
