# Authored STEP face names (For #87)

Code-first CAD authors can give faces persistent names in their source model.
Specify reads only the names actually carried by STEP representation items,
through haecceity v0.4.3's `json::Anchors::read(doc, definition).names`. That API
returns one optional string per definition face, in Specify's face index order.
Empty STEP names become `None`. Specify never derives names. For newly saved
identities it accepts only a name in cadgen's explicit `f:` namespace (with a
nonempty remainder) that occurs on exactly one face of the part. The rest of the
string is opaque and stored unchanged. Cadgen's `f:pmi_plate.body_make_box1.5;M:pmi_plate.body` is one example.
OCCT exports XCAF sub-shape names with `write.stepcaf.subshapes.name = 1`.

Analysis and WASM snapshot face objects add optional `name: string` only for those
usable identities. Shared names, numeric feature-class labels and generic CAD names
are omitted, even when a generic label happens to occur once. Uniqueness alone is
insufficient: MFCAD++ has singleton class labels such as `7`, and NIST has a unique
`Fill.1` beside repeated `PartBody` labels. Other naming conventions currently use
the pre-existing geometric path; no identity is inferred from their spelling.
Mesh picking still uses the same integer face index. The full Python oracle face
comparison is unchanged: generic label metadata is not part of the analysis, so
there are no new differences to exempt or hide in its projection.
The native loader caches `LoadedPart.face_names` once, using the same haecceity
reader as its PMI JSON layer. Rust callers constructing a `LoadedPart` themselves
must populate that field from `Anchors::read`. There are no new CLI commands,
WASM methods or Python functions. `AnalysisFace` and `AnswerEvidence` in the
TypeScript declarations describe the existing methods' JSON strings.

## Stored answers and compatibility

All newly saved payloads now write stored-answer **schema 3**, using Quiddity's own versioned
geometric fingerprints (`geometry`, carrying `fingerprint_version`, faces, and features) instead
of Specify's legacy SHA-256 face hash (`faces`). A part with any usable unique `f:` names adds
`face_names`, an array of the saved STEP names in face index order, with `null` for an unnamed,
shared or generic-labelled face. This table records the name of every face used by a feature
answer, datum or extra without changing answer keys or values. The existing `evidence()` returns
the same evidence fields without `answers`/`part`. Keep the entire evidence object beside answers
saved outside STEP.

Unnamed and mixed parts store `geometry`, quiddity's versioned geometric fingerprints,
so exact matching, `correspond` rebinding, and `0.01 mm` tolerant recovery can match their unnamed
supports across revisions and kernel upgrades. Parts whose faces all have usable identities need
no geometric correspondence and omit `geometry`. Parts with no usable identities write schema 3
with `geometry` and no `face_names`. No name is refused at write time merely because it is shared.

This engine reads schemas 1, 2 and 3. Legacy schema 1/2 payloads (carrying Specify's `faces`
hash) stay readable, with pre-#317 projected-boundary recovery on mismatch, and are rewritten as
schema 3 on the next save. When a stored payload's fingerprint does not match and is not
recovered, Specify never drops it silently: it reports `"Saved answers in this file were not
reused: the part has changed since they were saved"` in analysis warnings (CLI/serve/MCP),
`analysis.resume_warning`, and review `problems` (`code: "saved_answers_not_reused"`).
With verified evidence, both analysis and file-only PMI scenes
rematch saved feature answers to current IDs, including a uniquely added planar floor.
Without face evidence (plain projects/answer files and unverified schema-1 payloads),
matching prefers an exact ID, then a unique same-family feature whose ID faces are a
strict superset of the saved ID faces. Ambiguous supersets and family changes remain
unresolved with their original values. These matches remain marked unverified.
A previous engine accepting only schemas 1/2
**skips a schema 3 stored-answer item entirely** under its existing version rule:
it does not recover a subset of the answers. It can still read the standard AP242
PMI as existing PMI to the extent its PMI reader supports it; automatic interview
resume is unavailable. Passing schema 3 evidence to an older `rematchAnswers`
returns its existing invalid-schema error. There is no claimed downgrade or
cross-version round trip for schema 3 answers. Unknown future stored schemas are
still skipped by this engine.

## Matching and refusals

Resume, `rematchAnswers`, and revision `diff` resolve each stored name exactly.
A stored name must occur on exactly one face in both saved and current revisions.
The current revision's raw names are checked, so a previously unique stored name
that becomes shared is still an ambiguity; it cannot be erased by the eligibility
filter and then re-matched by geometry. A missing name leaves the answer in `unresolved` with
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
