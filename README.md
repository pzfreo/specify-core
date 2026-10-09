# specify-core-rust

A pure-Rust port of [specify-core](https://github.com/pzfreo/specify-core): interview-driven
AP242 PMI authoring for STEP parts. It recognises a part's features, works out what
manufacturing intent is missing (datums, fits, threads, positions, finish, the general
tolerance and the dimensions that need their own), proposes a default for each with the reason
it was chosen, and writes the answers back into the STEP as AP242 semantic PMI, verified by
reading the file back. No OpenCascade and no Python: features come from
[quiddity-rust](https://github.com/pzfreo/quiddity-rust)'s recognisers, and STEP reading,
meshing input and PMI reading and writing from its haecceity kernel.

## Status

Every module of specify-core is ported (part loading, features, existing PMI, the rules,
mates, meshing, anchors, the writer with resume and assemblies, the API, the command line and
the JSON-lines service) and checked against outputs captured from the Python implementation.
Every difference from Python carries a verdict and its evidence, listed in
[docs/known-differences.md](docs/known-differences.md) (generated from
`tests/fixtures/known_differences/`; its "Open work" section lists every difference that is a
known bug or unsettled). What is verified per module, the stand-ins used for what quiddity-rust
does not yet provide, and what remains are in [docs/design.md](docs/design.md), "Status"; the
needs from quiddity-rust are in [docs/upstream-needs.md](docs/upstream-needs.md) and the
decisions waiting for the maintainer in [docs/questions.md](docs/questions.md).

As of stage 5 (consolidation, 2026-10-09) all five planned stages are on `main`. The full suite
(302 tests, with the quiddity corpus) passes. quiddity and haecceity follow quiddity-rust's
branch `prototype/rust-port` (Cargo.lock pins 5798c59; question Q2).

The two engines' STEP files are not yet interchangeable: OpenCascade (and so specify-core's
Python) cannot open a Rust-written file with datums (Q16), and neither engine resumes answers
the other stored (Q14). Draftwright Specify can drive either engine through the same command
line and JSON formats, one engine per part.

## Build

```bash
cargo build --release          # target/release/specify-core
```

Rust 1.99 (`rust-toolchain.toml`).

## Use

The command line is specify-core's: the same subcommands, arguments, JSON files, stdout
summaries and exit codes (0 done, 1 a write that did not verify or a file that cannot be read,
2 refused: unanswered questions, an invalid answer, or bad arguments).

```bash
specify-core analyse part.step -o analysis.json            # faces, features, existing PMI
specify-core mesh part.step -o part.mesh.json               # triangles by face index, for a picker
specify-core questions analysis.json --answers a.json --open  # what is still unanswered
specify-core write part.step analysis.json a.json -o out.step --accept-defaults --intent intent.json
```

`a.json` maps question id to value, e.g. `{"part.material": "6082-T6",
"hole.function:hole_patterns.grid:…": "fit:H7"}`. `write` refuses until every question has an
answer (or a default, with `--accept-defaults`), then writes AP242 into the original document
and verifies it by reading it back; the report (`output`, `written`, `not_written`,
`warnings`) is printed as JSON, and the answers are stored in the file so a later `analyse`
resumes them.

An assembly is specified a part at a time. `specify-core parts asm.step` lists its distinct
parts (a part placed twice is one part, with two placements); `analyse` and `mesh` take
`--part N`; and `write` takes an analysis and answers for each part, writing them all into the
assembly at once, each part's PMI on its own product definition:

```bash
specify-core parts asm.step
specify-core analyse asm.step --part 0 -o plate.json
specify-core analyse asm.step --part 1 -o pin.json
specify-core mesh asm.step --part 1 -o pin.mesh.json
specify-core write asm.step plate.json plate-a.json pin.json pin-a.json -o out.step --accept-defaults
```

Writing an assembly specify-core wrote replaces its PMI, so every part with answers stored in it
must be written again together.

`specify-core serve` answers JSON-line requests on stdin for a host process, one reply per line
on stdout (ops `parts`, `analyse`, `questions`, `preview`, `interpret`, `write`; errors
`incomplete`, `invalid`, `failed`), as specify-core's `serve.py` does; the protocol is in
`src/serve.rs`.

```bash
echo '{"id": 1, "op": "analyse", "step": "part.step"}' | specify-core serve
```

### Where the Rust engine differs

Each of these is recorded with its reason (docs/known-differences.md, docs/questions.md):

- **Part notes and surface finish are not written** (the surface-finish default, coating, heat
  treatment, edge note, and a finish on faces): haecceity has no standard AP242 form for notes
  yet. Each is listed in the report's `not_written` with that reason and the write succeeds
  (Q3, upstream need U3).
- **Knurls are not written**, diamond or straight: AP242's `turned_knurl` needs parameters the
  intent does not state (a helix angle for a diamond knurl; at least three of teeth, tooth
  depth, root fillet, helix angle and hand for any knurl). Listed in `not_written` (Q4, Q15).
- **An assembly that already has PMI from elsewhere is written** (by haecceity's add mode,
  keeping every original byte), where Python refuses it because of OpenCascade; pending the
  maintainer's confirmation (Q6).
- **Feature ids can differ from Python's** for the same feature (quiddity-rust reports a
  feature by its defining faces, Python's quiddity by its constituent faces, and some families
  differ), so question ids and answer keys do too. An analysis and its answers belong together;
  analyse and write with the same engine (Q1, upstream needs U2, U12).
- **A gzipped STEP file is refused** ("the file is gzip-compressed; decompress it first", exit
  1), as specify-core reads only STEP text.
- A file that cannot be read, or an assembly analysed without `--part`, is an `error:` line on
  stderr (exit 1) where Python prints a traceback; `write`'s stdout is only the report, without
  OpenCascade's transfer statistics; meshes are triangulated by a local stand-in (U4) with about
  1.8 times Python's triangles (Q12); a product with surface models beside its solid (NIST
  CTC-02, 04, 05) is one part, not OpenCascade's several (Q7).

## Tests

```bash
cargo test --release
SPECIFY_CORPUS=/path/to/quiddity/tests/corpus cargo test --release   # with the corpus comparisons
```

Tests compare the port with fixtures captured from specify-core (`tools/capture.py`, run in
specify-core's environment); tests whose input is a quiddity corpus file skip without the
corpus. `docs/known-differences.md` is regenerated after an area file changes with
`UPDATE_KNOWN_DIFFERENCES=1 cargo test --release --test known_differences_doc`.

## Licence

AGPL-3.0-or-later; see [LICENSE](LICENSE) and [NOTICE](NOTICE).
