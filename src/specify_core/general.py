"""Dimensions under the general tolerance.

A part's PMI otherwise states only what was specified: fits, threads,
positions. What the general tolerance covers -- every other diameter, length
and hole location -- is left to whoever dimensions the drawing, and an
inspection list built from the PMI misses it. Here those dimensions are
found, toleranced by the general tolerance class (ISO 2768-1 linear), for the
inspection list and the view. They are not written into the STEP: Draftwright
draws an authored dimension beside its own rather than instead of it
(draftwright#2185), and measures one between offset parallel planes between
their centres (draftwright#2184).

The scheme is baseline from the datums, as MBD practice asks: along each
direction the part has parallel planes in, every plane level is a length from
one reference plane -- the datum plane in that direction if there is one, else
the lowest. Holes not located by a position are located from those same
references. Diameters are each hole's and turned step's own, unless a fit,
thread or knurl already sizes them.
"""

from __future__ import annotations

import bisect
from typing import Any

from . import locations

#: ISO 2768-1 permissible deviations (±mm) for linear dimensions, by class,
#: for nominal sizes up to each bound (over 0.5 mm).
_BOUNDS = (3, 6, 30, 120, 400, 1000, 2000, 4000)
ISO_2768 = {
    "f": (0.05, 0.05, 0.1, 0.15, 0.2, 0.3, 0.5, None),
    "m": (0.1, 0.1, 0.2, 0.3, 0.5, 0.8, 1.2, 2.0),
    "c": (0.2, 0.3, 0.5, 0.8, 1.2, 2.0, 3.0, 4.0),
    "v": (None, 0.5, 1.0, 1.5, 2.5, 4.0, 6.0, 8.0),
}

_LEVEL = 1e-4


def deviation(general: str, nominal: float) -> float | None:
    """The ± ``general`` (e.g. ``ISO 2768-m``) allows ``nominal``; None where
    the class sets none."""
    cls = general.strip().rsplit("-", 1)[-1].lower()
    if cls not in ISO_2768 or not 0.5 <= nominal <= _BOUNDS[-1]:
        return None
    return ISO_2768[cls][bisect.bisect_left(_BOUNDS, nominal)]


def dimensions(
    analysis: dict[str, Any],
    datums: dict[str, dict[str, Any]],
    specified: list[dict[str, Any]],
    general: str,
) -> list[dict[str, Any]]:
    """The general dimensions ``analysis`` needs beside the ``specified``
    requirements, measured from ``datums``, each marked ``general``."""
    from .rules import _bores, _bores_of

    faces = {f["id"]: f for f in analysis["faces"]}
    features = analysis["features"]
    by_id = {f["id"]: f for f in features}
    sized = {
        int(i) for r in specified if r["kind"] in ("size", "thread", "knurl") for i in r["faces"]
    }
    located = {r["feature"] for r in specified if r["kind"] in ("location", "position")}
    # A distance called out between the same places -- a plane level, a hole's
    # axis -- says what the general one would, whichever of their faces were picked.
    called = {
        _between(r["faces"], r["reference"], faces) for r in specified if r["kind"] == "location"
    }
    members = {
        m: f["id"] for f in features if f["family"] == "hole_patterns" for m in f.get("members", ())
    }
    out: list[dict[str, Any]] = []

    def toleranced(req: dict[str, Any], value: float) -> None:
        t = deviation(general, value)
        between = (
            _between(req["faces"], req["reference"], faces) if req["kind"] == "location" else None
        )
        if t is not None and between not in called:
            out.append({**req, "upper": t, "lower": -t, "general": True})

    # Diameters: one requirement on a pattern covers each of its holes.
    for feature in features:
        family = feature["family"]
        if family == "holes" and feature["id"] not in members:
            holes = [feature]
        elif family == "hole_patterns" and feature.get("members"):
            holes = [by_id[m] for m in feature["members"]]
        elif family == "turned_steps":
            d = float(feature["record"]["diameter"])
            cylinders = [i for i in feature["faces"] if faces[i]["kind"] == "cylinder"]
            if cylinders and not set(cylinders) & sized:
                toleranced(
                    {"kind": "size", "feature": feature["id"], "faces": cylinders, "nominal": d}, d
                )
            continue
        else:
            continue
        d = float(holes[0]["record"]["diameter"])
        bores = [i for h in holes for i in _bores(h["faces"], d, faces)]
        if bores and not set(bores) & sized:
            toleranced({"kind": "size", "feature": feature["id"], "faces": bores, "nominal": d}, d)

    # Lengths: every plane level from its direction's reference. A hole's own
    # floors and counterbores are the hole's, not the part's.
    in_holes = {i for f in features if f["family"] == "holes" for i in f["faces"]}
    planes = [f for f in faces.values() if f["kind"] == "plane" and f["id"] not in in_holes]
    references: list[tuple[int, ...]] = []
    for line in _lines(planes):
        levels = _levels(line)
        reference = _reference(levels, datums, faces) or levels[0][1]
        references.append(tuple(reference))
        base = _offset(faces[reference[0]], line[0]["direction"])
        for offset, ids in levels:
            if ids == reference or abs(offset - base) < _LEVEL:
                continue
            toleranced(
                {
                    "kind": "location",
                    "feature": "",
                    "faces": list(ids),
                    "reference": list(reference),
                    "distance": round(abs(offset - base), 6),
                },
                abs(offset - base),
            )

    # Hole locations, for holes no position or ± location already places.
    for feature in features:
        if feature["family"] != "holes" or feature["id"] in located:
            continue
        if members.get(feature["id"]) in located:
            continue
        found = locations.dimensions(
            [(feature["id"], feature["record"], _bores_of(feature, faces))],
            references,
            faces,
            tolerance=None,
        )
        for req in found:
            req.pop("basic", None)
            toleranced(req, req["distance"])
    return out


def _between(ids, reference, faces: dict[int, dict]) -> frozenset:
    """Where a distance runs, from and to, as places rather than faces."""
    return frozenset((_place(ids, faces), _place(reference, faces)))


def _place(ids, faces: dict[int, dict]) -> tuple:
    """A plane level (its normal's line and offset along it) or an axis (its
    direction's line and nearest point to the origin), rounded; else the faces."""
    first = faces.get(int(ids[0])) if ids else None
    if first is None:
        return ("faces", tuple(sorted(int(i) for i in ids)))
    n = first["direction"]
    # A line's two directions are one: point it to a positive first non-zero component.
    sign = next((1 if x > 0 else -1 for x in n if abs(x) > 1e-9), 1)
    line = tuple(round(sign * x, 4) + 0.0 for x in n)
    if first["kind"] == "plane":
        return ("plane", line, round(sign * _dot(first["centroid"], n), 3) + 0.0)
    if first["kind"] == "cylinder" and "origin" in first:
        o = first["origin"]
        t = _dot(o, n)
        return ("axis", line, tuple(round(x - t * y, 3) + 0.0 for x, y in zip(o, n, strict=True)))
    return ("faces", tuple(sorted(int(i) for i in ids)))


def _lines(planes: list[dict]) -> list[list[dict]]:
    """``planes`` grouped by the line their normals lie on."""
    out: list[list[dict]] = []
    for plane in sorted(planes, key=lambda p: p["id"]):
        for line in out:
            if abs(abs(_dot(line[0]["direction"], plane["direction"])) - 1.0) < 1e-6:
                line.append(plane)
                break
        else:
            out.append([plane])
    return out


def _levels(line: list[dict]) -> list[tuple[float, tuple[int, ...]]]:
    """The planes on one line, as (offset, faces) per level, lowest first."""
    n = line[0]["direction"]
    out: list[tuple[float, list[int]]] = []
    for plane in sorted(line, key=lambda p: _offset(p, n)):
        offset = _offset(plane, n)
        if out and abs(out[-1][0] - offset) < _LEVEL:
            out[-1][1].append(plane["id"])
        else:
            out.append((offset, [plane["id"]]))
    return [(offset, tuple(sorted(ids))) for offset, ids in out]


def _reference(levels, datums, faces) -> tuple[int, ...] | None:
    """The faces of the first datum plane among ``levels``, by letter."""
    ids = {i for _, level in levels for i in level}
    for letter in sorted(datums):
        datum = datums[letter]
        if datum["kind"] == "plane" and set(datum["faces"]) <= ids:
            return tuple(sorted(datum["faces"]))
    return None


def _offset(plane: dict, n) -> float:
    return _dot(plane["centroid"], n)


def _dot(a, b) -> float:
    return sum(x * y for x, y in zip(a, b, strict=True))
