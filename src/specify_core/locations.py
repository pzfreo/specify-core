"""Where a hole is: its distance from reference planes, as AP242 location
dimensions.

A hole positioned by a tolerance has **basic** dimensions -- exact, boxed on a
drawing -- from the planes of its datum frame; a hole located by ± dimensions
has toleranced ones from the part's edge faces. Either way a dimension runs
from a plane along the hole's axis (whose normal is square to it) to the axis,
for each hole of a pattern. A frame of axes (a bolt circle round a bore) has
no plane to measure from: its holes get none here.

AP242 marks a basic dimension, as NIST's test parts do, with a descriptive item
``'dimensional note','theoretical'`` in its shape dimension representation.
OCCT cannot write that, so ``mark_basic`` adds it to the written file.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

#: The ± a hole group may be located to; general leaves it to the general tolerance.
TOLERANCES = ("general", "±0.2", "±0.1", "±0.05", "±0.02")
_THEORETICAL = "DESCRIPTIVE_REPRESENTATION_ITEM('dimensional note','theoretical')"


def along(axis, plane: dict[str, Any]) -> bool:
    """Whether ``plane`` runs along ``axis``: its normal square to it."""
    return abs(sum(a * n for a, n in zip(axis, plane["direction"], strict=True))) < 1e-6


def references(
    planes: list[tuple[int, ...]], axis, faces: dict[int, dict]
) -> list[tuple[int, ...]]:
    """Of ``planes`` (each one or more coplanar faces), those a hole on ``axis``
    can be measured from."""
    return [
        p
        for p in planes
        if p and all(faces[i]["kind"] == "plane" for i in p) and along(axis, faces[p[0]])
    ]


def distance(hole: dict[str, Any], plane: dict[str, Any]) -> float:
    """From ``plane`` to the axis of ``hole`` (a hole feature's record)."""
    offset = [h - c for h, c in zip(hole["location"], plane["centroid"], strict=True)]
    return abs(sum(o * n for o, n in zip(offset, plane["direction"], strict=True)))


def dimensions(
    members: list[tuple[str, dict, tuple[int, ...]]],
    planes: list[tuple[int, ...]],
    faces: dict[int, dict],
    *,
    tolerance: float | None,
) -> list[dict[str, Any]]:
    """A location from each of ``planes`` to each hole in ``members`` -- (id,
    record, bore faces) -- toleranced ±``tolerance``, or basic when None."""
    out = []
    for fid, record, bores in members:
        for plane in references(planes, record["axis"], faces):
            d = round(distance(record, faces[plane[0]]), 6)
            req = {
                "kind": "location",
                "feature": fid,
                "faces": list(bores),
                "reference": list(plane),
                "distance": d,
            }
            if tolerance is None:
                req["basic"] = True
            else:
                req |= {"upper": tolerance, "lower": -tolerance}
            out.append(req)
    return out


def parse(value: str) -> float | None:
    """``±0.1`` (or ``0.1``, ``+-0.1``) as 0.1; general as None."""
    text = str(value).strip().lower().replace("+-", "±").lstrip("±").strip()
    if text in ("", "general"):
        return None
    return float(text)


def mark_basic(path: Path) -> int:
    """Mark every location dimension in ``path`` with no ± as basic; return how many."""
    text = path.read_text()
    bodies = dict(re.findall(r"#(\d+)\s*=\s*(.*?);\s*(?=#\d+\s*=|ENDSEC)", text, re.S))
    toleranced = {
        r
        for body in bodies.values()
        if body.startswith("PLUS_MINUS_TOLERANCE(")
        for r in re.findall(r"#(\d+)", body)
    }
    locations = {
        i
        for i, body in bodies.items()
        if body.startswith("DIMENSIONAL_LOCATION(") and i not in toleranced
    }
    representations = [
        re.findall(r"#(\d+)", body)[1]
        for body in bodies.values()
        if body.startswith("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(")
        and re.findall(r"#(\d+)", body)[0] in locations
    ]
    if not representations:
        return 0
    note = max(int(i) for i in bodies) + 1
    for rep in representations:
        body = bodies[rep]
        marked = re.sub(r"\(\s*((?:#\d+\s*,\s*)*#\d+)\s*\)", rf"(\1,#{note})", body, count=1)
        text = re.sub(rf"#{rep}\s*=\s*{re.escape(body)};", f"#{rep}={marked};", text, count=1)
    end = text.rfind("ENDSEC;")
    text = text[:end] + f"#{note}={_THEORETICAL};\n" + text[end:]
    path.write_text(text)
    return len(representations)
