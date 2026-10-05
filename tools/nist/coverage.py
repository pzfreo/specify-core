"""How much of NIST's PMI a person can give a part through specify-core.

NIST's MBE PMI test cases come as a geometry-only STEP and an AP242 with
semantic PMI. The AP242's PMI is the truth; the geometry-only part is what a
user starts from. A case file gives the answers a person would give for it in
Specify, and notes on what cannot be given. Each item of NIST's PMI is then:

    default   -- what specify-core proposes unasked
    answered  -- given by answering, or by an added requirement
    partial   -- given, but not as NIST states it (the difference is shown)
    missing   -- cannot be given
    unread    -- the truth itself is not known: its value or tolerance is not read
    unmatched -- its faces are not found in the geometry-only part

and what specify-core states that NIST does not is listed after: NIST's PMI is
curated, so stating more is a finding too.

    python tools/nist/coverage.py NIST_DIR tools/nist/ctc01.json

NIST_DIR holds NIST's PMI test files, at any depth (Release/NIST-PMI-STEP-Files.zip
in usnistgov/SFA), which are not redistributed here. The report is a measure,
not a test: it is read, and what it finds is filed as issues.

The truth is what specify-core's reader makes of the AP242, with its limits: a
tolerance given as limits is not read, and a lower deviation's sign is
not to be relied on. Modifiers (Ⓜ, ⌀ zones, on datum references) are not read
from the AP242, so are not compared; where specify-core states one it is shown.
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

from specify_core import api

INCH = 25.4
#: NIST's geometric tolerance types that specify-core has, as its kinds.
KINDS = {
    "Position": "position",
    "ProfileOfSurface": "profile",
    "Perpendicularity": "perpendicularity",
    "Parallelism": "parallelism",
    "Flatness": "flatness",
    "CircularRunout": "runout",
    "TotalRunout": "total_runout",
}
_TOL = 1e-3
_ON = 0.01  # mm: a point on a surface, in two files' coordinates


def _dot(a, b) -> float:
    return sum(x * y for x, y in zip(a, b, strict=True))


def _off_axis(p, q, n) -> float:
    """How far ``p`` is from the line through ``q`` along ``n``."""
    d = [x - y for x, y in zip(p, q, strict=True)]
    c = (d[1] * n[2] - d[2] * n[1], d[2] * n[0] - d[0] * n[2], d[0] * n[1] - d[1] * n[0])
    return math.sqrt(_dot(c, c))


def _same_surface(f: dict, g: dict) -> bool:
    """Whether ``f`` and ``g`` lie on one surface: the files split faces
    differently (a hole is two half-cylinders in one, one face in the other)."""
    if f["kind"] != g["kind"] or "direction" not in f or "direction" not in g:
        return False
    n = f["direction"]
    if abs(abs(_dot(n, g["direction"])) - 1.0) > 1e-6:
        return False
    if f["kind"] == "plane":
        return (
            abs(_dot([x - y for x, y in zip(f["centroid"], g["centroid"], strict=True)], n)) < _ON
        )
    if "origin" not in f or "origin" not in g or _off_axis(f["origin"], g["origin"], n) > _ON:
        return False
    if f["kind"] == "cylinder":
        return abs(f.get("radius", 0.0) - g.get("radius", 0.0)) < _TOL
    if f["kind"] == "cone":
        # No half-angle is recorded: the same axis, at the same place along it.
        along = _dot([x - y for x, y in zip(f["centroid"], g["centroid"], strict=True)], n)
        return abs(along) < 0.5
    return False


def mapping(truth: dict, start: dict) -> dict[int, list[int]]:
    """Each face of ``truth`` as the faces of ``start`` on its surface.

    A hole or cone is all of its surface. Planes may be disjoint at one level,
    so a plane is the nearest pieces on it until their area is its own.
    """
    out: dict[int, list[int]] = {}
    for f in truth["faces"]:
        near = [g for g in start["faces"] if _same_surface(f, g)]
        near.sort(key=lambda g: math.dist(f["centroid"], g["centroid"]))
        if f["kind"] != "plane" or not near:
            out[f["id"]] = [g["id"] for g in near]
            continue
        chosen, area = [], 0.0
        for g in near:
            if area >= f.get("area", 0.0) * 0.99:
                break
            chosen.append(g["id"])
            area += g.get("area", 0.0)
        out[f["id"]] = chosen
    return out


def truth_items(analysis: dict, nominal_unit: str = "mm") -> list[dict]:
    """NIST's PMI, each with a key notes are given by. An inch file's
    dimension nominals are read in inches, their tolerances in mm."""
    scale = INCH if nominal_unit == "inch" else 1.0
    out = []
    for item in analysis["existing"]:
        if item["type"] == "DimensionPresentation" or item["kind"] == "note":
            continue  # graphical only; notes are the part's, asked about as such
        item = dict(item)
        if item["kind"] == "dimension" and item.get("value") is not None:
            item["value"] = item["value"] * scale
        faces = sorted({*(item.get("faces") or ()), *(item.get("faces2") or ())})
        item["key"] = f"{item['type']}@{','.join(map(str, faces))}"
        out.append(item)
    return out


def _faces(item: dict, to: dict[int, list[int]]) -> set[int] | None:
    ids = [*(item.get("faces") or ()), *(item.get("faces2") or ())]
    mapped = [to.get(i) for i in ids]
    if not mapped or not all(mapped):
        return None
    return {g for m in mapped for g in m}


def _num(v) -> float | None:
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def _close(a, b) -> bool:
    a, b = _num(a), _num(b)
    return a is not None and b is not None and abs(a - b) < _TOL


def _modifiers(r: dict) -> str:
    said = [x for x, on in (("Ⓜ", r.get("mmc")), ("⌀ zone", r.get("diametral"))) if on]
    return f"ours has {', '.join(said)}, NIST's not read" if said else ""


def _ours(item: dict) -> str | None:
    """The requirement kind of specify-core's that would state ``item``."""
    if item["kind"] == "geometric_tolerance":
        return KINDS.get(item["type"])
    if item["kind"] == "dimension" and item["type"].startswith("Size_"):
        return "size"
    if item["kind"] == "dimension" and item["type"] in ("Location_None", "Location_LinearDistance"):
        return "location"
    return None


def _gdt(item: dict, faces: set[int], r: dict) -> str:
    """How ``r`` differs from NIST's ``item``; empty when it does not."""
    diffs = []
    if not _close(r.get("tolerance"), item.get("value")):
        diffs.append(f"{r.get('tolerance')} not {item.get('value'):.4g}")
    if list(r.get("datums") or []) != list(item.get("datums") or []):
        diffs.append(f"datums {'|'.join(r.get('datums') or []) or 'none'}")
    if not faces <= set(r["faces"]):
        diffs.append("not on all its faces")
    return ", ".join(diffs)


def judge(item: dict, faces: set[int], intent) -> tuple[bool, str]:
    """Whether ``intent`` states ``item`` on ``faces``: (exactly, how not, or
    how it is said, or nothing where it is not stated at all)."""
    if item["kind"] == "datum":
        for d in intent.datums:
            if d["letter"] == item["type"]:
                if set(d["faces"]) == faces:
                    return True, ""
                return False, f"datum {item['type']} on faces {sorted(d['faces'])}"
        return False, ""
    kind = _ours(item)
    if kind is None:
        return False, f"specify-core has no {item['type']}"
    on = [r for r in intent.requirements if r["kind"] == kind and faces & set(r.get("faces") or ())]
    if item["kind"] == "geometric_tolerance":
        if not on:
            return False, ""
        for r in on:
            if not _gdt(item, faces, r):
                return True, _modifiers(r)
        return False, "; ".join(x for x in (_gdt(item, faces, on[0]), _modifiers(on[0])) if x)
    if item["type"].startswith("Size_"):
        for r in on:
            if _close(r.get("upper"), item["upper"]) and _close(r.get("lower"), item["lower"]):
                return True, ""
        if on:
            r = on[0]
            return False, f"{r.get('fit') or ''} {r.get('upper'):+g}/{r.get('lower'):+g}".strip()
        general = [g for g in intent.general if g["kind"] == "size" and faces & set(g["faces"])]
        if general:
            return False, f"under the general tolerance, ±{general[0]['upper']:g}"
        return False, ""
    return False, ""


def _label(item: dict) -> str:
    if item["kind"] == "datum":
        return f"datum {item['type']}"
    value = item.get("value")
    if item["type"].startswith("Location") and not value:
        return f"{item['type']} (no value)"
    tol = ""
    if item.get("upper") is not None:
        u, lo = item["upper"], item["lower"]
        tol = f" ±{u:.4g}" if abs(u + lo) < _TOL else f" {u:+.4g}/{lo:+.4g}"
    bar = "\\|"  # a table cell's
    datums = f" {bar}{bar.join(item['datums'])}" if item.get("datums") else ""
    shown = "" if value is None else f" {value:.4g}"
    return f"{item['type']}{shown}{tol}{datums}"


def status(item: dict, faces: set[int] | None, defaults, answered) -> tuple[str, str]:
    """``item``'s status, and what is said of it."""
    if (
        item["kind"] == "dimension"
        and item["type"].startswith("Location")
        and not item.get("value")
    ):
        return "unread", "no value read from the AP242"
    if (
        item["kind"] == "dimension"
        and item["type"].startswith("Size_")
        and item.get("upper") is None
    ):
        return "unread", "no tolerance read: it may be given as limits"
    if faces is None:
        return "unmatched", "its faces are not found in the geometry-only part"
    ok, how = judge(item, faces, defaults)
    if ok:
        return "default", how
    ok, how = judge(item, faces, answered)
    if ok:
        return "answered", how
    return ("partial" if how and not how.startswith("specify-core has no") else "missing"), how


def beyond(truth: list[dict], to: dict[int, list[int]], answered) -> list[str]:
    """What ``answered`` states that NIST does not, as lines: requirements on
    faces NIST gives nothing of their kind, and the general dimensions beside
    NIST's dimensions."""
    claimed = {(kind, f) for item in truth if (kind := _ours(item)) for f in _faces(item, to) or ()}
    lines = []
    for r in answered.requirements:
        if not any((r["kind"], f) in claimed for f in r.get("faces") or ()):
            faces = ",".join(map(str, sorted(r.get("faces") or ())))
            lines.append(f"- {r['kind']} on faces {faces} ({r.get('question')})")
    stated = sum(1 for item in truth if item["kind"] == "dimension")
    ours = sum(1 for r in answered.requirements if r["kind"] in ("size", "location"))
    lines.append(
        f"- Dimensions: NIST states {stated} (sizes and locations, toleranced or not). "
        f"specify-core states {ours} and lists {len(answered.general)} more under the general "
        "tolerance; NIST leaves those to the geometry."
    )
    return lines


def _find(nist: Path, name: str) -> Path:
    found = next(nist.rglob(name), None)
    if found is None:
        raise SystemExit(f"{name} is not in {nist}")
    return found


def report(nist: Path, case: Path) -> str:
    spec = json.loads(case.read_text())
    truth_analysis = api.analyse(_find(nist, spec["truth"]))
    start = api.analyse(_find(nist, spec["start"]))
    truth = truth_items(truth_analysis, spec.get("nominal_unit", "mm"))
    to = mapping(truth_analysis, start)
    answers = spec["answers"]
    material = {k: v for k, v in answers.items() if k == "part.material"}
    defaults = api.apply(start, material, accept_defaults=True)
    answered = api.apply(start, answers, accept_defaults=True)
    notes: dict[str, str] = spec.get("notes", {})
    orphans = set(notes) - {item["key"] for item in truth}
    if orphans:
        raise SystemExit(f"notes for no item: {', '.join(sorted(orphans))}")
    rows, tally = [], {}
    for item in truth:
        faces = _faces(item, to)
        state, how = status(item, faces, defaults, answered)
        tally[state] = tally.get(state, 0) + 1
        said = "; ".join(x for x in (how, notes.get(item["key"], "")) if x)
        where = ",".join(map(str, sorted(faces))) if faces else "?"
        rows.append(f"| {item['key']} | {_label(item)} | {where} | {state} | {said} |")
    head = [
        f"# {case.stem}: {spec['truth']} from {spec['start']}",
        "",
        ", ".join(f"{n} {s}" for s, n in sorted(tally.items())),
        "",
        "| NIST item | as read | our faces | status | notes |",
        "|---|---|---|---|---|",
    ]
    more = ["", "## Stated here, not by NIST", "", *beyond(truth, to, answered)]
    return "\n".join(head + rows + more) + "\n"


if __name__ == "__main__":
    print(report(Path(sys.argv[1]), Path(sys.argv[2])), end="")
