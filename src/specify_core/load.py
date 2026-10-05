"""Load a STEP file once, through XCAF, and number its faces.

Everything downstream -- recognition, existing PMI, intent, the writer and the
browser's picking -- refers to faces by their position in one face map of one
loaded shape. Loading through XCAF rather than Quiddity's geometry-only import
keeps names, colours and any PMI already in the file, so the writer can add to
the document instead of rebuilding it.

A part is often stored as an assembly holding one solid (the NIST test parts
are). Its faces are numbered on that solid, not on the placed instance: PMI
attaches to the solid's label, and the reader returns the solid's faces.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from build123d import Compound, Part, Solid
from OCP.IFSelect import IFSelect_RetDone
from OCP.STEPCAFControl import STEPCAFControl_Reader
from OCP.STEPControl import STEPControl_Controller
from OCP.TCollection import TCollection_ExtendedString
from OCP.TDF import TDF_Label, TDF_LabelSequence
from OCP.TDocStd import TDocStd_Document
from OCP.TopAbs import TopAbs_FACE
from OCP.TopExp import TopExp
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

    def to_dict(self) -> dict[str, Any]:
        return {
            "source_sha256": self.source_sha256,
            "face_count": self.face_count,
            "loader_version": self.loader_version,
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


def load(path: str | Path, *, gdt: bool = True) -> LoadedPart:
    """Read ``path`` through XCAF with GD&T, names and colours kept.

    Without ``gdt`` the file's PMI is left out of the document: the writer adds
    to a part that has PMI without writing that PMI back (see ``merge``).
    """
    path = Path(path)
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
    parts = [label for label in _simple_shapes(roots.Value(1)) if _faces(label).Extent()]
    if len(parts) != 1:
        raise ValueError(f"{path}: expected one part, found {len(parts)} shapes with faces")
    label = parts[0]
    shape = XCAFDoc_ShapeTool.GetShape_s(label)
    faces = _faces(label)
    binding = Binding(hashlib.sha256(path.read_bytes()).hexdigest(), faces.Extent())
    return LoadedPart(path, doc, label, shape, _wrap(shape), faces, binding)


def _simple_shapes(label: TDF_Label) -> list[TDF_Label]:
    """The distinct non-assembly shapes under ``label``, following references."""
    if XCAFDoc_ShapeTool.IsReference_s(label):
        referred = TDF_Label()
        XCAFDoc_ShapeTool.GetReferredShape_s(label, referred)
        label = referred
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
