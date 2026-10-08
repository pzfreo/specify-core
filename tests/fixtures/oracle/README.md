# The Python oracle

specify-core's own outputs (Python, `/Users/paul/repos/specify-core` at the revision each
`meta.json` names), captured by `tools/capture.py` and read by the Rust harness in
`tests/common`. Python is the oracle, not the specification: a Rust result that differs is
recorded in `tests/fixtures/known_differences/` with a verdict and its evidence.

Recapture (specify-core's venv; a few minutes for everything, four inputs at a time):

```bash
PY=/Users/paul/repos/specify-core/.venv/bin/python
$PY tools/capture.py all tests/fixtures/oracle          # every case, the scenarios, interpret
$PY tools/capture.py case tests/fixtures/parts/pin.step tests/fixtures/oracle   # one input
$PY tools/capture.py readback written.step OUT_DIR     # a Rust-written file, read as below
$PY tools/capture.py make-scenarios tests/fixtures/oracle   # rewrite scenarios/*.json
```

A capture is reproducible: run again, only each `meta.json`'s `captured` date changes.

## Cases

One directory per case, every artifact gzipped JSON (`<name>.json.gz`, keys in Python's order):

| Case | Input |
|---|---|
| `parts/<stem>` | `tests/fixtures/parts/<stem>.step` (built by `tools/make_parts.py`) |
| `nist/<stem>` | `tests/fixtures/nist/<stem>.stp.gz` (see its README) |
| `corpus/<subdir>/<stem>` | `$QUIDDITY_CORPUS/<subdir>/<file>`, not committed; `.gz` dropped from the name |
| `<file case>#<k>` | part `k` of a file of several parts |

A `.gz` input is decompressed before either engine reads it; `meta.json` has the sha256 of
both, and the analysis binding's `source_sha256` is the decompressed file's.

Artifacts of an input file (the file's own directory):

- `meta`: `case`, `kind` (`parts`, `nist`, `corpus`), `input` (relative to the kind's root),
  `input_sha256`, `decompressed_sha256` (a `.gz`), `part` (null), `parts` (how many), the
  specify-core and quiddity versions and revisions, `python`, `command`, `captured`, `answers`
  (the base answers below), `normalisations` (what was changed in Python's output), `artifacts`.
- `parts`: `api.parts`.
- `write`: every part applied with the base answers `{"part.material": "steel"}` and defaults
  (`api.apply(accept_defaults=True)`) and written with those answers stored (`api.write`, or
  `api.write_parts` for several parts, as serve's `write` does): the write report, or the
  refusal (an `IncompleteError` for several parts names the part).
- `readback`: the written file read back directly, a list with one entry per part:
  `{existing: [read_existing(load(out, part=k))], part: part_settings(...), resume:
  resume.answers_for(...)}`. Not `api.analyse`, which for a file with answers it can resume
  returns `existing: []` and `part: {}`.
- `written.step.gz` (synthetic and NIST only): the file Python wrote, its FILE_NAME time stamp
  fixed. Its bytes are not a parity target.
- `serve` (synthetic only): one session through `serve.reply`, a list of `{request, reply}`:
  hello, parts, and per part analyse (mesh as its summary), questions, interpret (the first hole
  or diameter whose default is a spec, typed as that default's code), preview with the step;
  then write (with `parts` for several).
- For a file of several parts, `analysis` is Python's refusal; the parts' artifacts are in
  `<case>#<k>`.

Artifacts of a part (the file's directory for a file of one part, else `<case>#<k>`):

- `meta` (`part` = k), `analysis` (`api.analyse`).
- `questions`: `{none: {answers: {}, questions, open}, defaults: {answers:
  with_all_defaults(analysis, {}), questions, open}}`, each question `Question.to_dict()`.
- `problems`: `problems(analysis, {})`.
- `preview`: `api.preview(analysis, {})`.
- `apply`: `api.apply(analysis, {"part.material": "steel"}, accept_defaults=True)`, including
  `general`.
  Material has no default, so a bare `apply(analysis, {}, accept_defaults=True)` is always an
  `IncompleteError` naming `part.material`; that empty-answers call is not captured and has no
  oracle value of its own.
- `mesh`: a summary of `api.mesh`: `format`, `version`, `binding`, `bbox`, `face_count`,
  `triangles` (per face), `missing`, `areas` (per face, of the triangles), `edges` (`count`,
  total `length`). Not the triangles.
- `anchors`: `{diagonal: serve._diagonal (OCCT's loose box, which scales the reach), anchors:
  serve._label_anchors for the preview's groups}`.

The analysis, questions, problems, preview and apply are computed on the analysis after a JSON
round trip, as any client passes it back.

## Failures

A Python refusal or crash is a result: the artifact is `{error: class, message}` (plus
`missing` and `part` for an `IncompleteError`), and capture carries on. A stage that needs a
failed one is left out (`write` is `{"skipped": ...}` when an analysis failed).

## Normalisations

- The input's path is written `{input}`, any output path `{output}`.
- Non-finite floats are `{"$float": "NaN" | "Infinity" | "-Infinity"}` (Python's bare tokens are
  not JSON).
- In serve transcripts a failure's `trace` is `(omitted)`.

## Scenarios and typed specs

`scenarios/<name>.json`: `{case, answers, why, from}` (and `write`: `"with answers"` or
`"without answers"` when the test writes the file), answer sets transcribed from specify-core's
tests by `tools/capture/scenarios.py`. `scenarios/<name>.result.json.gz`: `questions`, `open`,
`problems`, `preview` and `apply` (with defaults), each a value or a failure; with `write`, also
the write report and `readback` (as above).

Corpus-backed scenarios: 15 scenarios answer a corpus case whose input is not committed (11 on
`corpus/nist/nist_ctc_03_asme1_rc`, from specify-core's CTC-03 tests, and 4 on
`corpus/cadgenbench/flanged_spool_132`). Without `QUIDDITY_CORPUS` (or `SPECIFY_CORPUS`) the Rust
side cannot run them and must skip them, printing why; a skipped scenario is not coverage. Find
them with `grep -l '"case": "corpus' scenarios/*.json`.

Not captured (the scenario format answers one captured input with one answer set):

- scenarios whose input is a file Python wrote: `test_merge.py`'s merged and simple chains, and
  `test_assembly.py`'s rewrite, foreign-PMI and moved-answers cases. The writer/resume stream
  builds these from the committed `written.step.gz` files;
- an assembly write with different answers per part (`test_assembly.py`'s steel/brass write):
  every case-level `write` uses `{"part.material": "steel"}` for each part;
- tests of internals on hand-made face dicts (`test_datums.py`'s `_datum_control` and
  `_default_datums`, `test_call_out.py`'s `general._between`): not the analysis-to-answers path.

`interpret.json.gz`: a list of `{from, fn, ..., result}`: every spec specify-core's tests type at
`choices.interpret` (and `check`, `code`, `callout`, `describe`), with Python's value or its
`SpecError`.
