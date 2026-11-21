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
carried) are never reused. A file with a number history of its own keeps that instead. See
[characteristic-numbers.md](characteristic-numbers.md).

In an assembly, each new part matched to an old one by its geometry (renamed parts included)
keeps its BOM item number and BOM answers, and its material where NEW states none; OLD's BOM
history, with its retired numbers, carries ([bom.md](bom.md)). The in-place writer edits a file
of one part, so in this release the parts' own requirements are not carried in an assembly:
each is reported `not_carried` with its part.

## All or nothing

The carry is one edit list applied all or nothing. If any of it cannot be applied (a hole
position citing a datum NEW decided none, say), nothing is written: `written` is false,
`refused` names each entry and why, and the command exits 2. Every entity of NEW the carry does
not address keeps its bytes.

## The report

The command prints the report as JSON, with its Markdown in `markdown`:

```json
{
  "written": true,
  "carried": [{"old": "hole.function:holes:6", "new": "hole.function:holes:6", "value": "fit:H7", "stated": "Size_Diameter 5 H7"}],
  "to_check": [{"old": "...", "new": "...", "value": "general", "stated": "Size_Diameter 6", "check": "Ø6 × 10 through hole changed (position)"}],
  "kept": [{"old": "...", "new": "...", "old_value": "Position 0.3 | A", "new_value": "Position 0.2 | A"}],
  "not_carried": [{"number": "#10", "kind": "dimension", "type": "Location_LinearDistance", "faces": [27, 2], "text": "...", "reason": "..."}],
  "decided_none": [{"old": "part.coating", "new": "part.coating", "value": "None (as machined)"}],
  "new_unstated": [{"feature": "holes:31", "what": "Ø8 × 3 through hole"}],
  "numbers_carried": true,
  "bom_carried": false,
  "markdown": "# Carry\n..."
}
```

MCP has the same as the `carry` tool (`old`, `new`, `output`, `overwrite?`); Rust has
`specify_core::carry::carry` and `carry_bytes`.

Not in this release: the three-way carry (`--base`, the old file as received, so a requirement
the upstream designer removed is not brought back and is reported "removed upstream"), and carry
in the WASM session and Python (piece 6).
