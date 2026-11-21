# Carrying requirements to a new revision

```bash
specify-core carry old.step new.step -o new.specified.step
```

`carry` copies an annotated revision's requirements onto a new revision of the geometry, file
to file ([ADR P11](https://github.com/pzfreo/draftwright-project/blob/main/adr/P11-the-file-is-the-only-copy.md),
"Carry forward"). NEW may be bare, or carry some PMI of its own (from upstream, or another tool).
No answers file takes part: OLD's PMI and its decided-none answers are what is carried.

## What carries

OLD's features are matched to NEW's with the revision matcher `diff` uses (its face
correspondence, and its feature changes for what moved or changed size). Then, for each
feature-and-kind OLD's feature table states:

| In NEW | What carry does | Listed in |
|---|---|---|
| a gap | fills it | `carried` |
| a gap, on a feature that moved or changed size | fills it, to be checked | `to_check` |
| stated, or decided none | NEW's stands; reported only when the two differ | `kept` |
| the feature is not found, or its match is ambiguous | nothing | `not_carried` |

OLD's decided-none answers carry the same way, onto NEW's gaps (`decided_none`).

**Datums** are matched by their faces, not their letters. NEW's datum A on other faces than
OLD's A is a difference (`kept`, both face sets shown: `datum A on faces [27]` and `datum A on
faces [2]`), and OLD's requirements on its datum A (its flatness, a perpendicularity) are not
written onto NEW's A: they are not carried. A requirement citing a datum that differs is carried
and listed `to_check`, saying which datum; one citing a datum that is not carried (not found in
NEW, or decided none there) is not carried.

**What the new revision cannot take** is reported and left out, not refused for: a fill that
writes nothing on the new part as it is (its feature changed so the answer has nothing to apply
to, a counterbored hole left to the general tolerance, say), or any fill on a feature that moved
or changed size that the writer refuses. The rest is applied as one list, all or nothing.

A fill is an answer, written by the in-place writer (`write STEP EDITS.json`): carry fills a
feature-and-kind only with an answer that restates OLD's requirement exactly. It tries the
row's value (a tolerance's magnitude, a datum's faces, a part setting's text), the answer OLD
stores for it (a file written by Specify 0.2), and the question's default and options, and
takes the first whose planned requirements on OLD, values included, are the ones OLD states
(`fit:H7` for `⌀5 H7`; `general` for a hole size under the general tolerance). For a kind
answered by a tolerance's magnitude, the frame is compared; its basic dimensions are the
planner's. A row whose requirements a carried row states too (a hole's thread depth, stated by
the thread callout its function writes) is listed as carried `with` that row.

**Not carried, and reported** (with its kind, type, faces and number, so nothing is dropped
silently): PMI on no feature-and-kind (the dimension planner's own dimensions, another tool's
dimensions between faces), and any requirement no answer restates exactly. Choices that are not
requirements (`part.locating`) are not carried.

New features in NEW that NEW states nothing on are listed as information (`new_unstated`,
"New in this revision with nothing stated: ...", as `diff` lists them).

## Numbers

A requirement carried from OLD keeps OLD's characteristic number, and OLD's number history
carries with it: its high-water mark, so numbers OLD retired (and numbers of requirements not
carried) are never reused. Where NEW has a numbering of its own, NEW's numbers stand: a carried
requirement keeps OLD's number only when NEW neither uses nor retired it (above NEW's high-water
mark); otherwise it takes a new number above both files' high-water marks, and is listed in
`renumbered` (`{old, new, kind, type, faces}`). A number either file retired is never reused.
`numbers_carried` is true when at least one carried requirement kept OLD's number. See
[characteristic-numbers.md](characteristic-numbers.md).

In an assembly, each new part matched to an old one by its geometry (renamed parts included)
takes the old part's BOM answers and material where NEW states none; where NEW states one,
NEW's stands and a difference is listed in `kept` (`plate: bom.description`). OLD's BOM history,
with its item numbers and retired numbers, carries unless NEW numbers its own items (a BOM
history of its own, or native find numbers), when NEW's numbering stands ([bom.md](bom.md)). The
BOM is appended and the materials added so that every entity of NEW keeps its bytes. The
in-place writer edits a file of one part, so in this release a part's own requirements in an
assembly, and its PMI on no feature-and-kind, are not carried: each is reported `not_carried`
with its part.

## All or nothing

The carry is one edit list applied all or nothing. If any of it cannot be applied (OLD's datum B
landing on the faces NEW's own datum A is on, say), nothing is written: `written` is false,
`refused` names each entry and why, and the command exits 2. Every entity of NEW the carry does
not address keeps its bytes.

## The report

The command prints the report as JSON, with its Markdown in `markdown`. It starts with the
counts, plainly (`summary`: "Carried 12, not carried 3, to check 1."), and a `verdict` as
`diff`'s: `ok` when everything OLD states was carried or NEW states it alike, `decide` when
anything is not carried or is to check, never a clean 0 that hides them. The command also
prints the verdict and counts on stderr; it exits 0 when it wrote OUT, 2 when the carry was
refused.

```json
{
  "verdict": "decide",
  "summary": "Carried 12, not carried 3, to check 1.",
  "written": true,
  "carried": [{"old": "hole.function:holes:6", "new": "hole.function:holes:6", "value": "fit:H7", "stated": "Size_Diameter 5 H7"}],
  "to_check": [{"old": "...", "new": "...", "value": "general", "stated": "Size_Diameter 6", "check": "Ø6 × 10 through hole changed (position)"}],
  "kept": [{"old": "...", "new": "...", "old_value": "Position 0.3 | A", "new_value": "Position 0.2 | A"}],
  "not_carried": [{"number": "#10", "kind": "dimension", "type": "Location_LinearDistance", "faces": [27, 2], "text": "...", "reason": "..."}],
  "decided_none": [{"old": "part.coating", "new": "part.coating", "value": "None (as machined)"}],
  "new_unstated": [{"feature": "holes:31", "what": "Ø8 × 3 through hole"}],
  "numbers_carried": true,
  "renumbered": [{"old": 1, "new": 24, "kind": "datum", "type": "A", "faces": [27]}],
  "bom_carried": false,
  "markdown": "# Carry\n..."
}
```

MCP has the same as the `carry` tool (`old`, `new`, `output`, `overwrite?`); Rust has
`specify_core::carry::carry` and `carry_bytes`.

Not in this release: the three-way carry (`--base`, the old file as received, so a requirement
the upstream designer removed is not brought back and is reported "removed upstream"), and carry
in the WASM session and Python (piece 6).
