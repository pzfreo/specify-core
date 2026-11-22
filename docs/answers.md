# Answer files and changed geometry

Answers remain a JSON map from question ID to answer. Existing plain maps work in CLI
`questions`/`write`, serve, Python, WASM and the editor's calls through WASM:

```json
{"part.material":"6061-T6","datum.A":[4],"datum.B":[1],"datum.C":[0]}
```

A plain map has no record of the geometry on which its face indices were chosen. Specify
checks whether the supplied answers are valid on the current analysis, but cannot tell
whether an in-range face number now denotes a different face. In particular, the plain v1
plate map above still writes on v2 after a groove splits bottom face 4 into faces 4 and 8:
it selects only face 4. It could equally have been handwritten for v2. Unchanged-geometry
plain maps remain accepted; Specify does not guess provenance from face numbers. `diff` no
longer carries answers to edited geometry (ADR P11): it reads the two files' PMI only.

This also applies to **released v0.1.8 and earlier `changes/answers.json` files**: they have no
`_specify` metadata. The literal old `changes/answers.json` repro still exits 0 with
`--accept-defaults`, because nothing in that file says an answer was dropped. Upgrading the
writer cannot recover their lost carry decisions.

## No export of answers

There is no export of answers as a record ([ADR P11](https://github.com/pzfreo/draftwright-project/blob/main/adr/P11-the-file-is-the-only-copy.md),
0.3.0): the written STEP file is the only copy of its answers. CLI `questions
--resolve-defaults`, serve and MCP `export_answers`, Rust `api::export_answers`, Python
`api.export_answers` and WASM `exportAnswers` are removed; the app's Answers (JSON) says "not
available in 0.3.0".

Answer maps written by earlier versions are still read wherever answers are taken (CLI
`questions --answers` and `write`, serve, Python, WASM, MCP `answers_file`). Their
`_specify.evidence` is checked, so a map whose evidence belongs to another revision is refused
as stale; a carry block in `_specify.unresolved` keeps its question open until it is answered;
`_specify.sources` is ignored.

## Answers to questions not asked

An answer can be valid in form while its question is not asked, given the other answers.
Dependent questions come and go: with `part.locating` `"± dimensions"` there is no
`datum.A.flatness`, `datum.B.control`/`datum.C.control`, `hole.position:*`, `hole.frame:*` or
`diameter.runout:*`, and with `"position tolerances"` no `hole.location:*`; a full-thread
depth is asked only of a blind tapped hole; a datum the file already states is not asked.
`part.process` leaves some part questions unasked (a printed part's `part.surface_finish`,
`part.edges`, `part.coating`; see [process.md](process.md)), and a bought part has no
`part.process`: these part answers lapse in the same way. Other `part.*` answers are read
whether or not they are asked. Which IDs are asked is the rules' question list, the same as `questions` returns.

Such an answer contributes no PMI, and is **never dropped silently**. `apply` adds a
faceless intent row `{"kind":"unasked","answer":ID,"faces":[],"not_written_reason":…}`, and
every write report (CLI, serve, Python, WASM) lists it in `not_written`. `preview` leaves
these rows out, so reviews do not pay for them; the editor's WASM write, which writes its
preview, adds them with Rust `rules::unasked_rows`:

```text
datum.A.flatness is not asked when part.locating is '± dimensions', so its answer was not written
```

Where switching `part.locating` would ask it, the reason says so; otherwise it says the
question is not asked of this part given the other answers, or that the file already states
the datum. `apply` does not refuse these answers: callers revising answers across a change
of `part.locating` keep both modes' answers, and the question takes its answer again when it
returns. The write does not store it (decision D11, with ADR P11: the file is the only copy, and
an answer to a question not asked leaves nothing in it): its `not_written` line is the report.
A carry block applies only while
its question is asked: a block on `datum.A.flatness` under `"± dimensions"` does not stop
`write` or bulk defaults, and is listed in `not_written` ("datum.A.flatness was
carried (carried answer could not be applied: …) but is not asked when part.locating is
'± dimensions', so nothing was written for it"). It stays in `_specify.unresolved`, so once
later answers ask the question again it shows as required and refuses again. Write derives
which blocks apply from its own answers on the part, not from the intent's rows.

Through a STEP, the write stores such blocks in the stored-answers payload's own
`unresolved` member (`{ID: reason}`), never inside its `answers`, which older builds would
refuse. Reopening restores them as the resumed answers' `_specify.unresolved`, so the block
applies again when its question returns. Older Specify and editor builds ignore the extra
payload member: they reopen and write the STEP normally, but without the block, so on them
an accepted default can replace it.

An unasked answer's value is checked against the question's options only while the question
is asked. `part.*`, `bom.*` and `extra.*` answers
are read whether or not asked, so they never appear here.

### Stored answers to questions not asked (D11)

A file an older build wrote may store such an answer, `datum.A.flatness` "none" under
`part.locating` "± dimensions", say. Reopening reports it **once** and leaves it out of the
resumed answers, as it does a stored answer whose requirement is no longer in the file: each is
listed in the analysis's `resume.dropped_answers`, `{"id", "value", "reason"}`, and named in a
warning:

```text
saved answer datum.A.flatness ('none'): its question is not asked when part.locating is '± dimensions'; it is not stored again
saved answer hole.function:holes:12 ('tapped:M5'): no requirement for this answer in the file; the question is open again
```

The reasons are "its question is not asked …" (given the other answers, as above; or "the file
already states this requirement", for a stored "none" where the file now states one) and "no
requirement for this answer in the file; the question is open again". The next save does not
store a dropped answer again: an intent made from that analysis (`apply`, `rules::unasked_rows`)
carries an `unasked` row for each dropped answer that would otherwise be kept, so the write lists
it in `not_written` ("saved answer datum.A.flatness ('none'): …; it was not stored again") and
leaves it out of the stored answers. Reopening the saved file reports nothing more.

### An answer that names nothing the part has (D11)

In answers given to `questions`, `apply`, `preview`, `review` and every write (CLI `write`,
serve, MCP `write_pmi`, the session's save, Python and WASM), an id that names a feature the
part does not have, or a kind that feature's family has no question of, refuses the **whole**
list, named with its reason:

```text
Invalid answers: hole.position:holes:999: unknown feature holes:999
Invalid answers: diameter.fit:holes:12: unknown answer ID for this feature family
```

The writer checks this itself, whatever intent it is given. It is never ignored. An id that names a real
question which is merely not asked given the other answers is not refused: it is reported in
`not_written`, as above.

An ID that **no** question can have, such as `bogus.question`, is refused with
`Invalid answers: bogus.question: unknown answer ID` by every entry point: Rust `api`,
CLI, serve, Python and WASM. `extra.*` requirements and the reserved `_specify*` members
remain accepted. Answers stored in a STEP by earlier versions, which kept such IDs verbatim,
are not refused on reopening: the unknown ID is left out of the resumed answers, listed in
`resume.unresolved` with its value, and named in an analysis warning
(`saved answer bogus.question: unknown answer ID: …`), so `questions` and `diff` still work.

## Reopening a file: the file is the only copy

[ADR P11](https://github.com/pzfreo/draftwright-project/blob/main/adr/P11-the-file-is-the-only-copy.md):
the file's PMI is the state, whoever wrote it. Reopening any file, whether Specify, another
tool or both wrote it, reads it the same way: `existing` and `part` are the file's whole PMI
and settings, and `analyse` adds the **feature table**, `table`:

- `rows`: one per feature and kind of requirement it can need, with the id of its question
  (`hole.position:holes:12`, the position of hole 12). Each is
  - `stated`: the file has a requirement of that kind on the feature's faces, whoever wrote
    it. `value` is its notation and `requirements` its items, with their characteristic
    numbers (`#N`). It is not asked.
  - `gap`: nothing is there. `suggestion` is Specify's default (`null` when there is none).
    It is asked. `open` is false when the suggestion is "none": leaving it and accepting
    "none" give the same file.
  - `decided_none`: an answer that nothing is needed, stored in the file; or an approval the
    planner wrote nothing for (a ⌀ left to the general tolerance on a datum's own faces: no size
    is written on a datum feature), stored and listed in the payload's `no_pmi` (resumed as
    `_specify.no_pmi`), its `value` the answer. The write report's `not_written` names it:
    "answer diameter.fit:turned_steps:0 ('general'): approved, but nothing of its kind was
    written on its feature (none is written on a datum's own faces; otherwise the planner
    skipped it); stored as decided, not asked again", so a planner gap stays visible. The
    check is per feature and kind (per direction for a location): a size written on a hole
    does not count for its position. Every route treats the stored decisions as answered: the
    command line's `questions` and `write`, `serve`, MCP, WASM and the page.
- `file_pmi`: the file's requirements on no recognised feature and kind. Each has an `id`,
  `file_pmi:#<entity>`, naming the STEP entity its characteristic number is (or would be) on
  (`file_pmi:#1397`), so it stays the same while the file is saved in place. An edit list
  removes one by it, numbered or not: `"items": {"remove": [{"id": "file_pmi:#1397"}]}`. A
  stated row is removed whole the same way, `{"id": "part.material"}`. (A note that is no
  thread, knurl or finish, and a setting on no row, have no id.)
- `decided_none`: the decided-none ids, read back from the file.

In an edit list, `fill` takes a general dimension row's displayed suggestion, and
`accept_suggestions` uses the same value by id. `none` records a decision on that row.
Once stated, the dimension can be removed through `items.remove` by row id or characteristic
number. An approved dimension the planner cannot write is reported and stays decided.

Quiddity's step levels along any part axis use the existing plane-to-plane length ids.
Independently measured plane lengths keep their rows. A stated location between retired
step levels appears under **Other PMI in the file** when it matches no measured row.
A face classified as supporting evidence for another feature gets no row of its own.
Each recognised fillet has a radius row
with “none needed” suggested, regardless of its size. To write a radius, fill the row with its
offered measured value, for example `{"fill":{"fillet.radius:fillets:7":"R0.25"}}`.

### What the file states, read back whole

The stated row is all anyone sees of what the file says, so nothing the file states is left
out of it. Each `existing` item, and so each requirement of a row, carries:

- `text`: the item as a drawing states it, in full: `⌀3.3 H11`, `⌀6.6 ±0.1`, `⌀6.5 - ⌀6.7`,
  `50 (basic)`, `⌖ ⌀0.3Ⓜ A`, `⌓ 0.02 A FⓂ`, `M2×0.4-6H ↧ 2`, `M5×0.8-6H THRU`,
  `diamond knurl, pitch 1 mm, major ⌀8 mm`, `√ Ra 1.6`, `datum A`. Values are as the file
  states them, in its own unit (an inch NIST file's `⌀0.25 +0.003/0`), shown as a drawing reads
  them: float noise beyond 10 significant digits rounded away (`⌀1.06500000000426` →
  `⌀1.065`) and trailing zeros trimmed (`0.016000` → `0.016`); a value with fewer digits is
  never rounded. `value`, `upper` and `lower` stay millimetres, as read. A dimension the file
  states without a value reads its kind in words (`Diameter, no value stated`). A row's
  `value` is its requirements' texts joined.
- the structured values a change needs to keep, by the planner's names (optional fields, absent
  when not stated):
  - a size's `fit` (its ISO 286 class, `H11`); a class stated alone also gives `upper` and
    `lower` from the ISO 286 tables (`⌀3.3 H11`: +0.075/0);
  - a tolerance's `diametral` (a ⌀ zone), `zone` (its zone form as stated), `mmc` (Ⓜ) and
    `modifiers` (every modifier as stated, schema names: `maximum_material_requirement`,
    `least_material_requirement`, `separate_requirement`, ...);
  - a thread's `side`, `designation` (`M2`), `spec` (`M2x0.4`), `pitch`, `class`, `hand`
    (`RH`/`LH`), `through`, `drill_diameter`, `drill_depth`, `full_thread`, `length`;
  - a knurl's `pattern`, `pitch`, `diameter`, `teeth`, `tooth_depth`, `root_fillet`,
    `helix_angle`, `hand`; a finish on faces' `finish` (`Ra 1.6`).
- `text_only: true` when the item states something those fields don't hold: a datum
  reference's modifier (`A FⓂ`), a common datum (`A-B`), a projected (`Ⓟ`) or non-uniform zone,
  a unit basis, a maximum, an unequal zone, all over, a dimension's qualifier, modifier or
  envelope requirement, a thread's minor or pitch diameter. Its `text` still has all of it; it
  just cannot be changed field by field. The feature table's requirement carries the same flag.

Two notes of one kind stay two items when they state different things (an M2 and an M3
thread), and are one item on all their faces when they state the same.

A requirement is matched to a feature and kind by the faces it sits on and its kind. Specify's
provenance marker plays no part. A hole the file locates by a location dimension (basic,
toleranced or general) has its location stated even when holes are located by position
tolerances: that setting only decides what is suggested for a hole the file does not locate.

A hole's location is read per direction (decision D8). A location dimension locates, along
the axis it measures (its reference face's normal; between two holes, the axis across both
whose offset is its value), every hole on its located side (the "to", `faces2` in `existing`),
and every other hole whose axis lies on the same line from the same reference: one 50 from the
left edge to a row of holes locates the Ø8 on that line too. Its reference side (the "from")
locates nothing: a dimension from hole A to hole B locates B, not A. A position tolerance on the
hole locates it in every direction. The location row is stated when every direction across the
hole's axis is located. Otherwise it is a gap, and when some directions are located, its
`missing` lists the others (`["y"]`), `located` those already located (`["x"]`), and
`requirements` the dimensions that locate it so far.
Its suggestion covers only the missing directions: answering it writes no position tolerance
(that would locate the hole a second time), only the general-tolerance location in the missing
directions, and the writer never adds a location along an axis the file already locates the
hole in. A datum hole is the origin the part is measured from: it is not asked its location in
the directions across its axis that no other datum plane measures. A hole that only appears on
a reference side, and is no datum, is asked.
Questions follow the table: every gap is asked, and nothing the file states is. A gap whose
suggestion is "none" is not open anywhere: not in `questions --open`, a save's still-open
list, `write`'s report or MCP. Approving its suggestion still decides it none.

A general-tolerance answer writes only its note (ADR P9, Paul's decision D1): approving
"ISO 2768-m" writes the general-tolerance note and nothing else. The dimensions the general
tolerance governs, each feature's size and location with its nominal alone, are written only
when the question they belong to is approved: a hole's or turned diameter's size with its
`hole.function` or `diameter.fit` answer, a hole's location with its `hole.position` or
`hole.location` answer. Until then those rows stay gaps with their suggestions.

A general dimension with no owning question is an optional row in its own right. For
example, the GRM-05 depth-lock bolt offers `14 ±0.2 from B` as `length:planes:2-3`.
Approve the displayed value by its ID, approve the suggestions together, or use
`write --accept-defaults`. Answering `none` records `decided_none` in the file. A file
that already states the measurement shows a `stated` row. If the general-tolerance note
is still a gap, approving the dimension writes its displayed tolerance on the dimension.

Plane lengths use `length:planes:<side>-<side>` with their two face groups in canonical
order. Unowned hole locations use `location:holes:<reference>-<located>`, with a planner
axis suffix for offsets between hole axes. Each face group lists its sorted face numbers
separated by dots. These IDs identify measurements within one geometry revision.

An older intent upgraded by the writer also needs an approved length between both supports.
The write report names any general dimension from the intent that the writer omits, with
its nominal, faces and reason in `not_written`.

Only answers that leave no PMI behind are stored in the file: "none" answers and choices that
are not requirements (`part.locating`). `resume.answers` holds those. Stored answers that
repeat the file's PMI (as Specify 0.2 stored every answer) are ignored on reading and dropped
at the next save. A stored answer whose PMI is no longer in the file leaves a gap: the deletion
stands, and the analysis warns about it once ("saved answer {id} ({value}): no requirement for
this answer in the file; the question is open again"). An answer that differs from a part
setting or process the file states is not written: the file's value stands whoever wrote it,
changing it is an explicit edit, and the write report lists it in `not_written`. If someone edited a requirement elsewhere, the
table shows the new value. There is nothing to conflict with, so there are no conflicts, no
keep and no `resume.present`, `resume.file` or `resume.foreign`.

A save never re-plans what the file states. A part with PMI is added to: every requirement in
the file keeps its bytes and its number, and only the stored answers and the
characteristic-number record are replaced. A requirement whose feature and kind the file
already states is not written again, whatever its value. It is reported in `not_written`
("the file already states it"), and changing it is an explicit edit. An answer to a question
the file states is not written, and the report says "the file already states this
requirement". A save that adds nothing keeps the file's bytes. Leaving an answer out removes
nothing.

## Specify metadata

The answer maps earlier Specify versions exported or `diff` returned (`answers.json`,
`report.answers`, and each assembly part's map) have a reserved `_specify` member. Preserve it when editing or storing answers.
Old plain files remain accepted by the new tools. New maps require updated Specify/editor
WASM builds: older builds reject `_specify` as an `unknown answer ID`. Use an updated build
to open and write these maps.

Example:

```json
{
  "part.material":"6061-T6",
  "_specify": {
    "evidence":{"schema":2,"face_count":12,"faces":"<saved face fingerprint>"},
    "unresolved":{"datum.A":"face 4 has no match in the new part"}
  }
}
```

`evidence` is the same `{schema, face_count, faces, face_names?, geometry?}` that Specify
stores alongside answers inside STEP and returns from WASM `evidence()`. It belongs to
the **destination** part's face numbering. Schema 3 may add authored names and geometry;
write requires the saved count, ordered face fingerprint and indexed authored names
to match, without remapping by name. This deliberately refuses changed numbering even if names could be rematched.
Malformed evidence is an error.

The shared writer checks evidence against the loaded part before writing bytes. CLI
also checks before saving `--intent`; stale evidence exits 2, names the answers path,
and says they belong to another revision of the part.
An unchanged part's evidence remains valid even if its file bytes change without changing
faces. Existing in-STEP answer payloads retain their format and evidence; transport
metadata is consumed before answers are embedded.

`unresolved` contains blocks only for revision-stable IDs: `datum.*`, `part.*`
(including general tolerance), `bom.*` and added requirements' `extra.*`. Their entries
preserve diff's exact refusal. An `extra.<name>` key is the person's own name for a
requirement they added, with the same meaning in both revisions; dropping it on write
because its faces did not carry would silently lose a requirement no question asks for
again. Answer it again on the new part's faces, or remove its entry to drop it.
A dropped answer for one of these questions contributes no PMI and cannot be replaced by
bulk default acceptance. CLI fails with
`incomplete: datum.A (carried answer could not be applied: <reason>)`, writes no STEP or
intent output, and preserves any existing output. Serve `accept_defaults`, Python
`apply(..., accept_defaults=True)` and WASM `acceptDefaults` use the same block.
The shared writer also refuses these blocks when called directly with a preview intent.

Feature-scoped IDs such as `hole.function:holes:7` encode old face numbers: after removing
a hole, `holes:7` may already refer to a different new hole. A failed carry has no unique,
verified destination question. These answers remain in **the diff report's `unresolved`
list**, with their original value and reason, but do not create sidecar blocks under old
IDs. Never use an
old feature ID as a clearing answer for a different hole. Review the reported removal or
unmatched requirement, and answer the new part's questions using its current geometry.
Earlier metadata that contains feature-scoped blocks is ignored for blocking, so it cannot
shadow a reused question ID or require an answer to a feature that no longer exists.

`questions` and review responses mark a blocked stable question as `attention: "required"`,
remove its acceptable suggestion (`default: null`), and put
`carried answer could not be applied: <diff reason>` in the existing `reason` field. This
presentation is shared by CLI, serve, Python and WASM. Review can still show partial
answers; the editor's automatic defaults skip blocked IDs.

A block whose question is not asked under the current answers does not refuse; see
[Answers to questions not asked](#answers-to-questions-not-asked).

Supply an explicit valid answer for that ID to acknowledge its refusal (for the split
bottom, `"datum.A":[4,8]`). This overrides its unresolved entry; future diff output removes
the resolved entry. Stable carry blocks survive repeated diffs until acknowledged. If a
question has disappeared entirely, review its removal and explicitly remove its
`unresolved` entry. Removing all metadata discards these protections.

WASM `rematchAnswers` also reads embedded evidence when no separate evidence argument is
given. For maps with `_specify`, it returns fresh evidence and carry blocks for its
unresolved results, so saving the returned answers preserves the protection.

Both metadata fields are optional so callers may keep a carry block without evidence,
or attach WASM `evidence()` to a manually managed map. Do not fabricate evidence or copy
it from a different part. Assembly maps keep metadata inside each part's answer map.
