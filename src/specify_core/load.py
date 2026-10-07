"""Load a STEP file once, through XCAF, and number its faces.

Everything downstream -- recognition, existing PMI, intent, the writer and the
browser's picking -- refers to faces by their position in one face map of one
loaded shape. Loading through XCAF rather than Quiddity's geometry-only import
keeps names, colours and any PMI already in the file, so the writer can add to
the document instead of rebuilding it.

A part is often stored as an assembly holding one solid (the NIST test parts
are). Its faces are numbered on that solid, not on the placed instance: PMI
attaches to the solid's label, and the reader returns the solid's faces.

An assembly of several parts is loaded one part at a time: ``parts`` lists the
distinct parts (a part placed twice is one part, its PMI shared by both), and
``load(path, part=i)`` loads the i-th, numbered on its own solid as above. All
of a file's parts can be loaded into one document (``load_all``), to be
written together.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from build123d import Compound, Part, Solid
from OCP.gp import gp_Trsf
from OCP.IFSelect import IFSelect_RetDone
from OCP.STEPCAFControl import STEPCAFControl_Reader
from OCP.STEPControl import STEPControl_Controller
from OCP.TCollection import TCollection_ExtendedString
from OCP.TDataStd import TDataStd_Name
from OCP.TDF import TDF_Label, TDF_LabelSequence
from OCP.TDocStd import TDocStd_Document
from OCP.TopAbs import TopAbs_FACE
from OCP.TopExp import TopExp
from OCP.TopLoc import TopLoc_Location
from OCP.TopoDS import TopoDS, TopoDS_Face, TopoDS_Shape
from OCP.TopTools import TopTools_IndexedMapOfShape
from OCP.XCAFApp import XCAFApp_Application
from OCP.XCAFDoc import XCAFDoc_DocumentTool, XCAFDoc_ShapeTool

#: Bumped whenever a change here could renumber faces for the same bytes.
#: 2: an assembly holding one solid is numbered on the solid.
LOADER_VERSION = 2

_XCAF_FORMAT = TCollection_ExtendedString("MDTV-XCAF")


@dataclass(frozen=True)
class Binding:
    """What face indices are only valid against."""

    source_sha256: str
    face_count: int
    loader_version: int = LOADER_VERSION
    #: Which of the file's parts (see ``parts``); 0 for a file of one part.
    part: int = 0

    def to_dict(self) -> dict[str, Any]:
        return {
            "source_sha256": self.source_sha256,
            "face_count": self.face_count,
            "loader_version": self.loader_version,
            "part": self.part,
        }


@dataclass
class LoadedPart:
    """One STEP file, loaded once."""

    path: Path
    doc: TDocStd_Document
    #: The label of the one solid; PMI is attached to its sub-shapes.
    label: TDF_Label
    shape: TopoDS_Shape
    part: Any  # the build123d wrapper Quiddity recognises
    faces: TopTools_IndexedMapOfShape
    binding: Binding
    #: How many parts the file holds; this is part ``binding.part`` of them.
    part_count: int = 1
    #: The reader, kept for what each face was read from (``face_ranks``).
    reader: Any = None
    #: The part's name in the file, if it has one (``_part_name``).
    name: str = ""
    #: Where each instance of the part is placed in the assembly.
    placements: tuple[gp_Trsf, ...] = ()

    def face_ranks(self) -> dict[int, int]:
        """Face index -> the rank in the file of the ``ADVANCED_FACE`` it was read from."""
        session = self.reader.Reader().WS()
        model, transfer = session.Model(), session.TransferReader()
        ranks = {model.Value(k): k for k in range(1, model.NbEntities() + 1)}
        out = {}
        for index in range(self.binding.face_count):
            entity = transfer.EntityFromShapeResult(self.faces.FindKey(index + 1), 1)
            if entity is not None and entity in ranks:
                out[index] = ranks[entity]
        return out

    def face(self, index: int) -> TopoDS_Face:
        """The face with 0-based ``index``."""
        return TopoDS.Face_s(self.faces.FindKey(index + 1))

    def index_of(self, face: Any) -> int | None:
        """The 0-based index of ``face`` (a build123d face or TopoDS shape)."""
        wrapped = getattr(face, "wrapped", face)
        found = self.faces.FindIndex(wrapped)
        return found - 1 if found > 0 else None


def init_step() -> None:
    """Initialise OCCT's STEP controller.

    Without this, ``write.step.schema`` is silently ignored until something has
    read a STEP file in the process, and AP214 without PMI is written.
    """
    STEPControl_Controller.Init_s()


def load(path: str | Path, *, part: int | None = None, gdt: bool = True) -> LoadedPart:
    """Read ``path`` through XCAF with GD&T, names and colours kept, and take its
    ``part``-th part; a file of several parts must say which.

    Without ``gdt`` the file's PMI is left out of the document: the writer adds
    to a part that has PMI without writing that PMI back (see ``merge``).
    """
    loaded = load_all(path, gdt=gdt)
    if part is None:
        if len(loaded) > 1:
            names = ", ".join(f"{i}: {p.name}" for i, p in enumerate(loaded))
            raise ValueError(f"{path} is an assembly of {len(loaded)} parts; choose one ({names})")
        part = 0
    if not 0 <= part < len(loaded):
        raise ValueError(f"{path} has {len(loaded)} part(s); there is no part {part}")
    return loaded[part]


def load_all(path: str | Path, *, gdt: bool = True) -> list[LoadedPart]:
    """Every part of ``path``, loaded into one document."""
    path = Path(path)
    doc, reader, root = _read(path, gdt)
    labels = _part_labels(path, root)
    sha = hashlib.sha256(path.read_bytes()).hexdigest()
    placed: list[list[gp_Trsf]] = [[] for _ in labels]
    for label, location in _instances(root, TopLoc_Location()):
        placed[_index(labels, label)].append(location.Transformation())
    out = []
    for index, label in enumerate(labels):
        shape = XCAFDoc_ShapeTool.GetShape_s(label)
        faces = _faces(label)
        binding = Binding(sha, faces.Extent(), part=index)
        name = _part_name(root, label)
        out.append(
            LoadedPart(
                path,
                doc,
                label,
                shape,
                _wrap(shape),
                faces,
                binding,
                len(labels),
                reader,
                name,
                tuple(placed[index]),
            )
        )
    return out


def parts(path: str | Path) -> list[dict[str, Any]]:
    """The distinct parts of ``path``: each one's name, face count, and where each
    of its instances is placed (a 3x4 matrix, rows of rotation | translation)."""
    path = Path(path)
    doc, _, root = _read(path, gdt=False)  # labels are only valid while doc lives
    labels = _part_labels(path, root)
    placed: list[list[list[list[float]]]] = [[] for _ in labels]
    for label, location in _instances(root, TopLoc_Location()):
        placed[_index(labels, label)].append(_matrix(location.Transformation()))
    out = [
        {
            "part": i,
            "name": _part_name(root, label),
            "faces": _faces(label).Extent(),
            "instances": len(placed[i]),
            "placements": placed[i],
        }
        for i, label in enumerate(labels)
    ]
    del doc
    return out


def _read(path: Path, gdt: bool) -> tuple[TDocStd_Document, STEPCAFControl_Reader, TDF_Label]:
    init_step()
    doc = TDocStd_Document(_XCAF_FORMAT)
    XCAFApp_Application.GetApplication_s().NewDocument(_XCAF_FORMAT, doc)
    reader = STEPCAFControl_Reader()
    reader.SetGDTMode(gdt)
    reader.SetNameMode(True)
    reader.SetColorMode(True)
    if reader.ReadFile(str(path)) != IFSelect_RetDone:
        raise ValueError(f"cannot read STEP file {path}")
    if not reader.Transfer(doc):
        raise ValueError(f"cannot transfer STEP file {path}")

    shape_tool = XCAFDoc_DocumentTool.ShapeTool_s(doc.Main())
    roots = TDF_LabelSequence()
    shape_tool.GetFreeShapes(roots)
    if roots.Length() != 1:
        raise ValueError(f"{path}: expected one part, found {roots.Length()} free shapes")
    return doc, reader, roots.Value(1)


def _part_labels(path: Path, root: TDF_Label) -> list[TDF_Label]:
    labels = [label for label in _simple_shapes(root) if _faces(label).Extent()]
    if not labels:
        raise ValueError(f"{path}: no shape with faces")
    return labels


def _instances(label: TDF_Label, location: TopLoc_Location):
    """Each placed part under ``label``: its label and its location in the assembly."""
    if XCAFDoc_ShapeTool.IsReference_s(label):
        location = location * XCAFDoc_ShapeTool.GetLocation_s(label)
    label = _referred(label)
    if not XCAFDoc_ShapeTool.IsAssembly_s(label):
        if _faces(label).Extent():
            yield label, location
        return
    components = TDF_LabelSequence()
    XCAFDoc_ShapeTool.GetComponents_s(label, components)
    for i in range(1, components.Length() + 1):
        yield from _instances(components.Value(i), location)


def _index(labels: list[TDF_Label], label: TDF_Label) -> int:
    return next(i for i, known in enumerate(labels) if known.IsEqual(label))


def _matrix(trsf: gp_Trsf) -> list[list[float]]:
    return [[round(trsf.Value(r, c), 9) for c in range(1, 5)] for r in range(1, 4)]


def _name(label: TDF_Label) -> str:
    attribute = TDataStd_Name()
    if label.FindAttribute(TDataStd_Name.GetID_s(), attribute):
        return attribute.Get().ToExtString()
    return ""


def _part_name(root: TDF_Label, label: TDF_Label) -> str:
    """A part's name: that of an assembly holding it alone -- an exporter may wrap
    a named part around a solid it calls SOLID -- or else its own."""
    for assembly in _assemblies(root):
        components = TDF_LabelSequence()
        XCAFDoc_ShapeTool.GetComponents_s(assembly, components)
        if components.Length() == 1 and _referred(components.Value(1)).IsEqual(label):
            if _name(assembly):
                return _name(assembly)
    return _name(label)


def _assemblies(label: TDF_Label):
    """Each assembly at or under ``label``, following references."""
    label = _referred(label)
    if XCAFDoc_ShapeTool.IsAssembly_s(label):
        yield label
        components = TDF_LabelSequence()
        XCAFDoc_ShapeTool.GetComponents_s(label, components)
        for i in range(1, components.Length() + 1):
            yield from _assemblies(components.Value(i))


def _referred(label: TDF_Label) -> TDF_Label:
    """What ``label`` places, if it is a reference; else itself."""
    if not XCAFDoc_ShapeTool.IsReference_s(label):
        return label
    referred = TDF_Label()
    XCAFDoc_ShapeTool.GetReferredShape_s(label, referred)
    return referred


def _simple_shapes(label: TDF_Label) -> list[TDF_Label]:
    """The distinct non-assembly shapes under ``label``, following references."""
    label = _referred(label)
    if not XCAFDoc_ShapeTool.IsAssembly_s(label):
        return [label]
    components = TDF_LabelSequence()
    XCAFDoc_ShapeTool.GetComponents_s(label, components)
    out: list[TDF_Label] = []
    for i in range(1, components.Length() + 1):
        for found in _simple_shapes(components.Value(i)):
            if not any(found.IsEqual(seen) for seen in out):
                out.append(found)
    return out


def _faces(label: TDF_Label) -> TopTools_IndexedMapOfShape:
    faces = TopTools_IndexedMapOfShape()
    shape = XCAFDoc_ShapeTool.GetShape_s(label)
    if not shape.IsNull():
        TopExp.MapShapes_s(shape, TopAbs_FACE, faces)
    return faces


def _wrap(shape: TopoDS_Shape) -> Any:
    """The build123d object Quiddity expects, sharing the same TopoDS faces."""
    solids = Compound(shape).solids()
    if len(solids) == 1:
        return Solid(solids[0].wrapped)
    return Part(shape)
