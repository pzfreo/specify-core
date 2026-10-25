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

**0.1.0-alpha.8 (2026-10-25)**; see [CHANGELOG.md](CHANGELOG.md).

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
(376 tests, with the quiddity corpus) passes, last on 2026-10-20 at quiddity-rust's alpha.8.
quiddity and haecceity (including WASM) are pinned to quiddity-rust's `v0.2.0-alpha.2` tag
(`ff1975ef`). Recognition publishes schema 6: every record is in file coordinates, with axis
fields as letters or unit vectors, and Specify reads geometry only from the published
records (the recognition frame is provenance). Saved STEP answers
are re-matched on reopening, with unresolved old IDs and values reported for review; see
[the upgrade rule](docs/design.md#saved-answers-across-kernel-upgrades).
Draftwright's released kernel pin temporarily differs; `tools/check_quiddity_pin.py`
checks the release pins and reports that expected mismatch as an advisory warning.
This includes derived PMI geometry and measured modelled threads, turned-diameter thread
choices and the editable full-thread depth of blind tapped holes.

OpenCascade (and so specify-core's Python) opens every Rust-written file and reads its datums,
sizes and tolerances, on one face or several (Q16, Q20; `tests/cross_engine.rs`); it reads a fit
without its limits, no thread and no surface finish, as specify-core's reader asks only for its
own forms of those. Answers the Rust engine stores (stored-answer schema 2, Q14) resume in
specify-core from its branch `fingerprint-v2` on, which writes schema 2 too; files the older
specify-core wrote (schema 1) do not resume in Rust. Draftwright Specify can drive either engine through the same command
line and JSON formats, one engine per part.

## Build

```bash
cargo build --release          # target/release/specify-core
```

Rust 1.99 (`rust-toolchain.toml`).

The workspace also builds **`specify-core-wasm`**, a browser ES module with TypeScript
bindings. Published `v*` releases attach `specify-core-wasm-<version>.tgz` (containing `pkg/`)
and a SHA-256 checksum; there is no npm publication. Its `IntentEngine` opens STEP bytes,
returns JSON snapshots/reviews and writes verified PMI with an optional embedded editor
project or named application payload, which it reads back with the original part. It also
re-matches answers saved outside the STEP (`rematchAnswers`). `IntentEngine.acceptDefaults` explicitly accepts all suggested defaults as editable
answers. Free `acceptDefaults`, `review` and `interpret` functions also take JSON strings. See
[docs/wasm.md](docs/wasm.md) for installation, the API, build/parity checks and editor migration.

```bash
wasm-pack build crates/specify-core-wasm --target web --profile wasm-release --no-opt --locked
```

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

Writing a part again replaces its PMI and answers. One part may be written alone (`write asm.step
pin.json pin-a.json -o out.step`): the other parts, their PMI and their stored answers are left as
they were. `parts` reports each part by its index (`part`, what `--part` takes) with its `name` and
`product_id` beside it.

`specify-core serve` answers JSON-line requests on stdin for a host process, one reply per line
on stdout (ops `parts`, `analyse`, `questions`, `preview`, `interpret`, `write`; errors
`incomplete`, `invalid`, `failed`), as specify-core's `serve.py` does; the protocol is in
`src/serve.rs`.

```bash
echo '{"id": 1, "op": "analyse", "step": "part.step"}' | specify-core serve
```

### Library API

- `part::File::{read, of_bytes, load, load_all}` reads a file once and loads its parts;
  `part::load_bytes` loads a single-part STEP held in memory.
- `api::{analyse, preview, review}` analyses a loaded part, turns answers into its bound intent, and composes a front end review.
- `writer::{write, write_parts}` writes verified PMI and stored answers to a destination;
  `writer::{write_bytes, write_parts_bytes}` returns the same verified STEP bytes in memory.
- `writer::{write_bytes_with, write_parts_bytes_with}` adds `extra: &[haecceity::pmi::AttributeSet]`
  after the existing arguments. Extras are written through haecceity and verified in the same
  edit as the PMI and stored answers. `writer::{write_with, write_parts_with}` accepts the same
  extras for atomic file output, including `.gz` destinations. Existing APIs are unchanged and
  delegate with an empty slice, producing identical bytes.

For example, the editor can store its base64 project beside the answers:

```rust
use haecceity::pmi::{AttributeSet, AttributeValue};
use specify_core::writer;

let name = "draftwright editor project";
let extra = AttributeSet {
    name: name.into(),
    on: None, // the part
    items: vec![(name.into(), AttributeValue::Text(base64_project))],
    description: None,
    general_property: None,
};
let (bytes, report) = writer::write_bytes_with(&loaded, &intent, Some(&answers), &[extra])?;
```

Nonempty extras require a file of one part; an assembly is refused even if only one of its
parts is being written. A set or item named `pmi-assist answers` is refused because that name
is reserved for resume. Names, owners and values pass through to haecceity; its read-only
`description` and `general_property` metadata follow its normal writer rules. Extras follow
the usual write mode: replacing a resumed part replaces its previous extras along with its PMI
and answers, so resupply every extra wanted in the new write. Adding to foreign PMI leaves
existing attributes alone. There is no separate merge or update by attribute name.

### Where the Rust engine differs

Each of these is recorded with its reason (docs/known-differences.md, docs/questions.md):

- **Datums are proposed as measurement origins in ± mode too** (maintainer decision,
  draftwright-rust #122; rules version 2). The same A/B/C defaults apply, including a turned
  part's axis A and shoulder B. They are written as AP242 datum features without adding
  geometric tolerances in ± mode. Selected datum planes also set the origins of explicit ±
  hole locations; changing or removing datums changes those origins. Verdict and exact Python
  before/after evidence: [rules differences](tests/fixtures/known_differences/rules.json) and
  `tests/fixtures/rule_defaults_oracle.json.gz`; the captured Python results are retained.
- **Part notes and surface finish are written in AP242's standard forms**, not Python's text
  route: a finish (the default or on faces) as an ISO 10303-1110 surface texture, the coating,
  heat treatment and edge note as the PMI practice's 'semantic text' (Q3). specify-core reads
  the notes back but not the finish.
  Rust also supports ordered free-text manufacturing notes through `part.notes` (default
  `[]`), each a semantic `general note` with an explicit one-based `note index`. See the
  [STEP form and readback contract](docs/part-notes.md). Drawing-only notes stay in the
  drawing Spec; no automatic migration is performed.
- **Knurls are written as AP242's `turned_knurl`**, straight or diamond, which Python writes
  as text: the number of teeth, tooth depth, root fillet and a diamond's helix angle are
  derived from DIN 82 and stated in the intent with their sources, filled in with a warning
  for an intent that lacks them; a pitch or pattern the standard does not give is listed in
  `not_written` (Q4, Q15).
- **An assembly that already has PMI from elsewhere is written** (by haecceity's add mode,
  keeping every original byte), where Python refuses it because of OpenCascade; pending the
  maintainer's confirmation (Q6).
- **Feature ids can differ from Python's** for the same feature (quiddity-rust reports a
  feature by its defining faces, Python's quiddity by its constituent faces, and some families
  differ), so question ids and answer keys do too. An analysis and its answers belong together;
  analyse and write with the same engine (Q1, upstream needs U2, U12).
- **A gzipped STEP file is read** (told by its magic bytes, decompressed in memory, bound by
  the decompressed bytes' hash), where specify-core refuses it; `write -o out.step.gz` writes
  it gzipped (maintainer decision 2026-10-15).
- A file that cannot be read, or an assembly analysed without `--part`, is an `error:` line on
  stderr (exit 1) where Python prints a traceback; `write`'s stdout is only the report, without
  OpenCascade's transfer statistics; meshes are triangulated by a local stand-in (U4) with about
  1.8 times Python's triangles (Q12); a product with surface models beside its solid (NIST
  CTC-02, 04, 05) is one part, not OpenCascade's several (Q7).

## Tests

```bash
cargo test --release
SPECIFY_CORPUS=/path/to/quiddity/tests/corpus cargo test --workspace --release   # with the corpus comparisons
```

CI runs the full workspace with the pinned corpus and `QUIDDITY_CORPUS_REQUIRED=1`, which
fails on a missing corpus or captured corpus input; set it locally to require the same coverage.

Tests compare the port with fixtures captured from specify-core (`tools/capture.py`, run in
specify-core's environment); tests whose input is a quiddity corpus file skip without the
corpus. `docs/known-differences.md` is regenerated after an area file changes with
`UPDATE_KNOWN_DIFFERENCES=1 cargo test --release --test known_differences_doc`.

Optional live Python/OpenCascade tests use only an explicitly configured
`SPECIFY_TEST_PYTHON=/path/to/specify-core/.venv/bin/python`; they never discover a
checkout on the host. `SPECIFY_TEST_NIST_AP242=/path/to/plain/nist` additionally enables
live comparisons on the five `nist_ctc_0N_asme1_ap242.stp` inputs. The committed captures
remain the default regression record; these live checks do not recapture them.

## Licence

AGPL-3.0-or-later; see [LICENSE](LICENSE) and [NOTICE](NOTICE).
