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
editor Cargo.lock before integration. No commit or push was made in this stream.

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
