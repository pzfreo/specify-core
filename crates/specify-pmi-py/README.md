# specify-pmi

**Add manufacturing intent to a STEP part, and write it as AP242 semantic PMI.**

specify-pmi reads a STEP part, recognises its features (holes with their counterbores and
threads, hole patterns, slots, bosses, fillets and turned bodies) and works out what a
manufacturer still needs to know: the material, datums, fits, thread classes, hole positions, surface
finish and the general tolerance. It asks a question for each gap, with a suggested default
and the reason for it. Your answers are written back into the STEP as AP242 semantic PMI
(GD&T that CAD and inspection tools can read), and the file is read back to verify it.

It is deterministic: rules and standards tables, no AI. The answers are stored in the file
too, so a later revision of the part can be opened and carried on with.

- No dependencies: no OpenCascade, build123d or quiddity. The engine is Rust, in one wheel.
- Wheels for Linux (x86_64, aarch64), macOS (Apple silicon, Intel) and Windows, for
  CPython 3.11 and later.
- Apache-2.0.

It is the engine behind [Draftwright Specify](https://draftwright.io/specify), which adds
a 3D review of the same questions in the browser.

## Install

```bash
pip install specify-pmi
```

## Python

```python
from specify_pmi import api

analysis = api.analyse("plate.step")         # features, faces and any existing PMI

for q in api.questions(analysis, {}):         # what is still unknown
    print(q.id, "|", q.prompt, "| default:", q.default)

# An edit list: fill gaps, change what the file states, remove by number, decide none,
# accept suggestions. Ids are the feature table's (analysis["table"]["rows"]).
edits = {"fill": {"part.material": "6082-T6", "hole.function:holes:9": "fit:H7"},
         "accept_suggestions": ["part.general_tolerance"]}
report = api.write("plate.step", edits, "plate-pmi.step")   # all or nothing

print(report.written)       # ['material 6082-T6', 'Ø8 H7 on holes:9', ...]
print(report.retired)       # characteristic numbers a change or a remove retired
```

A list is applied all or nothing: if any entry cannot be applied, nothing is written and
`EditsRefused` (a `ValueError`, from `specify_pmi.writer`) names every failing entry in its
`failures`. `api.check_edits(step, edits)` checks a list without writing.

Write reports optionally include `geometry_carrier_exemptions` (`{part, condition,
aspect, feature}`), `preserved_normalizations` (`{id, normalization}`),
`presentation_warnings` (strings), `retained_shared` (ids of support Replace kept
for PMI it cannot read) and `saved_view_findings` (`{kind, part, ids, entity, detail}`
findings about the default Notes saved view). `to_dict()` omits empty lists. The v0.4.8 kernel
preserves original entity text during Add, including when adding a separate datum
feature on a kept carrier. Add upgrades requiring source repairs raise a clear
`RuntimeError` naming the entities and repair; Specify never retries with Replace.

Opening the written file again picks up where you left off: the file is the only copy (its PMI,
read into `api.analyse(...)["table"]`, and the decisions that left no PMI, in
`["resume"]["answers"]`).

The main functions, all in `specify_pmi.api`:

| Function | What it does |
|---|---|
| `parts(step)` | Lists an assembly's parts. |
| `load(path, part=None)` | Reads a file once; pass the returned `LoadedPart` to the others instead of a path. |
| `analyse(step, part=None)` | Recognises features and reads existing PMI. Returns the analysis (a dict). |
| `questions(analysis, answers)` | The questions, given the answers so far (`Question` objects). |
| `apply(analysis, answers, accept_defaults=False)` | Turns answers into the intent to write. Raises `IncompleteError` naming what is still missing, unless defaults are accepted. |
| `preview(analysis, answers)` | The intent the answers so far would give, without requiring every answer. |
| `write(step, edits, output)` | Applies an edit list to the file, all or nothing, and verifies it. Returns a `WriteReport`. |
| `check_edits(step, edits)` | Checks an edit list as `write` would apply it; writes nothing. |
| `write_parts(step, intents, output)` | The same for several parts of an assembly, in one file. |
| `mesh(step, part=None, *, max_deviation_mm=None)` | Triangles grouped by face index, for showing the part and picking faces. |

Answers are a dict of question id to value, e.g. `{"part.material": "6082-T6"}`. Requirements
no question asked for (a flatness, a fit, a finish on faces you choose) are added as `extra.*`
answers.

## Command line

```bash
specify-pmi analyse plate.step -o plate.analysis.json
specify-pmi questions plate.analysis.json --open                # unanswered questions, as JSON
echo '{"fill":{"part.material":"6082-T6"},"accept_suggestions":["part.general_tolerance"]}' > edits.json
specify-pmi write plate.step edits.json -o plate-pmi.step
```

`specify-pmi parts` lists an assembly's parts. Pass `--part INDEX` to write the edit
list to one of them. `specify-pmi mesh` writes the face-indexed triangles.

## For a host program

`specify-pmi serve` answers JSON requests, one per line, on stdin and stdout. Each request
carries everything it needs, so a restarted process loses nothing. This is how an application
drives it as a child process:

```json
{"id": 1, "op": "analyse", "step": "/path/part.step"}
{"id": 2, "op": "questions", "analysis": {...}, "answers": {...}}
```

Each reply is `{"id", "ok": true, "result"}` or `{"id", "ok": false, "error": {"code", "message"}}`,
where the code is `incomplete` (with what is `missing`), `invalid` or `failed`. `analyse` replies
with the analysis and the mesh. For edit-list writes use the CLI command above or the
Python `write` function. The serve protocol retains its intent-based `write` request
for existing clients.
`analyse` accepts optional `"max_deviation_mm": 0.05` for its mesh. It must be a
finite number of at least 0.001 mm; invalid or unmet bounds return `invalid`. Omitting
it (or `null`) preserves the default mesh. Requested meshes add `max_deviation_mm`
and `achieved_deviation_mm`; see [mesh tolerance](../../docs/mesh-tolerance.md).
`specify_pmi.serve.reply(request)` answers the same requests in-process.

## What it writes

AP242 semantic PMI: datums and datum systems, size dimensions with ± and fit tolerances,
location dimensions, position, flatness, perpendicularity, parallelism, profile and runout
tolerances, threads, knurls, surface texture, material, notes and the general tolerance. Every
file is read back and checked before it is kept.

## Links

- Source and issues: [github.com/pzfreo/specify-pmi](https://github.com/pzfreo/specify-pmi)
- Changelog: [CHANGELOG.md](https://github.com/pzfreo/specify-pmi/blob/main/CHANGELOG.md)
- Building and testing this package: [docs/python.md](https://github.com/pzfreo/specify-pmi/blob/main/docs/python.md#the-python-package)
- Try it in the browser: [draftwright.io/specify](https://draftwright.io/specify)

Status: early (0.x). The JSON formats may change between minor versions; changes are listed in
the changelog. Draftwright® is a registered trade mark of Betwixt Ltd.

There is no export of answers as a record (ADR P11, 0.3.0: `api.export_answers` is removed);
the written STEP holds them. See [answer files](../../docs/answers.md#no-export-of-answers).
