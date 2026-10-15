# S3: thread choices beside fits and knurls (2026-10-15, issue #3)

A turned diameter's `diameter.fit:` question now offers every ISO 261 thread at its size:
`standards::external_threads(d)` gives the coarse thread as the bare size (`M10`, the spec
Python's answers already use) then each fine pitch (`M10x1.25`). `choices::code` names a fine
one `M10×1.25 thread`, `choices::callout` `M10×1.25-6g`, and `turned_requirement` writes
designation `M10`, pitch 1.25, spec `M10x1.25`, class 6g, as the writer already takes it.
The free-text box (`choices::interpret`) reads a typed `M10x1.25`/`M10×1.25` on a Ø10 as the
same `thread:M10x1.25`; `tests/thread_choices.rs` writes both routes and reads the thread back.

Finding: on the base commit the spindle's Ø10 question already offered `thread:M10`
(M10×1.5-6g), as Python does for its 13 coarse sizes. The spindle's Ø15 offered no thread,
because Python's table has no M15 (ISO 261 gives M15 in fine pitches only, M15×1.5 and
M15×1). That is probably the question the issue saw; it now offers both.

Editor (session API): additive only. New option values `thread:<size>x<pitch>` appear in
`options` and `choices` (each with `purpose: thread`, label, code and callout), so the
editor's purpose buttons show a thread group with several codes. Old answers are unchanged.

Overturnable judgement calls:

- The table is ISO 261:1998 Table 1, M1.6 to M64, all three choice columns (third-choice sizes
  such as M15×1 and M17×1 are bearing locknut threads on spindles). Left out: the bracketed
  M30×3 and M33×3, and M14×1.25 (spark plugs only). Written from the standard's table as
  recalled, not checked against a copy of it. ISO 262's selected sizes would be a narrower
  choice if the button row is too long on large diameters (M42 to M64 offer five threads).
- Holes, tapped-hole choices, mates and `thread_size` are unchanged (Python's coarse table).
- The coarse pitch stays the bare size (`thread:M10`), not `thread:M10x1.5`, so existing
  answers and Python's oracle keep their values. A typed `M10x1.5` reads as `thread:M10`.
- A typed thread departs from Python only when ISO 261 gives it at that diameter's size;
  any other is refused with Python's message. At a matching size, a pitch ISO 261 does not
  give (`M15x2`) and a bare fine-only size (`M15`) are refused with new messages that list
  the pitches. The Python grid comparison (`tests/choices.rs`) accepts exactly those
  departures: Python refused, Rust read an ISO 261 thread of that diameter's size.
- No known-difference entries. The rules and CLI oracle sweeps (`tests/rules_oracle.rs`,
  `tests/cli.rs`) drop from Rust's `diameter.fit` questions, before comparing, exactly the
  thread options and choices Python cannot offer (a fine pitch, or a coarse size not in
  Python's table: `common::without_threads_beyond_python`); everything else in those
  questions must still equal Python's. A CLI stdout holding such threads keeps its value
  comparison but skips the byte-for-byte text check.

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
