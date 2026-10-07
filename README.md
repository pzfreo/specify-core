# specify-core

Interview-driven PMI authoring for STEP parts. specify-core recognises a part's
features, works out what manufacturing intent is missing (datums, fits, threads,
positions, finish, the general tolerance and the dimensions that need their own),
proposes a default for each with the reason it was chosen, and writes the answers
back into the STEP as AP242 semantic PMI, verified by reading the file back.

It is deterministic code: rules and standards tables, no AI. It is the engine
behind [Draftwright Specify](https://draftwright.io/specify), which adds the 3-D
review on top; this package is usable on its own from the command line or Python.

Status: early (0.x). The command line and the JSON formats may change.

## Install

```bash
uv sync                       # or: pip install .
```

## Use

```bash
uv sync
specify-core analyse part.step -o analysis.json            # faces, features, existing PMI
specify-core mesh part.step -o part.mesh.json               # triangles by face index, for a picker
specify-core questions analysis.json --answers a.json --open  # what is still unanswered
specify-core write part.step analysis.json a.json -o out.step --accept-defaults --intent intent.json
```

`answers.json` maps question id to value, e.g. `{"part.material": "6082-T6",
"hole.function:hole_patterns.grid:…": "fit:H7"}`. `write` refuses until every
question has an answer (or a default, with `--accept-defaults`), then writes AP242
into the original document and verifies it by reading it back.

An assembly is specified a part at a time. `specify-core parts asm.step` lists its
distinct parts (a part placed twice is one part, with two placements); `analyse`
and `mesh` take `--part N`; and `write` takes an analysis and answers for each
part, writing them all into the assembly at once, each part's PMI on its own
product definition:

```bash
specify-core write asm.step plate.json plate-a.json pin.json pin-a.json -o out.step
```

Writing an assembly specify-core wrote replaces its PMI, so every part with
answers stored in it must be written again together. An assembly that already
has PMI from elsewhere is refused for now.

v1 covers: material, general tolerance (ISO 2768), datums A/B/C (default: three
mutually square planes, A taking every coplanar face, or plane + main cylinder on
turned parts), hole function (clearance / tapped / ISO 286 fit, defaults from ISO
273 and tap-drill sizes), position tolerance (defaults from the fastener formula),
turned diameters (fit, external thread at a metric size, or straight / diamond
knurl), flatness on datum A. Tapped holes carry the full thread: ISO 965 class,
hand, drill and minimum full thread. Metric only: inch parts get no size-based
defaults.

A datum is one face, several coplanar faces, or a hole or boss (its axis). B and
C get a control where v1 can write one: a tertiary hole is positioned to A|B;
otherwise the feature is held perpendicular to the earlier datums it is square
to (a plane C to A|B). A feature parallel to A gets none, as it would need
parallelism. A hole chosen as a datum keeps its size question and leaves its
set; its control replaces its position, and without one it keeps a position
from the frames with its own letter removed. Positions need datum A. Extra
datums (D–H) are asked only when the answers add them (`"datum.D": [faces]`);
positions then offer a frame per feature (`hole.frame:…`, e.g. `D|B|C`), and a
frame answer that is no longer offered is refused rather than guessed.

Written to the file: datums, size limits, position, flatness, perpendicularity
(by OCCT's XCAF); material (OCCT, as the CAx-IF material practice's 'material
name'); and, appended to the Part 21 text because OCCT has no entity for them,
threads, knurls and the general tolerance. Each tolerance gets its own datum
entries, because OCCT keeps a datum's precedence on the datum rather than the
tolerance.

Appended requirements are written twice (`requirements.py`): as Draftwright's
text route (a 'manufacturing requirement' sentence, linked to its faces through
a named draughting callout), and as standard constructs (a CAx-IF user defined
attribute with the values as named items, and the PMI practice's 'default
tolerances' 'tolerance class'). Sentences state only what is known -- no drill
point on a flat-bottomed hole, no chamfers that are not there -- so Draftwright,
whose parser expects those clauses, does not read every one yet.

Tests read Quiddity's STEP corpus (`QUIDDITY_CORPUS`, default the sibling
`quiddity/tests/corpus`) and skip without it. `tools/draftwright_roundtrip.py`
shows what Draftwright reads from an output file; run it in Draftwright's own
environment.

## Pipeline

1. **Load** the STEP once through OCCT XCAF (keeps names, colours, existing PMI).
2. **Recognise** features with Quiddity's evidence view on that same shape.
3. **Read existing PMI** and attach it to features — don't ask for what exists.
4. **Gap rules** (deterministic, versioned) decide what "manufacturable" needs.
5. **Questions** with defaults; the user answers in a form, with 3D picking.
6. **Intent** — the answers — is the source of truth, stored as JSON and
   anchored on stable face indices.
7. **Write** an enriched AP242 STEP generated from intent, verified by read-back.
   A part that already has PMI keeps it byte for byte: OCCT cannot write back
   PMI it has read, so the new PMI is transplanted into the original file
   (`merge.py`).
8. **Draw**: Draftwright reads the enriched STEP (`pmi='annotate'`).

```
analyse(step) -> Analysis                # features, existing PMI, gaps
questions(analysis, intent) -> [Question]
apply(intent, answers) -> Intent
write(step, intent, out) -> WriteReport  # verified by read-back
```

## Principles

- **Quiddity says what the geometry is; specify-core says what the engineer means.**
  Standards knowledge (fits, clearance/tap sizes, ISO 2768), datum choice and gap
  rules live here; geometric facts live in Quiddity.
- **Rules decide completeness.** Every default is chosen by a rule from the part
  and the standards, and says why, so a person can keep it or change it.
- **A GUI for pointing, picking and review**, the command line for everything else.
- **Intent is the source of truth.** The STEP is a regenerable artifact.
- **Face indices, not Quiddity refs, anchor intent.** `FaceRef`/`FeatureRef` are
  run-local by design.

## Dependencies

[Quiddity](https://github.com/pzfreo/quiddity) (feature recognition) and
build123d/OCP (OpenCascade). Draftwright, if installed, draws the result.

## Tests

```bash
uv run pytest
```

Some tests read Quiddity's STEP corpus (`QUIDDITY_CORPUS`, default the sibling
`quiddity/tests/corpus`) or NIST's PMI test files (`NIST_PMI`, the unzipped
[NIST-PMI-STEP-Files.zip](https://github.com/usnistgov/SFA/tree/master/Release)),
and skip without them.

## Licence

Copyright (C) 2026 Betwixt Limited (trading as Draftwright). Licensed under the
GNU Affero General Public License v3.0 or later: see [LICENSE](LICENSE) and
[NOTICE](NOTICE). For other licensing terms, get in touch.

Contributions are welcome. So that the project can continue to be offered
under other terms as well, contributors will be asked to agree to a contributor
licence agreement.
