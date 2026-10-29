# Comparing revisions: `specify-core diff`

```bash
specify-core diff old.step new.step [--answers old.answers.json] [--title TITLE] [--part N]
                  [--viewer URL] -o DIR
```

`diff` compares two revisions of a part, or of an assembly part by part (see
[Assemblies](#assemblies)). It reports what changed in words a person can check
against the model, draws the change, and carries the part's answers to the new revision.
The [STEP diff action](../step-diff/README.md) runs it on pull requests.

## What it writes

| File | What it holds |
| --- | --- |
| `report.md` | The review in Markdown, headed `TITLE` (by default the new file's name), written to be posted as a pull request comment. |
| `model.png` | The new part, drawn isometrically. |
| `answers.json` | The answers carried to the new revision, to commit as its answers. |
| `report.json` | All of it as data: the verdict, summary, carried answers, changed features, measures, PMI changes, unresolved answers, answers to check and new questions. |

It also prints `{"verdict", "summary"}` to stdout.

## How the parts are matched

quiddity's correspondence matches the two parts face by face. It finds the motion that takes
the old part onto the new one, so a part moved or rotated as a whole still matches. Each feature of
the old part (a hole, a pattern, a boss, a slot, a fillet) is then:

- **unchanged:** the same feature, perhaps with renumbered faces;
- **adapted:** the same feature, resized or moved;
- **orphaned:** gone;
- **ambiguous:** it matches several features that look alike, such as a symmetric
  repeat, or several features have become one;
- **new:** in the new part only.

## What the report says

- **The verdict:**
  - ✅ `ok`: every answer carried and the new part asks nothing new.
  - ⚠️ `decide`: an answer couldn't be carried, or the new part asks a question the old one
    didn't.
  - ❌ `problems`: Specify's review finds problems in the new part that it didn't find in the
    old one.
- **The summary** counts the changes by family ("2 holes changed, 8 rounds and surfaces with
  them"). Fillets, blends, chamfers and freeform surfaces that change along with another
  feature are counted after it.
- **The measures** are the volume, surface area, bounding box and face count, before and after.
- **What changed** groups the features that changed in the same way. Each one is named as a
  drawing names it, with its size ("2× Ø5 × 30 through holes", "R1 fillet", "10 thick plate").
  Then it's placed so a person can find it:
  - on a side of the part ("on the −Y side"), or through it along an axis;
  - in a host feature ("in the Ø70 boss around the Ø45 hole"), with offsets from that
    feature's axis;
  - or, otherwise, from a nearby larger hole's axis, or from a corner of the part ("from the −X,
    −Y corner").

  Then it says what changed: a size ("depth 11 → 13; diameter 10 → 9"), a move along the
  part's axes ("moved Z +2"), "reshaped" for a freeform surface, or "removed". A move along a
  feature's own axis isn't reported when its depth or height changed, since that move is the
  change in length. A move component of a tenth of the largest one or less is left out.
- **Ambiguous features** say what happened:
  - "are now one": several features merged into one;
  - "is now split in N": one feature split into several;
  - "still there, but alike…": the features remain, but which is which can't be told.
- **Answers not carried** keep their value and give the reason:
  - **orphaned:** the feature was removed;
  - **ambiguous:** it now matches several features;
  - **unmatched:** its faces carried, but no single feature of its family holds them;
  - **stale:** the old part had no such feature either.
- **Answers to check** were carried to a feature that changed. They're applied, but a fit or a
  clearance chosen for the old size may be wrong for the new one.
- **New questions** are the questions the new part asks that the old one didn't.
- **PMI** compares the PMI in the two files:
  - **Specify's own:** the new file's stored answers, set against the answers carried.
  - **The files' own:** dimensions, tolerances, datums and notes from the CAD system, added,
    removed or changed. Each is named by the feature it's on ("flatness 0.2 → 0.1").

## How answers carry

Each answer's faces are mapped to the new part, and the answer moves to the new feature of the
same family that holds them. Its id is renamed when the faces renumbered. An answer is never
guessed onto a feature: when there's no single place for it, it's left unresolved with its
value and reason, for a person to place.

The answers carried are, in order of preference:

1. those given with `--answers`;
2. without `--answers`, or with an empty file, those Specify stored in the old STEP file when it
   wrote its PMI;
3. none, in which case the report still says what changed.

## The picture

`model.png` is the new part, seen isometrically with Z up. Changed faces are orange, new faces
green, faces that can't be told apart purple, and the rest grey. The old part's faces that are
gone or moved are drawn over it in translucent red, placed by the matching motion.

## Assemblies

A file of several parts, or of a part placed more than once, is compared as an assembly:

- **Parts are paired by name**, the product name in the STEP file (its product id when it has
  no name, else `part N`). Parts of the same name pair in file order, the second shown as
  `name (2)`. A renamed part is one removed and another added.
- **Each part in both files is compared on its own**, as a single part is, with the rest of the
  file consulted for its mates. Its status is `changed` when its comparison finds anything,
  else `quantity` when it is placed a different number of times, `moved` when a placement
  differs, else `unchanged`. Parts in one file only are `added` or `removed`.
- **The verdict** is the worst of the changed parts'. The summary counts each status and names
  the parts when there are 5 or fewer: `2 parts changed (GRM-02 shaft, GRM-04 drive plate), 10
  unchanged.`
- **`report.md`** has the summary, the picture, a table of the parts that differ, and a section
  `#### name` for each changed part: that part's own report. With `--viewer URL` (the
  [/diff page](https://draftwright.io/diff)'s link for the two revisions), each section links to
  `URL&part=name`.
- **`model.png`** is the whole assembly, every part where it is placed: changed, re-counted and
  moved parts orange, added parts green, the rest grey, and removed parts in translucent red
  where they were.
- **Answers are by part name**: `--answers` takes `{part name: {question id: value}}`, and
  `answers.json` holds the answers carried, by part. A part with no entry carries the answers
  Specify stored in it.
- **`report.json`** is `{verdict, summary, parts}`, each part `{name, status, old, new}` (which
  part of each file and how many times it is placed) and its `report` when it is in both.

`--part N` compares only part N of each file, as before: a single part's report. Both files must
have that part.
