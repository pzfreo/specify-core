"""Semantic PMI already present in the loaded document, keyed by face index.

The interview must never ask for something the file already states, so each
dimension, geometric tolerance and datum is resolved to the faces it references.

OCCT's reader leaves a datum without faces when its feature names them as a
set (NIST CTC-01's datums B and C, each a hole's two half-cylinders); those are
read from the STEP text instead. It also builds one datum per tolerance that
cites it; each letter is reported once per set of faces.

In an assembly, only the loaded part's PMI is read: what references none of its
faces belongs to another part.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from OCP.TDF import TDF_Label, TDF_LabelSequence
from OCP.TopAbs import TopAbs_FACE
from OCP.TopExp import TopExp_Explorer
from OCP.XCAFDoc import (
    XCAFDoc_Datum,
    XCAFDoc_Dimension,
    XCAFDoc_DocumentTool,
    XCAFDoc_GeomTolerance,
)

from . import p21
from .load import LoadedPart
from .magnitudes import tolerance_magnitudes
from .requirements import PART_NOTES, entities_of, face_entities


@dataclass(frozen=True)
class ExistingPmi:
    kind: str  # dimension | geometric_tolerance | datum | note
    type: str  # OCCT type name without prefix, the datum letter, or the note's kind
    faces: tuple[int, ...]
    #: Second reference set, for two-sided dimensions.
    faces2: tuple[int, ...] = ()
    value: float | None = None
    upper: float | None = None
    lower: float | None = None
    datums: tuple[str, ...] = ()

    def to_dict(self) -> dict[str, Any]:
        out = {k: v for k, v in self.__dict__.items() if v not in (None, ())}
        # A datum the file attaches to no face still has faces: none.
        out.setdefault("faces", ())
        for key in ("faces", "faces2", "datums"):
            if key in out:
                out[key] = list(out[key])
        return out


def read_existing(loaded: LoadedPart) -> list[ExistingPmi]:
    tool = XCAFDoc_DocumentTool.DimTolTool_s(loaded.doc.Main())
    shape_tool = XCAFDoc_DocumentTool.ShapeTool_s(loaded.doc.Main())
    out: list[ExistingPmi] = []

    labels = TDF_LabelSequence()
    tool.GetDimensionLabels(labels)
    for label in _each(labels):
        obj = XCAFDoc_Dimension.Set_s(label).GetObject()
        first, second = _refs(tool, shape_tool, loaded, label)
        if _elsewhere(loaded, first, second):
            continue
        kind = _name(obj.GetType(), "XCAFDimTolObjects_DimensionType_")
        if kind == "CommonLabel":
            # Presentation with no value -- a callout such as the ones written
            # for threads and knurls, which ``notes`` reads -- states no size.
            continue
        # Read back as the file states it: NIST CTC-01's 60° angle reads 60.0,
        # not radians, and an inch file's nominal stays in inches.
        value = obj.GetValue()
        upper = lower = None
        # OCCT's reader loses the sign of the lower deviation, and drops the limits
        # (and garbles the value) when both deviations are below nominal. Read
        # values are reliable for the common H-hole and symmetric cases only.
        if obj.IsDimWithPlusMinusTolerance():
            upper, lower = obj.GetUpperTolValue(), -abs(obj.GetLowerTolValue())
        out.append(ExistingPmi("dimension", kind, first, second, value, upper, lower))

    labels = TDF_LabelSequence()
    tool.GetGeomToleranceLabels(labels)
    magnitudes = tolerance_magnitudes(loaded.path) if labels.Length() else {}
    for label in _each(labels):
        obj = XCAFDoc_GeomTolerance.Set_s(label).GetObject()
        value = obj.GetValue()
        if value == 0.0:
            # OCCT reads 0 for magnitudes held in complex entities (every NIST
            # file); the STEP text has them. A value OCCT reports is preferred.
            name = obj.GetSemanticName()
            value = magnitudes.get(name.ToCString() if name else "", value)
        first, _ = _refs(tool, shape_tool, loaded, label)
        if _elsewhere(loaded, first):
            continue
        datums = TDF_LabelSequence()
        tool.GetDatumOfTolerLabels_s(label, datums)
        letters = tuple(_datum_letter(d) for d in _each(datums))
        kind = _name(obj.GetType(), "XCAFDimTolObjects_GeomToleranceType_")
        out.append(ExistingPmi("geometric_tolerance", kind, first, value=value, datums=letters))

    labels = TDF_LabelSequence()
    tool.GetDatumLabels(labels)
    resolved: dict[str, tuple[int, ...]] | None = None
    seen = set()
    for label in _each(labels):
        letter = _datum_letter(label)
        first, _ = _refs(tool, shape_tool, loaded, label)
        if not first:
            if resolved is None:
                resolved = datum_faces(loaded.path, loaded.binding.part)
            first = resolved.get(letter, ())
        if _elsewhere(loaded, first):
            continue
        if (letter, first) not in seen:
            seen.add((letter, first))
            out.append(ExistingPmi("datum", letter, first))
    return out + notes(loaded.path, loaded.binding.part)


def _elsewhere(loaded: LoadedPart, *faces: tuple[int, ...]) -> bool:
    """Whether PMI referencing these faces -- none of this part's -- is another
    part's, in an assembly."""
    return loaded.part_count > 1 and not any(faces)


_TOLERANCE_CLASS = re.compile(
    r"DESCRIPTIVE_REPRESENTATION_ITEM\(\s*'tolerance class'\s*,\s*'([^']*)'"
)


def part_settings(loaded: LoadedPart) -> dict[str, str]:
    """The material and general tolerance the file states, as the writer writes them.
    Read from the text: OCCT's material reader takes any named representation.
    None for a part of an assembly: the text does not say whose they are."""
    if loaded.part_count > 1:
        return {}
    text = loaded.path.read_text(errors="replace")
    entities = {int(i): body for i, body in entities_of(text)}
    out = {}
    for body in entities.values():
        if body.startswith("REPRESENTATION('material name',"):
            item = entities.get(int(re.findall(r"#(\d+)", body)[0]), "")
            if m := re.match(r"DESCRIPTIVE_REPRESENTATION_ITEM\(\s*'([^']*)'", item):
                out["material"] = p21.unescape(m.group(1))
    found = _TOLERANCE_CLASS.search(text)
    if found:
        out["general_tolerance"] = found.group(1)
    # The part's notes, as requirements.append writes them -- and the finish as
    # it was written before it took Draftwright's form.
    for key, kind in PART_NOTES.items():
        kinds = (kind, "general surface texture") if key == "surface_finish" else (kind,)
        for kind in kinds:
            for m in re.finditer(
                rf"DESCRIPTIVE_REPRESENTATION_ITEM\(\s*'{kind}'\s*,\s*'((?:[^']|'')*)'", text
            ):
                words = p21.unescape(m.group(1).replace("''", "'"))
                if key == "surface_finish":
                    default = _DEFAULT_FINISH.fullmatch(words)
                    if not default:
                        continue  # a finish on faces, written before it was 'surface finish'
                    words = default.group(1)
                out.setdefault(key, words)
    return out


_DEFAULT_FINISH = re.compile(r"(.*?) µm unless otherwise (?:specified|stated)")


def notes(path: Path, part: int = 0) -> list[ExistingPmi]:
    """Threads and knurls the file states as ``requirements.append`` writes them."""
    faces = {face: i for i, face in face_entities(path, part).items()}
    found: dict[str, set[int]] = {}
    for kind, face in _NOTE.findall(path.read_text(errors="replace")):
        if int(face) in faces:
            found.setdefault(kind, set()).add(faces[int(face)])
    return [ExistingPmi("note", kind, tuple(sorted(f))) for kind, f in found.items()]


_NOTE = re.compile(
    r"GEOMETRIC_ITEM_SPECIFIC_USAGE\(\s*"
    r"'(internal thread|external thread|knurl|surface finish|surface texture)'"
    r"\s*,\s*'[^']*'\s*,\s*#\d+\s*,\s*#\d+\s*,\s*#(\d+)\s*\)"
)
_USAGES = ("GEOMETRIC_ITEM_SPECIFIC_USAGE(", "ITEM_IDENTIFIED_REPRESENTATION_USAGE(")


def datum_faces(path: Path, part: int = 0) -> dict[str, tuple[int, ...]]:
    """Each datum letter in the STEP file at ``path`` to the faces of its feature
    on its ``part``-th part."""
    entities = {int(i): body for i, body in entities_of(path.read_text(errors="replace"))}
    letters = {
        i: m.group(1)
        for i, body in entities.items()
        if body.startswith("DATUM(") and (m := re.search(r"'([^']*)'\s*\)$", body))
    }
    if not letters:
        return {}
    index = {face: i for i, face in face_entities(path, part).items()}
    usages: dict[int, set[int]] = {}
    parts: dict[int, list[int]] = {}
    features: dict[str, list[int]] = {}
    for body in entities.values():
        refs = [int(x) for x in re.findall(r"#(\d+)", body)]
        if body.startswith(_USAGES):
            usages.setdefault(refs[0], set()).update(index[r] for r in refs[2:] if r in index)
        elif body.startswith("SHAPE_ASPECT_RELATIONSHIP(") and len(refs) == 2:
            relating, related = refs
            if related in letters:
                features.setdefault(letters[related], []).append(relating)
            else:
                parts.setdefault(relating, []).append(related)

    def faces_of(aspect: int, depth: int = 0) -> set[int]:
        found = set(usages.get(aspect, ()))
        if depth < 8:
            for part in parts.get(aspect, ()):
                found |= faces_of(part, depth + 1)
        return found

    return {
        letter: tuple(sorted(set().union(*(faces_of(a) for a in aspects))))
        for letter, aspects in features.items()
    }


def _refs(tool, shape_tool, loaded: LoadedPart, label: TDF_Label) -> tuple[tuple[int, ...], ...]:
    first, second = TDF_LabelSequence(), TDF_LabelSequence()
    tool.GetRefShapeLabel_s(label, first, second)
    return _face_indices(shape_tool, loaded, first), _face_indices(shape_tool, loaded, second)


def _face_indices(shape_tool, loaded: LoadedPart, labels: TDF_LabelSequence) -> tuple[int, ...]:
    found: set[int] = set()
    for label in _each(labels):
        shape = shape_tool.GetShape_s(label)
        if shape.IsNull():
            continue
        explorer = TopExp_Explorer(shape, TopAbs_FACE)
        while explorer.More():
            index = loaded.index_of(explorer.Current())
            if index is not None:
                found.add(index)
            explorer.Next()
    return tuple(sorted(found))


def _datum_letter(label: TDF_Label) -> str:
    name = XCAFDoc_Datum.Set_s(label).GetObject().GetName()
    return name.ToCString() if name is not None else "?"


def _each(labels: TDF_LabelSequence):
    for i in range(1, labels.Length() + 1):
        yield labels.Value(i)


def _name(enum_value: Any, prefix: str) -> str:
    return str(getattr(enum_value, "name", enum_value)).split(".")[-1].removeprefix(prefix)
