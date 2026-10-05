"""A triangle mesh of the part whose faces carry specify-core's face indices.

A picker shows the part and reports which face was clicked. It must report the
index the analysis, questions and intent use, so the mesh is made here, from the
same loaded shape and face map, rather than by a viewer re-reading the STEP file
and numbering faces its own way. The mesh carries the analysis binding, so a
mesh and an analysis of different files cannot be paired.

Triangles are stored face by face: face ``i`` owns triangles
``faces[i][0] .. faces[i][0] + faces[i][1] - 1``, so a hit triangle maps to its
face by a search over the starts. Vertices are not shared between faces, which
keeps each face's shading and highlight its own. A face that does not
triangulate has an empty range and is listed in ``missing``, so a picker can
say so rather than show a hole.

Coordinates are the solid's own, the frame the analysis describes; for a part
stored inside an assembly that is not necessarily where a CAD viewer places it.

A copy of the solid is meshed. OCCT keeps a triangulation on the faces it
meshes and reuses it later, and Quiddity tessellates the same faces during
analysis: meshing the loaded part in place made each depend on which ran first.
"""

from __future__ import annotations

from typing import Any

from OCP.Bnd import Bnd_Box
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Curve
from OCP.BRepBndLib import BRepBndLib
from OCP.BRepBuilderAPI import BRepBuilderAPI_Copy
from OCP.BRepMesh import BRepMesh_IncrementalMesh
from OCP.GCPnts import GCPnts_QuasiUniformDeflection
from OCP.TopAbs import TopAbs_EDGE, TopAbs_FACE, TopAbs_REVERSED
from OCP.TopExp import TopExp
from OCP.TopLoc import TopLoc_Location
from OCP.TopoDS import TopoDS
from OCP.TopTools import TopTools_IndexedDataMapOfShapeListOfShape, TopTools_IndexedMapOfShape

from .load import LoadedPart

# A wire format readers check for: its name stays as first published.
MESH_FORMAT = "pmi-assist-mesh"
MESH_VERSION = 2

#: Chordal deflection as a share of the part's diagonal: fine enough that small
#: holes are round, coarse enough that a 300-face part stays a few megabytes.
DEFLECTION = 0.001
ANGULAR_DEFLECTION = 0.3

#: Coordinates are written to this many decimals of the model unit (mm).
DECIMALS = 4


def mesh(loaded: LoadedPart, deflection: float = DEFLECTION) -> dict[str, Any]:
    """The part as JSON-safe triangles, face-ranged by specify-core face index."""
    # Measured on the exact geometry: a triangulation pads the box, so a second
    # call would otherwise see a larger part.
    box = Bnd_Box()
    BRepBndLib.Add_s(loaded.shape, box, False)
    lo, hi = box.CornerMin(), box.CornerMax()
    diagonal = lo.Distance(hi) or 1.0

    shape = BRepBuilderAPI_Copy(loaded.shape, True, False).Shape()
    faces = TopTools_IndexedMapOfShape()
    TopExp.MapShapes_s(shape, TopAbs_FACE, faces)
    if faces.Extent() != loaded.binding.face_count:
        raise RuntimeError("the copied solid numbers its faces differently")
    BRepMesh_IncrementalMesh(shape, diagonal * deflection, False, ANGULAR_DEFLECTION, True)

    positions: list[float] = []
    indices: list[int] = []
    ranges: list[list[int]] = []
    missing: list[int] = []
    for index in range(loaded.binding.face_count):
        face = TopoDS.Face_s(faces.FindKey(index + 1))
        location = TopLoc_Location()
        triangulation = _triangulation(face, location)
        start = len(indices) // 3
        if triangulation is None or triangulation.NbTriangles() == 0:
            ranges.append([start, 0])
            missing.append(index)
            continue
        transform = location.Transformation()
        base = len(positions) // 3
        for i in range(1, triangulation.NbNodes() + 1):
            positions.extend(_point(triangulation.Node(i).Transformed(transform)))
        # A reversed face's triangles wind against its outward side.
        flip = face.Orientation() == TopAbs_REVERSED
        for i in range(1, triangulation.NbTriangles() + 1):
            a, b, c = triangulation.Triangle(i).Get()
            if flip:
                b, c = c, b
            indices.extend((base + a - 1, base + b - 1, base + c - 1))
        ranges.append([start, len(indices) // 3 - start])

    return {
        "format": MESH_FORMAT,
        "version": MESH_VERSION,
        "binding": loaded.binding.to_dict(),
        "bbox": [_point(lo), _point(hi)],
        "positions": positions,
        "indices": indices,
        "faces": ranges,
        "missing": missing,
        "edges": _edges(shape, diagonal * deflection),
    }


def _triangulation(face: Any, location: Any) -> Any:
    return BRep_Tool.Triangulation_s(face, location)


def _point(p: Any) -> tuple[float, float, float]:
    return (round(p.X(), DECIMALS), round(p.Y(), DECIMALS), round(p.Z(), DECIMALS))


def _edges(shape: Any, deflection: float) -> list[list[float]]:
    """The boundaries between faces as flat polylines. A seam -- where a
    cylinder closes on itself -- is no boundary, and drawing it would make a
    bore look like two faces."""
    owners = TopTools_IndexedDataMapOfShapeListOfShape()
    TopExp.MapShapesAndAncestors_s(shape, TopAbs_EDGE, TopAbs_FACE, owners)
    out: list[list[float]] = []
    for i in range(1, owners.Extent() + 1):
        edge = TopoDS.Edge_s(owners.FindKey(i))
        if BRep_Tool.Degenerated_s(edge):
            continue
        if any(BRep_Tool.IsClosed_s(edge, TopoDS.Face_s(f)) for f in owners.FindFromIndex(i)):
            continue
        sampler = GCPnts_QuasiUniformDeflection(BRepAdaptor_Curve(edge), deflection)
        if not sampler.IsDone() or sampler.NbPoints() < 2:
            continue
        line: list[float] = []
        for j in range(1, sampler.NbPoints() + 1):
            line.extend(_point(sampler.Value(j)))
        out.append(line)
    return out
