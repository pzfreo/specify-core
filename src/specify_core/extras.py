"""Requirements a person adds where no question asked: a flatness, a
perpendicularity or parallelism to a datum, a fit, or a dimension they call
out, on faces they pick.

Each is an answer keyed ``extra.<id>``::

    {"faces": [12], "kind": "flatness", "value": "0.05"}
    {"faces": [3, 4], "kind": "perpendicularity", "value": "0.1", "datums": ["A"]}
    {"faces": [7], "kind": "fit", "value": "H7"}
    {"faces": [7], "kind": "finish", "value": "Ra 0.8"}
    {"faces": [7], "kind": "size", "value": "±0.05"}
    {"faces": [9], "kind": "distance", "reference": [2], "value": "+0.1/-0"}

A called-out ``size`` is a diameter, toleranced ±, by limits or by a fit; a
``distance`` runs from ``reference`` (flat, coplanar faces) to ``faces``: a
parallel flat level, or a hole's axis. Either takes the place of what the
general tolerance would have said of the same faces.

A malformed one is refused when given. One citing a datum that has since been
removed is not refused -- the datum may come back -- but is not written, and
is reported as a problem.
"""

from __future__ import annotations

import math
import re
from typing import Any

from . import choices, standards

PREFIX = "extra."
KINDS = (
    "flatness",
    "perpendicularity",
    "parallelism",
    "fit",
    "finish",
    "profile",
    "runout",
    "total_runout",
    "size",
    "distance",
)
#: Measured from a datum, necessarily; and those that may be (a profile with
#: none controls form alone, with datums its place too).
ORIENTED = ("perpendicularity", "parallelism", "runout", "total_runout")
DATUMED = (*ORIENTED, "profile")
_FIT = re.compile(r"(JS|js|H|[fghkmnp])(\d{1,2})")
_NUMBER = r"\d*\.?\d+"
_PLUS_MINUS = re.compile(rf"(?:±|\+-|\+/-)?\s*({_NUMBER})")
_LIMITS = re.compile(rf"([+-]?{_NUMBER})(?:/|(?=[+-]))([+-]?{_NUMBER})")


def given(answers: dict[str, Any]) -> list[tuple[str, dict[str, Any]]]:
    """The added requirements among ``answers``, in the order given."""
    return [(k, v) for k, v in answers.items() if k.startswith(PREFIX) and isinstance(v, dict)]


def cited(answers: dict[str, Any]) -> set[str]:
    """The datum letters added requirements are measured from."""
    return {
        str(x)
        for _, extra in given(answers)
        if extra.get("kind") in DATUMED
        for x in extra.get("datums") or ()
    }


def check(key: str, extra: dict[str, Any], faces: dict[int, dict]) -> None:
    """Refuse an added requirement that cannot be read or does not suit its faces."""
    try:
        chosen = [faces[int(i)] for i in extra.get("faces", ())]
    except (KeyError, TypeError, ValueError):
        raise ValueError(f"{key}: a face picked does not exist") from None
    if not chosen:
        raise ValueError(f"{key}: pick the faces it applies to")
    kind = extra.get("kind")
    if kind not in KINDS:
        raise ValueError(f"{key}: {kind!r} is not one of {', '.join(KINDS)}")
    planes = all(f["kind"] == "plane" for f in chosen)
    cylinders = all(f["kind"] == "cylinder" for f in chosen)
    value = str(extra.get("value", ""))
    if kind == "flatness" and not planes:
        raise ValueError(f"{key}: flatness applies to flat faces")
    if kind in ORIENTED and not (planes or cylinders):
        raise ValueError(f"{key}: {kind} applies to flat faces, or to one hole or boss")
    if kind in DATUMED:
        letters = extra.get("datums") or []
        if kind in ORIENTED and not letters:
            raise ValueError(f"{key}: say which datum it is measured from")
        if not all(isinstance(x, str) and re.fullmatch(r"[A-H]", x) for x in letters):
            raise ValueError(f"{key}: datums are letters A to H")
    if kind == "finish":
        _finish(key, value)
        return
    if kind in ("fit", "size"):
        if not cylinders or len({round(f["radius"], 6) for f in chosen}) != 1:
            raise ValueError(f"{key}: a {kind} applies to the cylinder(s) of one diameter")
        d = 2 * chosen[0]["radius"]
        _fit(key, value, d) if kind == "fit" else _limits(key, value, d, fits=True)
    elif kind == "distance":
        _limits(key, value, _distance(key, extra, chosen, faces), fits=False)
    else:
        _zone(key, value)


def requirement(
    key: str, extra: dict[str, Any], faces: dict[int, dict], datums: dict[str, Any]
) -> dict[str, Any] | None:
    """The requirement an added one makes, or None while a datum it cites is missing."""
    chosen = [faces[int(i)] for i in extra["faces"]]
    base = {"feature": "added", "faces": [int(i) for i in extra["faces"]], "question": key}
    kind = extra["kind"]
    if kind == "finish":
        return {"kind": "finish", **base, "value": _finish(key, str(extra["value"]))}
    if kind == "fit":
        d = 2 * chosen[0]["radius"]
        fit = _fit(key, str(extra["value"]), d)
        upper, lower = standards.fit_limits(fit, d)
        return {"kind": "size", **base, "nominal": d, "fit": fit, "upper": upper, "lower": lower}
    if kind == "size":
        d = 2 * chosen[0]["radius"]
        fit, upper, lower = _limits(key, str(extra["value"]), d, fits=True)
        return {"kind": "size", **base, "nominal": d, "fit": fit, "upper": upper, "lower": lower}
    if kind == "distance":
        distance = _distance(key, extra, chosen, faces)
        _, upper, lower = _limits(key, str(extra["value"]), distance, fits=False)
        reference = [int(i) for i in extra["reference"]]
        return {
            "kind": "location",
            **base,
            "reference": reference,
            "distance": round(distance, 6),
            "upper": upper,
            "lower": lower,
        }
    out = {"kind": kind, **base, "tolerance": float(_zone(key, str(extra["value"])))}
    if kind in DATUMED:
        letters = list(extra.get("datums") or ())
        if any(x not in datums for x in letters):
            return None
        out["datums"] = letters
        # A perpendicular or parallel axis has a cylindrical zone; runout and
        # profile zones are not diametral.
        out["diametral"] = (
            kind in ("perpendicularity", "parallelism") and chosen[0]["kind"] == "cylinder"
        )
    return out


def _fit(key: str, value: str, d: float) -> str:
    match = _FIT.fullmatch(value.strip())
    if not match:
        raise ValueError(f"{key}: {value!r} is not a fit, e.g. H7 for a hole or g6 for a shaft")
    # Capitals are hole fits (H7, JS7), lower case shaft fits (g6, js6).
    letter = match.group(1)
    try:
        return choices._fit(letter, match.group(2), d, hole=letter[0].isupper())
    except choices.SpecError as exc:
        raise ValueError(f"{key}: {exc}") from None


def _limits(key: str, value: str, nominal: float, *, fits: bool) -> tuple[str, float, float]:
    """(fit, upper, lower) from ``±0.05`` (or ``0.05``), deviations ``+0.1/-0``,
    or, where ``fits``, a fit such as ``H7`` on a diameter of ``nominal``; the
    fit is '' unless one is given. Limits are deviations from ``nominal``,
    each signed (or 0): ``5.98/6.02`` is refused, not read as +6.02/+5.98."""
    text = value.strip().replace(" ", "").replace("−", "-")
    if fits and _FIT.fullmatch(text):
        fit = _fit(key, text, nominal)
        return (fit, *standards.fit_limits(fit, nominal))
    if match := _PLUS_MINUS.fullmatch(text):
        upper = float(match.group(1))
        lower = -upper
    elif (match := _LIMITS.fullmatch(text)) and all(
        part[0] in "+-" or float(part) == 0 for part in match.groups()
    ):
        upper, lower = sorted((float(match.group(1)), float(match.group(2))), reverse=True)
    else:
        fit = " or a fit, e.g. H7" if fits else ""
        raise ValueError(f"{key}: give ±0.05, or deviations from {nominal:g} such as +0.1/-0{fit}")
    if upper == lower:
        raise ValueError(f"{key}: the limits must differ")
    # A deviation as big as a quarter of the dimension is a limit typed as one.
    if max(abs(upper), abs(lower)) > min(10.0, nominal / 4):
        raise ValueError(
            f"{key}: {value.strip()} is too big a deviation for {nominal:g}; give deviations "
            "from it, e.g. +0.1/-0, not the limits themselves"
        )
    return "", upper + 0.0, lower + 0.0  # no -0.0


def _distance(key: str, extra: dict[str, Any], chosen: list[dict], faces: dict[int, dict]) -> float:
    """From ``extra``'s reference -- coplanar flat faces -- to its faces: a
    parallel level, or one hole's axis."""
    given = extra.get("reference")
    if not isinstance(given, list):
        raise ValueError(f"{key}: measure a distance from flat faces")
    try:
        reference = [faces[int(i)] for i in given]
    except (KeyError, TypeError, ValueError):
        raise ValueError(f"{key}: a reference face does not exist") from None
    if not reference or not all(f["kind"] == "plane" for f in reference):
        raise ValueError(f"{key}: measure a distance from flat faces")
    if {f["id"] for f in reference} & {f["id"] for f in chosen}:
        raise ValueError(f"{key}: a face cannot be measured from itself")
    n = reference[0]["direction"]
    if not all(_parallel(f["direction"], n) for f in reference):
        raise ValueError(f"{key}: the faces measured from must be parallel")
    base = _dot(reference[0]["centroid"], n)
    if any(abs(_dot(f["centroid"], n) - base) > 1e-4 for f in reference):
        raise ValueError(f"{key}: the faces measured from must be at one level")
    if all(f["kind"] == "plane" for f in chosen):
        if not all(_parallel(f["direction"], n) for f in chosen):
            raise ValueError(f"{key}: a distance runs to faces parallel to its reference")
        level = _dot(chosen[0]["centroid"], n)
        if any(abs(_dot(f["centroid"], n) - level) > 1e-4 for f in chosen):
            raise ValueError(f"{key}: the faces measured to must be at one level")
        distance = abs(level - base)
    elif all(f["kind"] == "cylinder" for f in chosen):
        if len({tuple(round(x, 3) for x in _foot(f)) for f in chosen}) != 1:
            raise ValueError(f"{key}: a distance runs to one hole's axis")
        if abs(_dot(chosen[0]["direction"], n)) > 1e-6:
            raise ValueError(f"{key}: a hole's axis must run along the faces measured from")
        distance = abs(_dot(chosen[0]["origin"], n) - base)
    else:
        raise ValueError(f"{key}: a distance runs to flat faces or to one hole")
    if distance < 1e-4:
        raise ValueError(f"{key}: those faces are at the same level")
    return distance


def _foot(cylinder: dict) -> list[float]:
    """The point of ``cylinder``'s axis nearest the origin: faces of one hole share it."""
    o, a = cylinder["origin"], cylinder["direction"]
    t = _dot(o, a)
    return [x - t * y for x, y in zip(o, a, strict=True)]


def _parallel(a, b) -> bool:
    return abs(abs(_dot(a, b)) - 1.0) < 1e-6


def _dot(a, b) -> float:
    return sum(x * y for x, y in zip(a, b, strict=True))


def _finish(key: str, value: str) -> str:
    """``Ra 0.8``, from ``Ra 0.8``, ``ra0.8`` or ``0.8``: roughness in micrometres."""
    match = re.fullmatch(r"(?:ra\s*)?(\d*\.?\d+)\s*(?:µm|um)?", value.strip().lower())
    if not match or not 0 < float(match.group(1)) <= 50:
        raise ValueError(f"{key}: give a roughness as Ra in micrometres, e.g. Ra 0.8")
    return f"Ra {float(match.group(1)):g}"


def _zone(key: str, value: str) -> str:
    try:
        zone = choices._zone(value)
    except choices.SpecError as exc:
        raise ValueError(f"{key}: {exc}") from None
    if zone == choices.NONE or not math.isfinite(float(zone)):
        raise ValueError(f"{key}: give a tolerance in mm, e.g. 0.05")
    return zone
