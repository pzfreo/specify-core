# Questions for the maintainer

Decisions the port does not make on its own. Each says what is being done meanwhile, so no work
waits on it.

**Q1. Analyses across engines.** Face numbering is the same in both engines on every file
checked so far, so the Rust binding keeps `loader_version` 2 and an analysis from either engine
can be written by the other. Feature ids can still differ (constituent against defining faces,
U2), so answers keyed by feature id are only meaningful with the analysis they were given
against, which is how specify-core already works. Should Draftwright Specify be able to mix
engines (analyse with one, write with the other), or should the Rust engine mark its analyses
(e.g. a distinct `loader_version` or an `engine` field) so a mix is refused? *Meanwhile:*
`loader_version` 2, with a test pinning face-by-face equality on every fixture.

**Q2. Moving to `prototype/rust-port`.** Once it contains a16d7af, the dependency moves there,
where recognition is framed (`recognise_placed`: records found in the part's own frame and
reported in the file's coordinates). Should specify-core-rust use the framed recognition (as
quiddity-rust's own default now is) or the caller-space `correspondence::recognise` it uses on
`ap242`? *Meanwhile:* caller-space, the only one on `ap242`.

**Q3. Notes and surface finish.** haecceity writes no notes (its decision 5: standard forms
only, none chosen). Part notes (surface-finish default, coating, heat treatment, edges) have
defaults, so nearly every write has some. *Meanwhile:* they are listed in the report's
`not_written` and the write succeeds. Should such a write instead be refused until a standard
form exists, or should the Rust engine stop asking those questions?

**Q4. Diamond knurls.** `turned_knurl` requires a helix angle for a diamond knurl (WR9); the
intent has pattern, pitch and diameter only. *Meanwhile:* straight knurls are written (major and
nominal diameter both the turned diameter, diametral pitch the intent's pitch); diamond knurls go
to `not_written`. Should a standard helix angle (e.g. 30°) be assumed and stated in intent?

**Q5. CI and the corpus.** quiddity-rust's CI checks out pzfreo/quiddity's `tests/corpus` at a
pinned revision. This repository's CI runs only committed fixtures (corpus-backed tests skip).
Should it check out the corpus too, so the corpus comparisons run in CI?

**Q6. Assemblies with PMI from elsewhere.** Python refuses to write an assembly that already has
PMI it did not write (an OpenCascade limitation). haecceity's add mode can write it, keeping
every original byte. *Meanwhile:* written by add mode, recorded as a deliberate difference.
Confirm, or keep Python's refusal.

**Q7. A product with surface models beside its solid.** NIST CTC-02, 04 and 05 (RD/RC and the
AP242 editions of 02 and 04) give one product a solid and one or two `SHELL_BASED_SURFACE_MODEL`s
in separate shape representations. OpenCascade's XCAF makes a part of each, so specify-core
lists CTC-02 AP242 as three parts (`SOLID` 635 faces, `SHELL` 1, `SHELL` 1); haecceity has one
part per product definition, so the Rust engine lists one part of 637 faces: the solid's 635 at
Python's indices, then the two surface faces. The binding's `face_count` differs (635 against
637), so an analysis of these files does not cross engines, and Python's parts 1 and 2 do not
exist in Rust. Should the Rust engine keep one part per product (PMI attaches to the product),
split as XCAF does, or number the solid alone and leave the surface models out?
*Meanwhile:* one part per product, faces solid first; the live comparison in `tests/part.rs`
checks the one part's faces against Python's parts' faces in turn.

**Q8. Diameters over 500 mm.** ISO 286 is tabulated here up to 500 mm, but specify-core still
offers fits for a hole or turned diameter above it: `choices.describe` then calls `callout`, whose
`fit_limits` raises `ValueError: nominal size 600.0 outside 0-500 mm`, so `questions` fails for
the whole part (checked with Python: `describe('hole.function:x', ['general', 'fit:H7'], 600.0)`).
Should fits not be offered above 500 mm (the general tolerance only), or should ISO 286 be
extended to 3150 mm? *Meanwhile:* Python's behaviour is kept: `choices::describe` returns the
same message as an error, which its caller reports (no part question list is made up).

**Q9 (rules stream). Malformed input Python crashes on.** A few answers or analysis records make
specify-core's rules raise something other than `ValueError`: an `AttributeError` or `TypeError`,
which serve reports as `failed` with a trace (a hole's purpose answered with a non-string while a
position is asked: Python calls `.partition` on it before `choices.check` runs; a datum answer that
is a bare number), or a `KeyError`, which serve reports as `invalid` with only the key as message
(a tapped size that is not tabulated, `'M7'`; a record missing a key the rules read). None occurs
in the oracle. *Meanwhile:* the Rust rules refuse each as `Error::Invalid` (serve's `invalid`)
with a message naming the answer or record; for a hole's purpose, the message `choices.check`
gives once it gets that far. Confirm, or should such input keep failing as `failed`?

**Q10. Face numbering that differs from OpenCascade's (bears on Q1).** On corpus
`inventory_refusal/13975.step` the two engines number the same 17 faces in different orders: Rust
in the `CLOSED_SHELL`'s order (#17, #145, #216, ...), Python in the order OpenCascade's reader gives
(#17, #216, #752, #1166, ...; why its shell differs from the file's order was not established); every face agrees entity for entity. Every other part the
features sweep compares (119) numbers alike: its remaining face differences are integration
noise (U11) or Q7's extra faces. An analysis of such a file made by one engine and written by the other
would put PMI on the wrong faces, and nothing in the binding detects it (same `source_sha256`,
`face_count`, `loader_version`). Should the binding carry something that detects this (e.g. a
hash of the face entity ids in order), or is Q1's answer enough? *Meanwhile:* recorded as an
`undetermined` known difference (`features.json`); no change to the binding.
