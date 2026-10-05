"""Where a label's leader lands on the part, and which way it leaves.

For each set of faces a label is about: the largest of them, a point proved to
lie on its real trim (Quiddity's ``inspect_face``, as quid2pmi anchors its
labels), and a direction whose leader leaves through free space rather than
through the part (``sightlines``). A flat face or a shaft is read from its
outward normal; a hole from along its axis, out of either end, since a leader
straight off a bore's wall would cross the hole into its far side.
"""

from __future__ import annotations

from typing import Any

from build123d import Face
from quiddity.inspection import inspect_face

from .load import LoadedPart
from .sightlines import SightTester, normalise

#: How far a leader must run clear, as a share of the part's diagonal.
REACH = 0.12


def key(faces) -> str:
    """The key a set of faces' anchor is found by: its ids, sorted."""
    return ",".join(str(int(i)) for i in sorted(faces))


def anchors(loaded: LoadedPart, groups, diagonal: float) -> dict[str, dict[str, Any]]:
    """An anchor for each set of face ids in ``groups`` that has one."""
    tester = SightTester(loaded.shape, diagonal * REACH)
    out: dict[str, dict[str, Any]] = {}
    for faces in groups:
        k = key(faces)
        if not faces or k in out:
            continue
        found = _anchor(loaded, [int(i) for i in faces], tester)
        if found:
            out[k] = found
    return out


def _anchor(loaded: LoadedPart, faces: list[int], tester: SightTester) -> dict[str, Any] | None:
    chosen = [Face(loaded.face(i)) for i in faces if 0 <= i < loaded.binding.face_count]
    if not chosen:
        return None
    face = max(chosen, key=lambda f: f.area)
    try:
        point = inspect_face(face).anchor
        at = (
            (float(point.X), float(point.Y), float(point.Z))
            if hasattr(point, "X")
            else tuple(point)
        )
        n = face.normal_at(face.center()) if at is None else face.normal_at(at)
    except Exception:
        return None
    normal = normalise((float(n.X), float(n.Y), float(n.Z)))
    if normal is None:
        return None
    preferred = [normal]
    hole = False
    if face.geom_type.name == "CYLINDER":
        axis = normalise(_axis(face))
        # A hole's normal points at its own axis: read it from an end instead.
        # (Not tested against the face's centre: a bore's can lie on its wall.)
        if axis and _inward(face, at, normal):
            hole = True
            preferred = [axis, (-axis[0], -axis[1], -axis[2]), normal]
    direction, clear = tester.choose(at, preferred)
    if hole:
        # Landing part way down the bore, a label a short way out would still be
        # in the hole, hidden by its walls: it lands on the rim it leaves by.
        at = _rim(face, at, direction)
    return {
        "at": [round(v, 6) for v in at],
        "out": [round(v, 6) for v in direction],
        "clear": clear,
    }


def _axis(face: Face):
    from OCP.BRepAdaptor import BRepAdaptor_Surface

    d = BRepAdaptor_Surface(face.wrapped).Cylinder().Axis().Direction()
    return (d.X(), d.Y(), d.Z())


def _inward(face: Face, at, normal) -> bool:
    """Whether ``normal`` at ``at`` on a cylinder points toward its axis: a hole."""
    from OCP.BRepAdaptor import BRepAdaptor_Surface

    axis = BRepAdaptor_Surface(face.wrapped).Cylinder().Axis()
    d, o = axis.Direction(), axis.Location()
    rel = (at[0] - o.X(), at[1] - o.Y(), at[2] - o.Z())
    v = rel[0] * d.X() + rel[1] * d.Y() + rel[2] * d.Z()
    radial = (rel[0] - v * d.X(), rel[1] - v * d.Y(), rel[2] - v * d.Z())
    return sum(r * n for r, n in zip(radial, normal, strict=True)) < 0


def _rim(face: Face, at, direction):
    """``at`` slid along a cylinder's axis to the end of the face ``direction`` leaves
    by; ``at`` itself if ``direction`` is not along the axis."""
    from OCP.BRepAdaptor import BRepAdaptor_Surface
    from OCP.BRepTools import BRepTools

    axis = BRepAdaptor_Surface(face.wrapped).Cylinder().Axis()
    d, o = axis.Direction(), axis.Location()
    along = direction[0] * d.X() + direction[1] * d.Y() + direction[2] * d.Z()
    if abs(abs(along) - 1) > 1e-6:
        return at
    _, _, vmin, vmax = BRepTools.UVBounds_s(face.wrapped)
    v = (at[0] - o.X()) * d.X() + (at[1] - o.Y()) * d.Y() + (at[2] - o.Z()) * d.Z()
    run = (vmax - v) if along > 0 else (v - vmin)
    return tuple(a + c * run for a, c in zip(at, direction, strict=True))
