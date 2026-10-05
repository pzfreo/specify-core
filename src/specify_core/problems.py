"""What is incomplete or does nothing in the intent so far, for a person to
put right: shown as the answers are given, each with the faces and question
it is about.

    unlocated   a hole meant for a fastener or a fit with nothing to locate it
    unused      a datum no tolerance is measured from
    redundant   a datum that fixes nothing an earlier one does not
    orphaned    an added requirement citing a datum that is gone
    circular    an added orientation on the faces of the datum it cites
"""

from __future__ import annotations

import re
from typing import Any

from . import extras, rules
from .choices import GENERAL, code


def problems(analysis: dict[str, Any], answers: dict[str, Any]) -> list[dict[str, Any]]:
    faces = {f["id"]: f for f in analysis["faces"]}
    merged = rules.with_all_defaults(analysis, answers)
    qs = rules.questions(analysis, merged)
    asked = {q.id for q in qs}
    intent = rules.preview(analysis, answers)
    out: list[dict[str, Any]] = []

    by_position = rules._answer(merged, qs, "part.locating") != rules.LOCATING[1]
    positioned = rules._existing_faces(analysis, "geometric_tolerance", "Position")
    datum_faces = {i for d in intent.datums for i in d["faces"]}
    for q in qs:
        if not q.id.startswith("hole.function:") or not by_position:
            continue
        value = merged.get(q.id, q.default)
        feature = q.id.partition(":")[2]
        if value == GENERAL or f"hole.position:{feature}" in asked:
            continue
        if set(q.faces) & (positioned | datum_faces):
            continue  # the file locates it, or it is a datum
        what = _what(q.prompt)
        many = "×" in what
        out.append(
            _problem(
                "warning",
                f"The {what} {'are' if many else 'is'} for {code(value)} but nothing locates "
                f"{'them' if many else 'it'}: there is no datum A to measure a position from.",
                q.faces,
                q.id,
            )
        )

    used = {x for r in intent.requirements for x in r.get("datums", ())}
    # A distance measured from a datum's faces -- one called out, say -- uses it too.
    used |= {
        d["letter"]
        for d in intent.datums
        for r in intent.requirements
        if r["kind"] == "location" and r.get("reference") and set(r["reference"]) <= set(d["faces"])
    }
    for d in intent.datums:
        if not d.get("existing") and d["letter"] not in used:
            out.append(
                _problem(
                    "info",
                    f"Datum {d['letter']} is not used: no tolerance is measured from it.",
                    d["faces"],
                    f"datum.{d['letter']}",
                )
            )

    seen: list[dict[str, Any]] = []
    for d in sorted(intent.datums, key=lambda d: d["letter"]):
        try:
            kind = rules.datum_kind(d["faces"], faces)
        except ValueError:
            continue
        mine = faces[d["faces"][0]]
        for earlier in seen:
            theirs = faces[earlier["faces"][0]]
            same = (
                rules._parallel(mine["direction"], theirs["direction"])
                if kind == "plane" and rules.datum_kind(earlier["faces"], faces) == "plane"
                else kind == "axis"
                and rules.datum_kind(earlier["faces"], faces) == "axis"
                and rules._coaxial(mine, theirs)
            )
            if same:
                relation = "parallel to" if kind == "plane" else "on the same axis as"
                out.append(
                    _problem(
                        "warning",
                        f"Datum {d['letter']} is {relation} {earlier['letter']}, so it fixes "
                        f"nothing {earlier['letter']} does not; choose one square to it.",
                        d["faces"],
                        f"datum.{d['letter']}",
                    )
                )
                break
        seen.append(d)

    # Two sizes on one diameter -- a fit asked for and one called out, say --
    # would both be written, and disagree.
    sizes = [r for r in intent.requirements if r["kind"] == "size"]
    for i, later in enumerate(sizes):
        earlier = next((r for r in sizes[:i] if set(r["faces"]) & set(later["faces"])), None)
        if earlier:
            twice = " and ".join(_size_text(r) for r in (earlier, later))
            out.append(
                _problem(
                    "warning",
                    f"⌀{later['nominal']:g} is sized twice ({twice}); remove one, or both are "
                    "written.",
                    later["faces"],
                    later.get("question") or earlier.get("question") or "",
                )
            )

    letters = {d["letter"] for d in intent.datums}
    by_letter = {d["letter"]: set(d["faces"]) for d in intent.datums}
    for key, extra in extras.given(answers):
        own = [
            x
            for x in extra.get("datums") or ()
            if extra.get("kind") in extras.DATUMED and by_letter.get(x, set()) & set(extra["faces"])
        ]
        if own:
            out.append(
                _problem(
                    "warning",
                    f"The {extra['kind']} you added is on datum {own[0]}'s own faces, so it is "
                    f"measured from itself; choose other faces for {own[0]} or for the "
                    f"{extra['kind']}.",
                    extra["faces"],
                    key,
                )
            )
        missing = [x for x in extra.get("datums") or () if x not in letters]
        if extra.get("kind") in extras.DATUMED and missing:
            out.append(
                _problem(
                    "warning",
                    f"The {extra['kind']} you added is measured from datum {', '.join(missing)}, "
                    "which has no faces yet; pick them under Datums, or it will not be written.",
                    extra.get("faces", []),
                    f"datum.{missing[0]}",
                )
            )
    return out


def _what(prompt: str) -> str:
    match = re.match(r"What (?:are|is) the (.+) for\?$", prompt)
    return match.group(1) if match else "hole"


def _size_text(req: dict[str, Any]) -> str:
    if req.get("fit"):
        return str(req["fit"])
    upper, lower = req["upper"], req["lower"]
    return f"±{upper:g}" if upper == -lower else f"{upper:+g}/{lower:+g}"


def _problem(severity: str, message: str, faces, about: str) -> dict[str, Any]:
    return {"severity": severity, "message": message, "faces": list(faces), "about": about}
