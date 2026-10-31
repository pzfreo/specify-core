"""For #87: authored face names on an OCP plate rebuilt at a different thickness.

Run with cadquery-ocp-novtk==8.0.1.1.0: python named_plate.py [output-directory].
Names are assigned by the author from primitive faces and boolean history, never
inferred from geometry. Reverse the shell's face traversal in the new export
to exercise changed face indices independently of the dimension edit.
"""
from pathlib import Path
import gzip
import sys

from OCP.BRep import BRep_Builder
from OCP.BRepAlgoAPI import BRepAlgoAPI_Cut
from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox, BRepPrimAPI_MakeCylinder
from OCP.IFSelect import IFSelect_ReturnStatus
from OCP.Interface import Interface_Static
from OCP.STEPCAFControl import STEPCAFControl_Writer
from OCP.STEPControl import STEPControl_StepModelType
from OCP.TCollection import TCollection_ExtendedString
from OCP.TDataStd import TDataStd_Name
from OCP.TDocStd import TDocStd_Document
from OCP.TopAbs import TopAbs_FACE
from OCP.TopExp import TopExp_Explorer
from OCP.TopoDS import TopoDS_Shell, TopoDS_Solid
from OCP.XCAFDoc import XCAFDoc_DocumentTool
from OCP.gp import gp_Ax2, gp_Dir, gp_Pnt


def faces(shape):
    result = []
    ex = TopExp_Explorer(shape, TopAbs_FACE)
    while ex.More():
        result.append(ex.Current())
        ex.Next()
    return result


def build(thickness, reverse, path):
    box = BRepPrimAPI_MakeBox(80, 50, thickness).Shape()
    named = [(f, f"f:named_plate.body_make_box1.{n};M:named_plate.body")
             for n, f in enumerate(faces(box), 1)]
    body = box
    # Two deliberately unequal through bores. The cylindrical side is the first
    # face made by MakeCylinder; boolean history tracks all surviving faces.
    for n, (x, radius) in enumerate([(20, 3), (57, 5)], 1):
        tool = BRepPrimAPI_MakeCylinder(
            gp_Ax2(gp_Pnt(x, 25, -1), gp_Dir(0, 0, 1)), radius, thickness + 2).Shape()
        cut = BRepAlgoAPI_Cut(body, tool)
        cut.Build()
        sources = named + [(faces(tool)[0],
                            f"f:named_plate.body_make_cylinder{n}.1;M:named_plate.body")]
        next_named = []
        for face, name in sources:
            modified = list(cut.Modified(face))
            if modified:
                next_named.extend((f, name) for f in modified)
            elif not cut.IsDeleted(face):
                next_named.append((face, name))
        body, named = cut.Shape(), next_named
    exported = faces(body)
    assert len(exported) == 8
    assert all(sum(f.IsSame(s) for s, _ in named) == 1 for f in exported)
    # A shell of the same faces changes only traversal order; it also makes
    # index migration deterministic across OCCT versions. Geometry is unchanged.
    shell, builder = TopoDS_Shell(), BRep_Builder()
    builder.MakeShell(shell)
    for f in reversed(exported) if reverse else exported:
        builder.Add(shell, f)
    solid = TopoDS_Solid()
    builder.MakeSolid(solid)
    builder.Add(solid, shell)
    doc = TDocStd_Document(TCollection_ExtendedString("BinXCAF"))
    shapes = XCAFDoc_DocumentTool.ShapeTool_s(doc.Main())
    label = shapes.AddShape(solid, False)
    TDataStd_Name.Set_s(label, TCollection_ExtendedString("named_plate"))
    for face, name in named:
        sub = shapes.AddSubShape(label, face)
        TDataStd_Name.Set_s(sub, TCollection_ExtendedString(name))
    writer = STEPCAFControl_Writer()
    Interface_Static.SetCVal_s("write.step.schema", "AP242DIS")
    Interface_Static.SetIVal_s("write.stepcaf.subshapes.name", 1)
    writer.SetNameMode(True)
    assert writer.Transfer(doc, STEPControl_StepModelType.STEPControl_AsIs)
    assert writer.Write(str(path)) == IFSelect_ReturnStatus.IFSelect_RetDone
    path.with_suffix(path.suffix + ".gz").write_bytes(gzip.compress(path.read_bytes(), mtime=0))
    path.unlink()


if __name__ == "__main__":
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent
    build(8, False, out / "named_plate.old.step")
    build(11, True, out / "named_plate.new.step")
