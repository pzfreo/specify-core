"""How the parts of an assembly fit together, as answers to suggest.

A part on its own says only what its holes could be: a Ø4.2 hole is an M5 tap
drill or just a hole. In an assembly the part next to it says more. Every
cylindrical face of every part is placed where each of its instances is, and
two that share an axis and meet along it, in different parts (or different
instances of one), are a mate:

* a tap drill under a clearance hole for the same screw: tapped, and clearance;
* two clearance holes for the same bolt: clearance, a bolt through both;
* a shaft in a bore of its own size: a fit, H7 in the bore and h6 on the shaft;
* a shaft at a thread's size in its tap drill: a thread, and tapped.

Each is a suggestion with its reason, by face index of the part analysed. A
face whose mates disagree gets none. The rules offer a suggestion as the
default, to be checked; nothing is decided from an assembly alone.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepTools import BRepTools
from OCP.GeomAbs import GeomAbs_Cylinder
from OCP.gp import gp_Pnt, gp_Trsf, gp_Vec
from OCP.TopAbs import TopAbs_REVERSED

from . import standards
from .load import LoadedPart

#: How far apart two axes, or the ends of two faces along them, may be and mate.
REACH = 0.01
#: How far a shaft and a bore may differ in diameter and still be one size.
SAME_SIZE = 0.05


@dataclass(frozen=True)
class _Cylinder:
    face: int
    #: A bore (the material outside it), not a shaft.
    bore: bool
    diameter: float
    origin: tuple[float, float, float]
    direction: tuple[float, float, float]
    #: Where the face starts and ends along ``direction`` from ``origin``.
    span: tuple[float, float]

    def placed(self, trsf: gp_Trsf) -> _Cylinder:
        o = gp_Pnt(*self.origin).Transformed(trsf)
        d = gp_Vec(*self.direction).Transformed(trsf)
        return _Cylinder(
            self.face,
            self.bore,
            self.diameter,
            (o.X(), o.Y(), o.Z()),
            (d.X(), d.Y(), d.Z()),
            self.span,
        )


def mates(parts: list[LoadedPart], part: int) -> dict[str, dict[str, str]]:
    """Suggested answers for the ``part``-th of ``parts`` (all of one assembly),
    from its mates: face index -> {"value": answer, "reason": why}."""
    shapes = [_cylinders(p) for p in parts]
    found: dict[int, set[tuple[str, str]]] = {}
    for i, trsf in enumerate(parts[part].placements):
        mine = [c.placed(trsf) for c in shapes[part]]
        for other, theirs in enumerate(shapes):
            for j, their_trsf in enumerate(parts[other].placements):
                if (other, j) == (part, i):
                    continue
                placed = [c.placed(their_trsf) for c in theirs]
                for a in mine:
                    for b in placed:
                        if _mated(a, b):
                            suggestion = _suggest(a, b, parts[other].name or f"part {other}")
                            if suggestion:
                                found.setdefault(a.face, set()).add(suggestion)
    out = {}
    for face, suggestions in found.items():
        if len({value for value, _ in suggestions}) == 1:
            value, reason = min(suggestions)
            out[str(face)] = {"value": value, "reason": reason}
    return out


def _cylinders(loaded: LoadedPart) -> list[_Cylinder]:
    out = []
    for index in range(loaded.binding.face_count):
        face = loaded.face(index)
        surface = BRepAdaptor_Surface(face)
        if surface.GetType() != GeomAbs_Cylinder:
            continue
        cylinder = surface.Cylinder()
        axis = cylinder.Axis()
        u0, u1, v0, v1 = BRepTools.UVBounds_s(face)
        # The face's normal against the way out from the axis: in, for a bore.
        point, du, dv = gp_Pnt(), gp_Vec(), gp_Vec()
        surface.D1((u0 + u1) / 2, (v0 + v1) / 2, point, du, dv)
        normal = du.Crossed(dv)
        if face.Orientation() == TopAbs_REVERSED:
            normal.Reverse()
        o, d = axis.Location(), axis.Direction()
        out_from_axis = gp_Vec(o, point) - gp_Vec(d) * gp_Vec(o, point).Dot(gp_Vec(d))
        out.append(
            _Cylinder(
                index,
                normal.Dot(out_from_axis) < 0,
                2 * cylinder.Radius(),
                (o.X(), o.Y(), o.Z()),
                (d.X(), d.Y(), d.Z()),
                (v0, v1),
            )
        )
    return out


def _mated(a: _Cylinder, b: _Cylinder) -> bool:
    """Whether ``a`` and ``b`` share an axis and meet along it."""
    along = _dot(a.direction, b.direction)
    if abs(along) < 1 - 1e-6:
        return False
    w = _sub(b.origin, a.origin)
    t = _dot(w, a.direction)
    off = _sub(w, tuple(t * x for x in a.direction))
    if _dot(off, off) ** 0.5 > REACH:
        return False
    lo, hi = sorted((t + along * b.span[0], t + along * b.span[1]))
    return max(lo - a.span[1], a.span[0] - hi) <= REACH


def _suggest(a: _Cylinder, b: _Cylinder, name: str) -> tuple[str, str] | None:
    """What ``a`` is, from ``b`` (of the part ``name``) on its axis."""
    db = _fmt(b.diameter)
    if a.bore and b.bore:
        tapped = standards.match_size(a.diameter, standards.TAP_DRILL)
        if tapped and tapped in standards.clearance_bolts(b.diameter):
            return (
                f"tapped:{tapped}",
                f"it lines up with the ⌀{db} {tapped} clearance hole in {name}: "
                f"a screw through that threads in here",
            )
        screw = standards.match_size(b.diameter, standards.TAP_DRILL)
        if screw and screw in standards.clearance_bolts(a.diameter):
            return (
                f"clearance:{screw}",
                f"it lines up with the {screw} tap drill in {name}: "
                f"a screw passes through it into that",
            )
        common = set(standards.clearance_bolts(a.diameter)) & set(
            standards.clearance_bolts(b.diameter)
        )
        if common:
            bolt = max(common, key=standards.bolt_diameter)
            return (
                f"clearance:{bolt}",
                f"it lines up with the ⌀{db} hole in {name}: a {bolt} bolt passes through both",
            )
        return None
    if a.bore:  # b is a shaft in it
        if abs(a.diameter - b.diameter) <= SAME_SIZE:
            return (
                "fit:H7",
                f"the ⌀{db} shaft of {name} fits in it: H7 assumed, with h6 on the shaft",
            )
        thread = standards.thread_size(b.diameter)
        if thread and standards.match_size(a.diameter, standards.TAP_DRILL) == thread:
            return f"tapped:{thread}", f"the {thread} shaft of {name} screws into it"
        if thread and thread in standards.clearance_bolts(a.diameter):
            return f"clearance:{thread}", f"the ⌀{db} shaft of {name} passes through it"
        return None
    if b.bore:  # a is a shaft in b
        if abs(a.diameter - b.diameter) <= SAME_SIZE:
            return "h6", f"it fits the ⌀{db} bore in {name}: h6 assumed, with H7 in the bore"
        thread = standards.thread_size(a.diameter)
        if thread and standards.match_size(b.diameter, standards.TAP_DRILL) == thread:
            return f"thread:{thread}", f"it screws into the {thread} tap drill in {name}"
    return None


def suggestion(found: dict[str, Any] | None, faces) -> dict[str, str] | None:
    """The suggestion every one of ``faces`` has, if they all have the same."""
    if not found or not faces:
        return None
    each = [found.get(str(i)) for i in faces]
    if any(s is None for s in each) or len({s["value"] for s in each}) != 1:
        return None
    return each[0]


def _dot(a, b) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def _sub(a, b) -> tuple[float, float, float]:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def _fmt(value: float) -> str:
    return f"{value:.3f}".rstrip("0").rstrip(".")
