"""What a choice means and what it puts on the drawing, and typed specs read as
choices.

A hole or turned diameter is asked what it is *for* -- a bolt passes, a screw
threads in, a pin fits -- and each purpose holds the codes an engineer would
name (M6, H7). ``describe`` gives every option its purpose, a plain label, its
code and its callout, so a person can choose by either. ``interpret`` reads
what someone typed ("h6", "M2", "clearance M6", "0.15") against the feature's
geometry and returns the answer it means, or refuses with the reason: a typed
spec may be anything, but it is only written if it fits the part.
"""

from __future__ import annotations

import re
from typing import Any

from . import standards

GENERAL = "general"
NONE = "none"

#: Purposes, in the order they are offered, for holes and for turned diameters.
HOLE_PURPOSES = {
    "clearance": "A bolt passes through",
    "tapped": "A screw threads into it",
    "fit": "A pin, shaft or bearing fits in it",
    GENERAL: "Nothing in particular",
}
TURNED_PURPOSES = {
    "fit": "It fits into a bore or bearing",
    "thread": "It is threaded",
    "knurl": "It is knurled for grip",
    GENERAL: "Nothing in particular",
}


class SpecError(ValueError):
    """A typed spec that cannot be read, or that does not fit the feature."""


def describe(qid: str, options, diameter: float | None) -> list[dict[str, Any]]:
    """Each option of a hole or turned-diameter question, for choosing by
    purpose or by code; empty for any other question."""
    if diameter is None or not qid.startswith(("hole.function:", "diameter.fit:")):
        return []
    hole = qid.startswith("hole.function:")
    purposes = HOLE_PURPOSES if hole else TURNED_PURPOSES
    out = []
    for value in options:
        kind = _kind(value, hole)
        out.append(
            {
                "value": value,
                "purpose": kind,
                "label": purposes[kind],
                "code": code(value),
                "callout": callout(value, diameter, hole),
            }
        )
    return out


def code(value: str) -> str:
    """An answer as an engineer would name it: ``H7``, ``M6 clearance``, ``M5 tapped``."""
    kind, _, spec = value.partition(":")
    if value == GENERAL:
        return "general tolerance"
    if kind == "fit":
        return spec
    if kind == "clearance":
        return f"{spec} clearance" if spec else "clearance"
    if kind == "tapped":
        return f"{spec} tapped"
    if kind == "thread":
        return f"{spec} thread"
    if kind == "knurl":
        return f"{spec} knurl"
    return value


def callout(value: str, d: float, hole: bool) -> str:
    """What the answer puts on the drawing for a feature of diameter ``d``."""
    kind, _, spec = value.partition(":")
    if value == GENERAL:
        return f"⌀{_fmt(d)}, general tolerance"
    if kind == "tapped":
        pitch = standards.PITCH[spec]
        return f"{spec}×{pitch:g}-{standards.INTERNAL_THREAD_CLASS}, ⌀{_fmt(d)} tap drill"
    if kind == "thread":
        pitch = standards.PITCH[spec]
        return f"{spec}×{pitch:g}-{standards.EXTERNAL_THREAD_CLASS}"
    if kind == "knurl":
        return f"{spec.capitalize()} knurl, {standards.KNURL_PITCH:g} mm pitch"
    fit = "H11" if kind == "clearance" else spec if kind == "fit" else value
    upper, lower = standards.fit_limits(fit, d)
    limits = f"{d + lower:.3f}–{d + upper:.3f}"
    tail = f" for {spec}" if kind == "clearance" and spec else ""
    return f"⌀{_fmt(d)} {fit} ({limits}){tail}"


def interpret(qid: str, text: str, options, diameter: float | None) -> str:
    """The answer ``text`` means for question ``qid``; ``options`` are those
    offered and ``diameter`` the feature's, if it has one."""
    typed = " ".join(text.strip().split())
    if not typed:
        raise SpecError("Type a spec, e.g. H7 or M6.")
    # On a shaft, case says which is meant: M6 is a thread, m6 a fit.
    fold = (lambda x: x) if qid.startswith("diameter.fit:") else str.lower
    for option in options:
        if fold(typed) in (fold(option), fold(code(option))):
            return option
    if qid.startswith("hole.function:") and diameter is not None:
        return _hole(typed, diameter)
    if qid.startswith("diameter.fit:") and diameter is not None:
        return _turned(typed, diameter)
    if qid.startswith(("hole.position:", "diameter.runout:")) or qid.endswith(
        (".control", ".flatness")
    ):
        return _zone(typed)
    if qid.startswith("hole.location:"):
        zone = _zone(typed.replace("+-", "").replace("±", ""))
        return "general" if zone == NONE else f"±{zone}"
    if qid.startswith("hole.frame:"):
        letters = "|".join(re.findall(r"[A-Za-z]", typed)).upper()
        if letters in options:
            return letters
        raise SpecError(f"Measure from one of: {', '.join(options)}.")
    if qid == "part.general_tolerance":
        grade = re.fullmatch(r"(?:iso\s*)?(?:2768\s*-?\s*)?([fmcv])", typed.lower())
        if grade and f"ISO 2768-{grade.group(1)}" in options:
            return f"ISO 2768-{grade.group(1)}"
    raise SpecError(f"Choose one of: {', '.join(options)}.")


def check(qid: str, value: Any, options, diameter: float | None) -> None:
    """Refuse an answer that is neither offered nor a spec that fits the feature."""
    if not isinstance(value, str):
        # JSON answers can be any type; a choice's are offered or typed text.
        raise SpecError(f"{qid}: an answer is text, not {type(value).__name__} {value!r}.")
    if value in options:
        return
    if interpret(qid, value, options, diameter) != value:
        raise SpecError(f"{value!r} is not an answer; it reads as something else.")


# --------------------------------------------------------------------------


_SIZE = r"M(\d+(?:\.\d+)?)(?:\s*[x×]\s*(\d+(?:\.\d+)?))?"
_NONE_WORDS = {"general", "none", "nothing", "no", "plain", "gen"}


def _hole(typed: str, d: float) -> str:
    low = typed.lower()
    if low in _NONE_WORDS:
        return GENERAL
    fit = re.fullmatch(r"(?:fit\s*:?\s*)?(js|h)(\d{1,2})", low)
    if fit:
        return f"fit:{_fit(fit.group(1).upper(), fit.group(2), d, hole=True)}"
    words = re.sub(_SIZE, " ", typed, count=1, flags=re.I).lower().split()
    size = re.search(_SIZE, typed, flags=re.I)
    if not size:
        if re.fullmatch(r"(?:fit\s*:?\s*)?[a-z]{1,2}\d{1,2}", low):
            raise SpecError("Holes take an H (or JS) fit, e.g. H7.")
        raise SpecError("Type a fit (H7), a thread (M5) or a bolt (clearance M6).")
    bolt = _bolt(size.group(1), size.group(2))
    clearance = any(w.startswith(("clear", "bolt", "pass")) for w in words)
    tapped = any(w.startswith(("tap", "thread", "screw")) for w in words)
    if clearance and tapped:
        raise SpecError("A hole is either tapped or a clearance hole, not both.")
    drill = standards.TAP_DRILL[bolt]
    fits_tap = abs(drill - d) <= 0.1
    fits_bolt = (
        0
        < d - standards.bolt_diameter(bolt)
        <= 2 * (standards.CLEARANCE_COARSE[bolt] - standards.bolt_diameter(bolt))
    )
    if tapped or (not clearance and fits_tap):
        if not fits_tap:
            raise SpecError(f"An {bolt} tap needs a ⌀{_fmt(drill)} drill; this hole is ⌀{_fmt(d)}.")
        return f"tapped:{bolt}"
    if not fits_bolt:
        if d <= standards.bolt_diameter(bolt):
            raise SpecError(f"An {bolt} bolt will not pass a ⌀{_fmt(d)} hole.")
        raise SpecError(
            f"⌀{_fmt(d)} is too loose to be a clearance hole for {bolt} "
            f"(ISO 273: ⌀{_fmt(standards.CLEARANCE_FINE[bolt])}–"
            f"{_fmt(standards.CLEARANCE_COARSE[bolt])})."
        )
    return f"clearance:{bolt}"


def _turned(typed: str, d: float) -> str:
    low = typed.lower()
    if low in _NONE_WORDS:
        return GENERAL
    # Capital M is a thread (M6), lower case m a fit (m6).
    fit = re.fullmatch(r"(?:fit\s*:?\s*)?(js|[fghkmnp])(\d{1,2})", low)
    if fit and not re.match(r"(?:thread\s*:?\s*)?M\d", typed):
        return _fit(fit.group(1), fit.group(2), d, hole=False)
    if "knurl" in low or low in ("diamond", "straight"):
        pattern = next((p for p in standards.KNURL_PATTERNS if p in low), None)
        if pattern is None:
            raise SpecError("Say a straight or a diamond knurl.")
        return f"knurl:{pattern}"
    size = re.search(_SIZE, typed, flags=re.I)
    if not size:
        if re.fullmatch(r"(?:fit\s*:?\s*)?[a-z]{1,2}\d{1,2}", low):
            raise SpecError("Shafts take f, g, h, js, k, m, n or p fits, e.g. g6.")
        raise SpecError("Type a fit (g6), a thread (M6) or a knurl (diamond knurl).")
    bolt = _bolt(size.group(1), size.group(2))
    if abs(standards.bolt_diameter(bolt) - d) > 0.2:
        raise SpecError(
            f"An {bolt} thread is ⌀{_fmt(standards.bolt_diameter(bolt))}; this is ⌀{_fmt(d)}."
        )
    return f"thread:{bolt}"


def _zone(typed: str) -> str:
    low = typed.lower().lstrip("⌀ø").strip()
    if low in _NONE_WORDS:
        return NONE
    try:
        value = float(low)
    except ValueError:
        raise SpecError("Type a tolerance in mm, e.g. 0.1, or none.") from None
    if not 0 < value <= 10:
        raise SpecError("A tolerance zone is more than 0 and at most 10 mm.")
    return _fmt(value)


def _fit(letter: str, grade: str, d: float, *, hole: bool) -> str:
    fit = f"{letter}{int(grade)}"
    try:
        standards.fit_limits(fit, d)
    except ValueError as exc:
        raise SpecError(f"{fit} is not tabulated here ({exc}).") from None
    return fit


def _bolt(nominal: str, pitch: str | None) -> str:
    size = f"M{_fmt(float(nominal))}"
    if size not in standards.PITCH:
        known = ", ".join(standards.PITCH)
        raise SpecError(f"{size} is not a tabulated metric thread ({known}).")
    if pitch is not None and abs(float(pitch) - standards.PITCH[size]) > 1e-9:
        raise SpecError(f"Only coarse pitch is tabulated: {size}×{standards.PITCH[size]:g}.")
    return size


def _kind(value: str, hole: bool) -> str:
    if value == GENERAL:
        return GENERAL
    kind = value.partition(":")[0]
    if hole:
        return kind
    return kind if kind in ("thread", "knurl") else "fit"


def _fmt(value: float) -> str:
    return f"{value:.3f}".rstrip("0").rstrip(".")
