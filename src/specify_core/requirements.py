"""Write what OCCT cannot: threads, knurls and the general tolerance.

OCCT's XCAF has no entity for a thread, a knurl or a general tolerance, so
after OCCT writes the file these are appended to its Part 21 text, each twice:

* **Draftwright's text route.** ``PROPERTY_DEFINITION('manufacturing
  requirement', <kind>, #product_definition)`` with one descriptive item
  holding a sentence, and -- for one on a face -- a shape aspect on the face
  linked to a ``DRAUGHTING_CALLOUT`` named after the requirement, whose
  presentation is a leader. Draftwright reads these today. The sentences
  follow the forms its parser accepts (``pmi_lowering.py``), but state only
  what is known: a drill point only for a drilled hole, chamfers only when
  there are some. A sentence without them is honest and, until Draftwright
  accepts the shorter forms, left unread there.
* **Standard constructs.** A CAx-IF user defined attribute (UDA practice
  v1.8) with the values as separate named items -- the names from AP242's own
  ``thread`` and ``turned_knurl`` -- on a shape aspect of its own, as OCCT's
  reader ignores PMI-linked ones; and the CAx-IF PMI practice's 'default
  tolerances' property with its 'tolerance class'. No ``default_tolerance_table``
  is written: its cells hold decimal places or limits, not a class.

Each new datum also gets a datum feature symbol, linked to its
``DATUM_FEATURE``: a viewer that shows only presentation then shows the datum
where the symbol is. Nothing else is drawn as text; Draftwright reads none of
the presentation.

Faces are found in the written file by reading it again: the reader says
which ``ADVANCED_FACE`` each of the part's faces came from, so in an assembly
only that part's faces are found. In a file OCCT has just written, an entity's
rank is its ``#`` id; that is checked, not assumed.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from OCP.Bnd import Bnd_Box
from OCP.BRep import BRep_Tool
from OCP.BRepBndLib import BRepBndLib
from OCP.TopAbs import TopAbs_VERTEX
from OCP.TopExp import TopExp_Explorer
from OCP.TopoDS import TopoDS

from . import lettering
from .load import LoadedPart, load

KINDS = {"internal": "internal thread", "external": "external thread"}
#: The part's notes, each written as a manufacturing requirement of this kind.
PART_NOTES = {
    # As Draftwright reads a document default (its GRM-03 fixture): the only
    # 'surface texture' in the file, "Ra 3.2 µm unless otherwise specified". A
    # finish on faces is a 'surface finish', or the two would be ambiguous.
    "surface_finish": "surface texture",
    "coating": "surface treatment",
    "heat_treatment": "heat treatment",
    "edges": "edge condition",
}

Point = tuple[float, float, float]

_ENTITY = re.compile(r"#(\d+)\s*=\s*(.*?);(?=\s*#\d+\s*=|\s*ENDSEC)", re.S)


@dataclass
class Appended:
    """What was added, for the report and for verification."""

    texts: dict[str, str]  # requirement kind -> sentence, one per requirement
    faces: dict[str, list[int]]  # requirement label -> face indices it is on
    count: int


def sentence(req: dict[str, Any]) -> str:
    """The requirement as a sentence in the form Draftwright reads."""
    if req["kind"] == "thread" and req["side"] == "external":
        return (
            f"{req['designation']} x {req['pitch']:g}-{req['class']} {req['hand']}, "
            f"full available length on nominal DIA {req['nominal']:g} region"
        )
    if req["kind"] == "thread":
        head = f"{req['designation']} x {req['pitch']:g}-{req['class']} {req['hand']}"
        if req.get("through"):
            return (
                f"{head}, full thread through; DIA {req['drill_diameter']:g} tapping drill through"
            )
        text = (
            f"{head}, {req['full_thread']:g} mm minimum full thread; "
            f"DIA {req['drill_diameter']:g} tapping drill x {req['drill_depth']:g} mm "
            "full-diameter depth"
        )
        return text + ("; conventional 118 degree drill point" if req.get("drill_point") else "")
    if req["kind"] == "knurl":
        chamfer = f", full width between C{req['chamfer']:g} chamfers" if req.get("chamfer") else ""
        return (
            f"{req['pattern'].capitalize()} knurl, {req['pitch']:g} mm pitch{chamfer}, "
            f"DIA {req['diameter']:g} mm maximum after knurling; cut or formed process permitted"
        )
    if req["kind"] == "finish":
        return f"{req['value']} µm"
    if req["kind"] == "surface texture":
        return f"{req['value']} µm unless otherwise specified"
    if req["kind"] in PART_NOTES.values():
        return str(req["value"])
    raise ValueError(f"no sentence for {req['kind']!r}")


def attributes(req: dict[str, Any]) -> list[tuple[str, str | float]]:
    """The UDA items: names from AP242's thread and turned_knurl parameters."""
    if req["kind"] == "thread":
        items: list[tuple[str, str | float]] = [
            ("thread side", req["side"]),
            ("designation", f"{req['designation']}x{req['pitch']:g}"),
            ("nominal size", req["designation"]),
            ("pitch", float(req["pitch"])),
            ("fit class", req["class"]),
            ("hand", "right" if req["hand"] == "RH" else "left"),
        ]
        if req["side"] == "internal":
            items.append(("tapping drill diameter", float(req["drill_diameter"])))
            if req.get("through"):
                items.append(("through", "true"))
            else:
                items += [
                    ("tapping drill depth", float(req["drill_depth"])),
                    ("minimum full thread", float(req["full_thread"])),
                ]
        elif req.get("length") is not None:
            items.append(("thread length", float(req["length"])))
        return items
    return [
        ("pattern", req["pattern"]),
        ("diametral pitch", float(req["pitch"])),
        ("major diameter", float(req["diameter"])),
    ]


def face_entities(path: str | Path, part: int = 0) -> dict[int, int]:
    """Face index -> ``#id`` of its ADVANCED_FACE in ``path``, for its ``part``-th part.

    OCCT numbers entities by rank, their order in the file; a file OCCT did not
    write need not number them in order, so ranks are mapped to ids.
    """
    return face_entities_of(load(path, part=part, gdt=False))


def face_entities_of(loaded: LoadedPart) -> dict[int, int]:
    """``face_entities`` of a part already loaded from the file."""
    ids = [int(i) for i, _ in entities_of(loaded.path.read_text(errors="replace"))]
    return {index: ids[rank - 1] for index, rank in loaded.face_ranks().items()}


def entities_of(text: str) -> list[tuple[str, str]]:
    """``(id, body)`` of each entity in a Part 21 file, in file order, each on one line."""
    return _ENTITY.findall(re.sub(r"\s*\n\s*", " ", text))


def append(
    path: Path,
    loaded: LoadedPart,
    intent,
    existing: frozenset[str] = frozenset(),
    faces: dict[int, int] | None = None,
) -> Appended:
    """Append the requirements of ``intent`` that OCCT cannot write to ``path``, and a
    datum feature symbol for each datum not lettered in ``existing`` (the file's own).
    ``faces`` is ``face_entities`` of the part in ``path``, if already known."""
    reqs = [r for r in intent.requirements if r["kind"] in ("thread", "knurl", "finish")]
    general = intent.part.get("general_tolerance")
    notes = {kind: intent.part[key] for key, kind in PART_NOTES.items() if intent.part.get(key)}
    datums = [
        d
        for d in intent.datums
        if d["letter"] not in existing and not d.get("existing") and d.get("faces")
    ]
    if not reqs and not general and not notes and not datums:
        return Appended({}, {}, 0)

    text = path.read_text()
    entities = {int(i): body for i, body in entities_of(text)}
    if faces is None:
        faces = face_entities(path, loaded.binding.part)
    ids = _anchors(entities, faces.values())
    for face in faces.values():
        if not entities.get(face, "").startswith("ADVANCED_FACE("):
            raise RuntimeError(f"entity #{face} is not the face OCCT's reader says it is")

    out = _Part21(max(entities) + 1)
    style = out.add(
        "PRESENTATION_STYLE_ASSIGNMENT((#{}))".format(
            out.add(
                "CURVE_STYLE('',#{},POSITIVE_LENGTH_MEASURE(0.35),#{})".format(
                    out.add("DRAUGHTING_PRE_DEFINED_CURVE_FONT('continuous')"),
                    out.add("DRAUGHTING_PRE_DEFINED_COLOUR('black')"),
                )
            )
        )
    )
    texts: dict[str, str] = {}
    on_faces: dict[str, list[int]] = {}
    # (what it is of, drawn callout, its plane, whether that is semantic PMI)
    links: list[tuple[int, int, int, bool]] = []
    plane_style = _plane_style(out)
    for req in reqs:
        kind = {"thread": KINDS.get(req.get("side", ""), ""), "knurl": "knurl"}.get(
            req["kind"], "surface finish"
        )
        words = sentence(req)
        label = f"{kind} {len(texts) + 1}"
        texts[label] = words
        on_faces[label] = list(req["faces"])
        _text_route(out, ids, kind, words)
        aspect = _aspect(out, ids, faces, kind, req["faces"])
        name = kind.capitalize() + " requirement"
        lines = [list(_leader(loaded, req["faces"][0]))]
        callout = _callout(out, "note", name, style, lines)
        plane = _annotation_plane(out, name, plane_style, callout, lines)
        # A note's faces, not semantic PMI: the plain link, not the PMI one.
        links.append((aspect, callout, plane, False))
        if req["kind"] != "finish":
            _attributes(out, ids, faces, kind, req)
    for kind, words in notes.items():
        texts[kind] = sentence({"kind": kind, "value": words})
        _text_route(out, ids, kind, texts[kind])
    if general:
        texts["general tolerances"] = general
        _text_route(out, ids, "general tolerances", general)
        _default_tolerances(out, ids, general)
    # A viewer reading only presentation shows a datum where its symbol is: the
    # symbol linked to the datum feature gives the datum a position.
    features = _datum_features(entities, ids["pds"])
    symbols = _DatumSymbols(loaded.shape)
    for d in datums:
        if d["letter"] in features:
            lines = symbols.draw(_tip(loaded, d["faces"][0]), d["letter"])
            name = f"Datum {d['letter']}"
            callout = _callout(out, "datum", name, style, lines)
            plane = _annotation_plane(out, name, plane_style, callout, lines)
            links.append((features[d["letter"]], callout, plane, True))
    if links:
        # The model holds the annotation planes, each holding its callout
        # (CAx-IF PMI practice, 9.1, Fig. 87).
        model = out.add(
            "DRAUGHTING_MODEL('',({}),#{})".format(
                ",".join(f"#{p}" for _, _, p, _ in links), ids["context"]
            )
        )
        for of, callout, _, semantic in links:
            # 'PMI representation to presentation link' is for a semantic PMI
            # item only (7.3); what a callout is merely on gets the plain link.
            name = "PMI representation to presentation link" if semantic else ""
            out.add(f"DRAUGHTING_MODEL_ITEM_ASSOCIATION({_s(name)},'',#{of},#{model},#{callout})")

    marker = re.search(r"\nENDSEC;\s*\nEND-ISO-10303-21;\s*$", text)
    if marker is None:
        raise RuntimeError(f"{path} does not end as OCCT writes a Part 21 file")
    path.write_text(text[: marker.start()] + "\n" + "\n".join(out.lines) + text[marker.start() :])
    return Appended(texts, on_faces, len(out.lines))


class _Part21:
    def __init__(self, first: int) -> None:
        self.next = first
        self.lines: list[str] = []

    def add(self, body: str) -> int:
        self.lines.append(f"#{self.next}={body};")
        self.next += 1
        return self.next - 1


def _s(text: str) -> str:
    return "'" + text.replace("'", "''") + "'"


def _anchors(entities: dict[int, str], faces=None) -> dict[str, int]:
    """The ids appended entities hang off: the representation holding the solid and
    its context, its product_definition_shape and product_definition, and mm.
    Given the ids of a part's ``faces``, the solid is the one they bound: in an
    assembly, that part's."""

    def refs(i):
        return [int(x) for x in re.findall(r"#(\d+)", entities[i])]

    # The representation holding the solid: an ADVANCED_BREP_SHAPE_REPRESENTATION,
    # or a plain SHAPE_REPRESENTATION where the file had only that (NIST CTC-05).
    # A plain one may also list the solid beside it (NIST CTC-03).
    solids = {i for i, b in entities.items() if b.startswith("MANIFOLD_SOLID_BREP(")}
    if faces is not None:
        faces = set(faces)
        shells = {
            i
            for i, b in entities.items()
            if b.startswith(("CLOSED_SHELL(", "OPEN_SHELL(")) and faces & set(refs(i))
        }
        solids = {i for i in solids if shells & set(refs(i))}
    breps = []
    for kind in ("ADVANCED_BREP_SHAPE_REPRESENTATION(", "SHAPE_REPRESENTATION("):
        breps = breps or [
            i for i, b in entities.items() if b.startswith(kind) and solids & set(refs(i))
        ]
    if len(breps) != 1:
        raise RuntimeError(f"expected one B-rep representation, found {len(breps)}")
    brep = breps[0]
    related = {brep} | {
        r
        for i, b in entities.items()
        if "REPRESENTATION_RELATIONSHIP" in b and brep in refs(i)
        for r in refs(i)
    }
    # The solid's own definition first: in a part stored as an assembly holding
    # one solid, the assembly's shape is related to the solid's too.
    sdrs = sorted(
        (
            i
            for i, b in entities.items()
            if b.startswith("SHAPE_DEFINITION_REPRESENTATION(") and refs(i)[1] in related
        ),
        key=lambda i: refs(i)[1] != brep,
    )
    if not sdrs:
        raise RuntimeError("no product definition has the B-rep representation")
    pds = refs(sdrs[0])[0]
    mm = next(
        (i for i, b in entities.items() if "SI_UNIT(.MILLI.,.METRE.)" in b.replace(" ", "")),
        None,
    )
    return {
        "brep": brep,
        "context": refs(brep)[-1],
        "pds": pds,
        "pd": refs(pds)[-1],
        "mm": mm,
    }


def _text_route(out: _Part21, ids, kind: str, words: str) -> None:
    item = out.add(f"DESCRIPTIVE_REPRESENTATION_ITEM({_s(kind)},{_s(words)})")
    rep = out.add(f"REPRESENTATION({_s(kind + ' requirement')},(#{item}),#{ids['context']})")
    prop = out.add(f"PROPERTY_DEFINITION('manufacturing requirement',{_s(kind)},#{ids['pd']})")
    out.add(f"PROPERTY_DEFINITION_REPRESENTATION(#{prop},#{rep})")


def _aspect(out: _Part21, ids, faces, kind: str, face_ids) -> int:
    aspect = out.add(f"SHAPE_ASPECT({_s(kind)},'',#{ids['pds']},.T.)")
    for i in face_ids:
        out.add(
            f"GEOMETRIC_ITEM_SPECIFIC_USAGE({_s(kind)},'',#{aspect},#{ids['brep']},#{faces[i]})"
        )
    return aspect


def _plane_style(out: _Part21) -> int:
    """The style an annotation plane takes, as NIST's test files give it."""
    colour = out.add("COLOUR()")
    shade = out.add(f"FILL_AREA_STYLE_COLOUR('',#{colour})")
    fill = out.add(f"FILL_AREA_STYLE('',(#{shade}))")
    return out.add(f"PRESENTATION_STYLE_ASSIGNMENT((#{fill}))")


def _annotation_plane(out: _Part21, name: str, style: int, callout: int, lines) -> int:
    """The plane ``lines`` are drawn in, holding ``callout``: every drawn
    annotation must have one (CAx-IF PMI practice, 9.1)."""
    origin, normal, along = _plane_of([p for line in lines for p in line])
    placement = out.add(
        "AXIS2_PLACEMENT_3D('',#{},#{},#{})".format(
            out.add("CARTESIAN_POINT('',({:.4f},{:.4f},{:.4f}))".format(*origin)),
            out.add("DIRECTION('',({:.6f},{:.6f},{:.6f}))".format(*normal)),
            out.add("DIRECTION('',({:.6f},{:.6f},{:.6f}))".format(*along)),
        )
    )
    plane = out.add(f"PLANE({_s(name)},#{placement})")
    return out.add(f"ANNOTATION_PLANE({_s(name)},(#{style}),#{plane},(#{callout}))")


def _plane_of(points) -> tuple[Point, Point, Point]:
    """A plane through ``points``: origin, unit normal, unit direction in it.
    Points on one line take a plane through that line."""
    o = points[0]
    offsets = [_sub(p, o) for p in points[1:]]
    along = next((v for v in offsets if _size(v) > 1e-9), (1.0, 0.0, 0.0))
    normal = max((_cross(along, v) for v in offsets), key=_size, default=(0.0, 0.0, 0.0))
    if _size(normal) < 1e-9 * _size(along) ** 2:
        # A single line: any plane through it -- the one whose normal is
        # square to it and to the axis it is most nearly square to.
        axis = min(_AXES, key=lambda a: abs(_dot(a, along)))
        normal = _cross(along, axis)
    return o, _unit(normal), _unit(along)


_AXES = ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))


def _sub(a, b) -> Point:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def _dot(a, b) -> float:
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def _cross(a, b) -> Point:
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _size(v) -> float:
    return _dot(v, v) ** 0.5


def _unit(v) -> Point:
    n = _size(v)
    return (v[0] / n, v[1] / n, v[2] / n)


def _callout(out: _Part21, kind: str, name: str, style: int, lines) -> int:
    """A callout drawn as polylines ``lines``, one tessellated curve set."""
    points = [p for line in lines for p in line]
    coordinates = out.add(
        "COORDINATES_LIST('',{},({}))".format(
            len(points), ",".join("({:.4f},{:.4f},{:.4f})".format(*p) for p in points)
        )
    )
    spans, first = [], 1
    for line in lines:
        spans.append("(" + ",".join(str(first + k) for k in range(len(line))) + ")")
        first += len(line)
    curve = out.add(f"TESSELLATED_CURVE_SET('',#{coordinates},({','.join(spans)}))")
    # Named as the CAx-IF presentation practice requires (8.1.1, 8.4): the set
    # by its PMI type, the occurrence as its callout.
    geometry = out.add(f"TESSELLATED_GEOMETRIC_SET({_s(kind)},(#{curve}))")
    occurrence = out.add(f"TESSELLATED_ANNOTATION_OCCURRENCE({_s(name)},(#{style}),#{geometry})")
    return out.add(f"DRAUGHTING_CALLOUT({_s(name)},(#{occurrence}))")


def _attributes(out: _Part21, ids, faces, kind: str, req) -> None:
    aspect = _aspect(out, ids, faces, kind, req["faces"])
    items = []
    for name, value in attributes(req):
        if isinstance(value, float):
            items.append(
                out.add(
                    f"MEASURE_REPRESENTATION_ITEM({_s(name)},LENGTH_MEASURE({value!r}),#{ids['mm']})"
                )
            )
        else:
            items.append(out.add(f"DESCRIPTIVE_REPRESENTATION_ITEM({_s(name)},{_s(value)})"))
    prop = out.add(f"PROPERTY_DEFINITION({_s(kind)},'pmi-assist',#{aspect})")
    general = out.add("GENERAL_PROPERTY('','user defined attribute',$)")
    out.add(f"GENERAL_PROPERTY_ASSOCIATION('',$,#{general},#{prop})")
    rep = out.add(
        f"REPRESENTATION({_s(kind)},({','.join(f'#{i}' for i in items)}),#{ids['context']})"
    )
    out.add(f"PROPERTY_DEFINITION_REPRESENTATION(#{prop},#{rep})")


def _default_tolerances(out: _Part21, ids, tolerance_class: str) -> None:
    context = out.add("REPRESENTATION_CONTEXT('','default setting')")
    item = out.add(f"DESCRIPTIVE_REPRESENTATION_ITEM('tolerance class',{_s(tolerance_class)})")
    rep = out.add(f"REPRESENTATION('default tolerances',(#{item}),#{context})")
    prop = out.add(f"PROPERTY_DEFINITION('default tolerances','',#{ids['pds']})")
    out.add(f"PROPERTY_DEFINITION_REPRESENTATION(#{prop},#{rep})")


def _leader(loaded: LoadedPart, face_id: int):
    """A leader from a point on the face out to where its label would sit. The
    presentation only says where; the semantics are in the text and the UDA."""
    tip = _tip(loaded, face_id)
    return ((tip[0] + 10, tip[1], tip[2] + 10), tip)


def _tip(loaded: LoadedPart, face_id: int) -> Point:
    """A point on the face for a leader to end at: one of its vertices."""
    explorer = TopExp_Explorer(loaded.face(face_id), TopAbs_VERTEX)
    p = BRep_Tool.Pnt_s(TopoDS.Vertex_s(explorer.Current()))
    return (p.X(), p.Y(), p.Z())


def _datum_features(entities: dict[int, str], pds: int) -> dict[str, int]:
    """Datum letter -> the DATUM_FEATURE it is established by, in a file OCCT wrote,
    of the part whose product_definition_shape is ``pds``."""
    letters = {
        i: m.group(1)
        for i, body in entities.items()
        if body.startswith("DATUM(") and (m := re.search(r"'([^']*)'\s*\)$", body))
    }
    out: dict[str, int] = {}
    for body in entities.values():
        if body.startswith("SHAPE_ASPECT_RELATIONSHIP("):
            feature, datum = (int(x) for x in re.findall(r"#(\d+)", body)[-2:])
            body = entities.get(feature, "")
            if (
                datum in letters
                and body.startswith("DATUM_FEATURE(")
                and pds in (int(x) for x in re.findall(r"#(\d+)", body))
            ):
                out.setdefault(letters[datum], feature)
    return out


class _DatumSymbols:
    """Datum feature symbols -- the letter boxed, a stem to a triangle on the face --
    standing above the part in the plane of its two longest extents, on the side
    that plane faces, sized to the part and kept off one another."""

    def __init__(self, shape) -> None:
        box = Bnd_Box()
        BRepBndLib.Add_s(shape, box)
        lo, hi = box.CornerMin(), box.CornerMax()
        self.lo, self.hi = (lo.X(), lo.Y(), lo.Z()), (hi.X(), hi.Y(), hi.Z())
        extent = [self.hi[k] - self.lo[k] for k in range(3)]
        self.u, self.v, self.w = sorted(range(3), key=lambda k: -extent[k])
        # Toward the viewer: u x v, which is +w for a cyclic order.
        cyclic = (self.u, self.v, self.w) in ((0, 1, 2), (1, 2, 0), (2, 0, 1))
        self.depth = self.hi[self.w] if cyclic else self.lo[self.w]
        diagonal = sum(e * e for e in extent) ** 0.5
        self.height = min(max(diagonal / 30, 0.5), 5.0)
        self.right = self.lo[self.u] - diagonal  # the last symbol's right edge

    def _at(self, a: float, b: float) -> Point:
        p = [0.0, 0.0, 0.0]
        p[self.u], p[self.v], p[self.w] = a, b, self.depth
        return (p[0], p[1], p[2])

    def _off(self, tip: Point, a: float, b: float) -> Point:
        p = list(tip)
        p[self.u] += a
        p[self.v] += b
        return (p[0], p[1], p[2])

    def draw(self, tip: Point, letter: str) -> list[list[Point]]:
        size = 1.6 * self.height
        left = max(tip[self.u] - size / 2, self.right + size / 2)
        self.right = left + size
        bottom = self.hi[self.v] + size
        box = [self._at(left, bottom), self._at(left + size, bottom)]
        box += [self._at(left + size, bottom + size), self._at(left, bottom + size), box[0]]
        strokes, width = lettering.line(letter, self.height)
        a, b = left + (size - width) / 2, bottom + (size - self.height) / 2
        drawn = [[self._at(a + x, b + y) for x, y in s] for s in strokes]
        h = self.height / 2
        triangle = [self._off(tip, -h, 0), self._off(tip, h, 0), self._off(tip, 0, h)]
        triangle.append(triangle[0])
        stem = [self._at(left + size / 2, bottom), self._off(tip, 0, h)]
        return [box, *drawn, stem, triangle]
