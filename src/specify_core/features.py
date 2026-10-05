"""Recognised features and plain faces, as serialisable records keyed by face index.

Quiddity's ``FeatureRef`` and ``FaceRef`` are run-local by design. A feature is
identified here by its family and the sorted indices of its faces, which are
stable for the same file (see :mod:`specify_core.load`). The analysis is stored,
not re-derived, so these ids never need to survive a Quiddity upgrade.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Any

from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepGProp import BRepGProp
from OCP.GeomAbs import GeomAbs_Cone, GeomAbs_Cylinder, GeomAbs_Plane, GeomAbs_Torus
from OCP.GProp import GProp_GProps
from OCP.TopAbs import TopAbs_REVERSED
from quiddity.evidence import build_recognition_evidence

from .load import LoadedPart

#: Quiddity's hole-pattern record types, by the short kind used in feature ids.
HOLE_PATTERN_KINDS = {
    "BoltCircle": "bolt_circle",
    "LinearArray": "linear",
    "RectGrid": "grid",
    "RectangularHoleSet": "rectangle",
}


@dataclass(frozen=True)
class Face:
    id: int
    kind: str  # plane | cylinder | cone | torus | other
    area: float
    centroid: tuple[float, float, float]
    #: Outward normal for a plane, axis direction for a surface of revolution.
    direction: tuple[float, float, float] | None = None
    #: A point on the axis, for a surface of revolution.
    origin: tuple[float, float, float] | None = None
    radius: float | None = None

    def to_dict(self) -> dict[str, Any]:
        return {k: v for k, v in self.__dict__.items() if v is not None}


@dataclass(frozen=True)
class Feature:
    id: str
    family: str
    record: dict[str, Any]
    faces: tuple[int, ...]
    #: For patterns: ids of the member features.
    members: tuple[str, ...] = ()
    #: Why a pattern is incomplete, when members could not all be linked.
    notes: tuple[str, ...] = field(default_factory=tuple)

    def to_dict(self) -> dict[str, Any]:
        out: dict[str, Any] = {
            "id": self.id,
            "family": self.family,
            "record": self.record,
            "faces": list(self.faces),
        }
        if self.members:
            out["members"] = list(self.members)
        if self.notes:
            out["notes"] = list(self.notes)
        return out


def feature_id(family: str, faces: tuple[int, ...]) -> str:
    return f"{family}:{'.'.join(str(i) for i in faces)}"


def describe_faces(loaded: LoadedPart) -> list[Face]:
    """Every face of the part, with what a datum or picking question needs."""
    out = []
    for index in range(loaded.binding.face_count):
        face = loaded.face(index)
        props = GProp_GProps()
        BRepGProp.SurfaceProperties_s(face, props)
        c = props.CentreOfMass()
        centroid = (round(c.X(), 6), round(c.Y(), 6), round(c.Z(), 6))
        surface = BRepAdaptor_Surface(face)
        kind = surface.GetType()
        if kind == GeomAbs_Plane:
            n = surface.Plane().Axis().Direction()
            sign = -1.0 if face.Orientation() == TopAbs_REVERSED else 1.0
            out.append(Face(index, "plane", props.Mass(), centroid, _vec(n, sign)))
        elif kind in (GeomAbs_Cylinder, GeomAbs_Cone, GeomAbs_Torus):
            name = {GeomAbs_Cylinder: "cylinder", GeomAbs_Cone: "cone", GeomAbs_Torus: "torus"}
            geom = {
                GeomAbs_Cylinder: surface.Cylinder,
                GeomAbs_Cone: surface.Cone,
                GeomAbs_Torus: surface.Torus,
            }[kind]()
            axis = geom.Axis()
            radius = geom.Radius() if kind == GeomAbs_Cylinder else None
            if kind == GeomAbs_Torus:
                radius = geom.MinorRadius()
            out.append(
                Face(
                    index,
                    name[kind],
                    props.Mass(),
                    centroid,
                    _vec(axis.Direction()),
                    _pnt(axis.Location()),
                    radius,
                )
            )
        else:
            out.append(Face(index, "other", props.Mass(), centroid))
    return out


def recognise(loaded: LoadedPart) -> list[Feature]:
    """Accepted features from Quiddity's evidence view, hole patterns with their
    member holes, and sets of identical holes no pattern claims."""
    view = build_recognition_evidence(loaded.part)
    features: list[Feature] = []
    ids = {}
    # Patterns last, so their member holes have ids to refer to.
    for ref in sorted(view.features, key=lambda r: bool(view.members(r))):
        family = view.family(ref)
        refs = view.constituent_faces(ref) or view.defining_faces(ref)
        faces = tuple(
            sorted(i for i in (loaded.index_of(view.face(r)) for r in refs) if i is not None)
        )
        if not faces:
            continue
        record = _plain(view.record(ref).to_dict())
        members = view.members(ref)
        name = family
        if members:
            # Quiddity names a pattern's member holes (quiddity#790).
            kind = type(view.record(ref)).__name__
            record["kind"] = HOLE_PATTERN_KINDS.get(kind, kind)
            name = f"{family}.{record['kind']}"
        feature = Feature(
            feature_id(name, faces),
            family,
            record,
            faces,
            tuple(ids[m] for m in members if m in ids),
            tuple(f"member {m} has no faces in this part" for m in members if m not in ids),
        )
        ids[ref] = feature.id
        features.append(feature)
    features.extend(_hole_sets(features))
    return features


def _hole_sets(features: list[Feature]) -> list[Feature]:
    """Group identical holes that no recognised pattern claims.

    Quiddity recognises layouts (circles, rows, grids, rectangles); a pair of
    holes, or equal holes scattered over a part, are none of those, but they
    are specified together -- one question, one position tolerance. Holes group
    when diameter, depth, bottom, entry treatment and drilling direction are
    all the same. These are sets of equal holes, not recognised layouts:
    nothing is claimed about their spacing.
    """
    claimed = {m for f in features if f.family == "hole_patterns" for m in f.members}
    groups: dict[tuple, list[Feature]] = {}
    for hole in features:
        if hole.family != "holes" or hole.id in claimed:
            continue
        r = hole.record
        key = (
            _rounded(r.get("diameter")),
            _rounded(r.get("depth")),
            r.get("bottom"),
            bool(r.get("cbore")),
            bool(r.get("spotface")),
            bool(r.get("csink")),
            tuple(_rounded(v) for v in r.get("axis") or ()),
        )
        groups.setdefault(key, []).append(hole)
    out = []
    for members in groups.values():
        if len(members) < 2:
            continue
        faces = tuple(sorted(i for h in members for i in h.faces))
        record = {
            "kind": "set",
            "diameter": members[0].record.get("diameter"),
            "holes": [h.record for h in members],
        }
        out.append(
            Feature(
                feature_id("hole_patterns.set", faces),
                "hole_patterns",
                record,
                faces,
                tuple(h.id for h in members),
            )
        )
    return out


def _rounded(value: Any) -> Any:
    return None if value is None else round(float(value), 6)


def _plain(value: Any) -> Any:
    """Records as JSON-safe values: tuples to lists, non-finite floats to None."""
    if isinstance(value, dict):
        return {str(k): _plain(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [_plain(v) for v in value]
    if isinstance(value, float) and not math.isfinite(value):
        return None
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    return str(value)


def _vec(d: Any, sign: float = 1.0) -> tuple[float, float, float]:
    return (round(sign * d.X(), 9), round(sign * d.Y(), 9), round(sign * d.Z(), 9))


def _pnt(p: Any) -> tuple[float, float, float]:
    return (round(p.X(), 6), round(p.Y(), 6), round(p.Z(), 6))
