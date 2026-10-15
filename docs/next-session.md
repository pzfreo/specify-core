# G2: assemblies on haecceity's part API (slice 2, specify-core-rust #2; 2026-10-15)

**Pin.** quiddity and haecceity are pinned by `rev = "0d601d207495dea60b1b6a7109bd3abd109bf745"`
(quiddity-rust main after haecceity's assemblies slice 1), not a tag. **The next tagged
quiddity-rust release replaces this rev pin** (Cargo.toml says so too).

**What changed.**
- `part::File` reads a file once (`StepFile::read`) and loads any part (`File::load(Some(k))`,
  `load_all`) with `StepFile::part`. The U1 and U9 stand-ins (`extract_part`, `part_names`, the
  placed-back read, the rebuilt path) are deleted; `upstream-needs.md` marks U1 and U9 met.
  `LoadedPart` keeps every field (the editor builds one with a struct literal); `rebuilt` is
  always false from this loader.
- `parts` reports `product_id` after `name` (D1: the index `part` stays the identifier). This is
  an addition to Python's listing; the CLI sweep records it in `known_differences/cli.json`.
- `api::analyse_file`, the CLI's and serve's `analyse` read the file once for the part and the
  other parts its mates need (`api::analyse_reporting_in(&file, &loaded)`); `api::analyse` of a
  part of an assembly loads the others from `loaded.bytes`, never from its path.
- `api::write_bytes(bytes, jobs) -> (bytes, report)` writes any parts of a file held in memory
  (the editor's browser build; it can drop its writer fork). `writer::write_parts_bytes` is the
  writer's form. The old single-part `writer::write_bytes` is unchanged.
- D3: one part of an assembly may be written alone; the other parts, their PMI and stored
  answers are left byte for byte. `tests/assembly_write.rs`
  (`writing_one_part_again_alone_keeps_the_others`) replaces the refusal test.
- D4: an assembly with no part chosen still refuses, naming its parts.

**Editor additions (backwards compatible).** New: `part::File` (`read`, `of_bytes`,
`of_shared`, `load`, `load_all`, `parts`, `definitions`, `sha256`), `PartSummary.product_id`,
`api::analyse_reporting_in`, `api::write_bytes`, `api::IN_MEMORY`, `writer::write_parts_bytes`.
Nothing was removed or changed in signature. To open part `k` of an assembly from bytes:
`File::of_bytes(Path::new(name), bytes)?.load(Some(k))`, then `api::analyse` / `api::mesh`; to
write: `api::write_bytes(bytes, &[(intent, Some(answers))])`.

**Overturnable judgement calls.**
- Write mode is now decided over the parts written, not the whole file: Replace when every part
  written is bare or its stored answers resume, else Add. For a file of one part this is the old
  rule. In an assembly it differs in two cases: a bare part beside a part with PMI from elsewhere
  is now Replace (answers stored; was Add, no answers), and a part with PMI from elsewhere beside
  a part with stored answers is now Add (was Replace, which would have removed the foreign PMI).
- `api::write_bytes` takes `(bytes, jobs)` as specified, so its report's `output` and errors use
  the name `api::IN_MEMORY` ("<in memory>") for the path.
- The acceptance item "`analyse --part 1` runs one `Parsed::new`" is met for part loading (one
  `StepFile::read` for the part and its mates, was two `read_part_definitions`/`read_step` parses,
  one `p21` parse, and the same again for the mates' `load_all`), but not for the whole
  analysis: `existing::read` still calls `read_part_definitions` (and `resume::stored` parses the
  `p21` document) on the part's bytes. Changing that needs `LoadedPart` to carry the file, which
  would break the editor's struct literal, or a change to `existing.rs` outside this stream.
- `tests/part.rs` and `tests/features.rs` tests of the rebuilt stand-in were rewritten to assert
  the new behaviour (each part is haecceity's own `Part`, checked against its first instance in
  the whole read; an unresolved face is recorded in the part's own `Part`, so it is not
  degraded; a caller-rebuilt part is still refused by recognition).

# D2: additive typed question presentation (2026-10-11)

`rules::Question` now carries an optional feature row `title` and a typed
`Characteristic` (`flatness`, `perpendicularity`, `parallelism`, `position`,
`runout`). Hole/turned rules build the title from their existing diameter,
count and group data. Hole positions, turned runout and datum controls set the
characteristic while choosing the requirement, independently of prompt wording.
`Question::to_presented_dict()` exposes those fields alongside the existing
record. The editor intent adapter uses it; review titles and zone explanations
consume the fields instead of regex-reading prompts.

Overturnable judgment: keep `to_dict()`'s exact Python-compatible JSON unchanged,
including key order/omissions and all oracle comparisons. The additive presentation
serializer is explicit opt-in. A future version can make the new fields the default
wire representation. The enum includes parallelism for the presentation vocabulary;
the current automatic datum-control rules do not invent it when there is no valid
control. Missing metadata stays absent, rather than guessed from the prompt.

Two `tests/question_presentation.rs` tests use committed plate/spool analysis
fixtures to exercise actual rules and serialization after changing/clearing the
prompt. They failed to compile before the change (10 missing enum/field/method
errors), and pass after it. No existing test or fixture was changed.

The editor uses a one-line development path dependency into this worktree. The
integrator must replace it with this repo's pushed git rev and regenerate the
editor Cargo.lock before integration. The stream was committed as 5cfcdbc and merged to main.

### Files changed by D2

```text
docs/next-session.md
src/rules/datums.rs
src/rules/holes.rs
src/rules/mod.rs
src/rules/turned.rs
tests/question_presentation.rs
```

### Final verification

All required Specify checks pass: `cargo fmt --check`,
`cargo clippy --all-targets --release -- -D warnings`, and `cargo test --release`.
The full suite reports **324 passed, 0 failed, 0 ignored across 41 result groups**,
including the two new presentation tests. Both `QUIDDITY_CORPUS` and
`SPECIFY_CORPUS` were `/Users/paul/repos/quiddity/tests/corpus`; the committed
Python-oracle fixtures were used unchanged. Commands used `CARGO_BUILD_JOBS=8`
and `RUST_TEST_THREADS=8`. Only one Cargo command ran at a time. Existing exact
oracle checks, including the complete CLI and write sweeps, pass without fixture
updates or relaxed comparisons. `git diff --check` passes; the staged diff is empty.
