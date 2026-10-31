# Authored STEP face names (For #87)

Code-first CAD authors can give faces persistent names in their source model.
Specify reads only the names actually carried by STEP representation items,
through haecceity 0.2.8's `json::Anchors::read(doc, definition).names`. That API
returns one optional string per definition face, in Specify's face index order.
Empty STEP names become `None`. Specify does not derive names or interpret their
syntax. Cadgen's `f:pmi_plate.body_make_box1.5;M:pmi_plate.body` is one example.
OCCT exports XCAF sub-shape names with `write.stepcaf.subshapes.name = 1`.

Analysis and WASM snapshot face objects add optional `name: string`. An unnamed
face omits that property; mesh picking still uses the same integer face index.
The native loader caches `LoadedPart.face_names` once, using the same haecceity
reader as its PMI JSON layer. Rust callers constructing a `LoadedPart` themselves
must populate that field from `Anchors::read`. There are no new CLI commands,
WASM methods or Python functions. `AnalysisFace` and `AnswerEvidence` in the
TypeScript declarations describe the existing methods' JSON strings.

## Stored answers and compatibility

A part with any named faces writes stored-answer **schema 3**. Its payload keeps
`answers`, `part`, `face_count` and `faces` (the existing unsigned-zero geometry
hash), and adds `face_names`, an array of the saved STEP names in face index order,
with `null` for an unnamed face. This table records the name of every face used
by a feature answer, datum or extra without changing answer keys or values. The
existing `evidence()` returns the same evidence fields without `answers`/`part`.
Keep the entire evidence object beside answers saved outside STEP.

Mixed parts also store `geometry`, quiddity's versioned geometric fingerprints,
so the existing correspondence algorithm can match their unnamed supports across
revisions. Entirely named parts need no geometric correspondence. Entirely unnamed
parts continue to write schema 2, with byte-identical payloads and analysis fields.
Schema 3 uses schema 2's hash algorithm; no new fingerprint namespace is invented.

This engine reads schemas 1, 2 and 3. Schema 1/2 resume retains its existing
fingerprint and legacy-ID rules. A previous engine accepting only schemas 1/2
**skips a schema 3 stored-answer item entirely** under its existing version rule:
it does not recover a subset of the answers. It can still read the standard AP242
PMI as existing PMI to the extent its PMI reader supports it; automatic interview
resume is unavailable. Passing schema 3 evidence to an older `rematchAnswers`
returns its existing invalid-schema error. There is no claimed downgrade or
cross-version round trip for named answers. Unknown future stored schemas are
still skipped by this engine.

## Matching and refusals

Resume, `rematchAnswers`, and revision `diff` resolve each stored name exactly.
A unique name must occur on exactly one face in both saved and current revisions.
A missing name leaves the answer in `unresolved` with
`face name "…" not in this revision`; a duplicate in either revision leaves it
unresolved with `ambiguous face name "…": used on several faces …`. Diff also
sets its refusal class to `unresolved` or `ambiguous`, respectively. Neither case
falls back to geometry, even when face geometry, indices or feature IDs agree.
The unresolved record retains the original answer for recovery.

Only a face whose saved name is `null` uses geometric evidence: verified unchanged
indices, otherwise quiddity's saved correspondence for mixed parts. Without usable
geometric evidence it remains unresolved. An incompatible future quiddity
fingerprint version leaves unnamed supports open while exact names still work.
A multi-face answer must resolve every support. Datum settings follow an unresolved
datum; extras map both `faces` and distance `reference` lists. Feature answers
then require a unique same-family feature containing the mapped support. Colliding
answers remain refusals, and automatic-answer IDs follow successful migrations.

Existing foreign CAD PMI comparisons retain their geometric support matching; the
name refusal policy applies to saved answers and extras.

The CLI's analyse/write/diff, Python's analyse/write/diff, and WASM session methods
all use these core paths. Names persist only as long as the author keeps them
stable and unique; renaming a face deliberately breaks its saved binding.
