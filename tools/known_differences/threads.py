"""Independently check measured thread hypotheses against OpenCascade's STEP surfaces.

Run with ~/cld/quiddity/.venv/bin/python -I tools/known_differences/threads.py
INPUT.step[.gz] ANALYSIS.json. Generate ANALYSIS with Specify's analyse command.
No quiddity recogniser is imported. Output is evidence for review, not a fixture.

For a screw surface, its normal is perpendicular to the screw velocity
axis x radial + (lead / 2pi) axis. OCP derivatives therefore independently give
the lead. Check trimmed flank samples, crest/root radii, material side and the
axial bounds of the flanks and runouts (carrier faces can extend beyond the
thread). Boundary helices give
the global lead; local differential leads are also reported because spline
approximations need not preserve their derivatives. Sampling is evidence,
not a certified continuous bound or recovery of a nominal thread designation.
"""

import gzip
import json
import math
import sys
import tempfile
from pathlib import Path

from OCP.BRepAdaptor import BRepAdaptor_Curve, BRepAdaptor_Surface
from OCP.BRepClass import BRepClass_FaceClassifier
from OCP.STEPControl import STEPControl_Reader
from OCP.TopAbs import TopAbs_EDGE, TopAbs_FACE, TopAbs_IN, TopAbs_REVERSED
from OCP.TopExp import TopExp
from OCP.TopTools import TopTools_IndexedMapOfShape
from OCP.TopoDS import TopoDS
from OCP.gp import gp_Pnt, gp_Pnt2d, gp_Vec


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def cross(a, b):
    return (a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0])


def sub(a, b):
    return tuple(x-y for x, y in zip(a, b))


def xyz(p):
    return (p.X(), p.Y(), p.Z())


def check(path, analysis):
    reader = STEPControl_Reader()
    assert int(reader.ReadFile(str(path))) == 1
    reader.TransferRoots()
    faces = TopTools_IndexedMapOfShape()
    TopExp.MapShapes_s(reader.OneShape(), TopAbs_FACE, faces)
    assert faces.Extent() == analysis["binding"]["face_count"]
    results = []
    for feature in analysis["features"]:
        if feature["family"] != "modelled_threads":
            continue
        t = feature["record"]
        origin, axis = t["axis"]["point"], t["axis"]["direction"]
        role = {f: name for name in ("flank", "crest", "root", "runout") for f in t[name+"_faces"]}
        assert sorted(role) == feature["faces"]
        lead = t["pitch"] * t["starts"] * (1 if t["hand"] == "right" else -1)
        measured_leads, radii, heights, checked_flanks = [], {"crest": [], "root": []}, [], []
        side_signs, boundary_leads, boundary_residuals = [], [], []
        for index, name in role.items():
            face = TopoDS.Face_s(faces.FindKey(index+1))
            surface = BRepAdaptor_Surface(face)
            u0, u1 = surface.FirstUParameter(), surface.LastUParameter()
            v0, v1 = surface.FirstVParameter(), surface.LastVParameter()
            samples = []
            for i in range(1, 14):
                for j in range(1, 14):
                    u, v = u0+(u1-u0)*i/14, v0+(v1-v0)*j/14
                    if BRepClass_FaceClassifier(face, gp_Pnt2d(u, v), 1e-8).State() != TopAbs_IN:
                        continue
                    p, du, dv = gp_Pnt(), gp_Vec(), gp_Vec()
                    surface.D1(u, v, p, du, dv)
                    n = cross(xyz(du), xyz(dv))
                    norm = math.sqrt(dot(n, n))
                    if norm == 0:
                        continue
                    n = tuple(x/norm for x in n)
                    if face.Orientation() == TopAbs_REVERSED:
                        n = tuple(-x for x in n)
                    relative = sub(xyz(p), origin)
                    z = dot(relative, axis)
                    radial = sub(relative, tuple(z*x for x in axis))
                    r = math.sqrt(dot(radial, radial))
                    samples.append((r, z))
                    if name == "flank" and abs(dot(n, axis)) > 0.1:
                        measured_leads.append(-2*math.pi*dot(n, cross(axis, radial))/dot(n, axis))
                    if name in radii:
                        side_signs.append(dot(n, radial) * (1 if t["side"] == "external" else -1))
            if name == "flank":
                assert samples, (index, "no trimmed flank samples")
                checked_flanks.append(index)
            edges = TopTools_IndexedMapOfShape()
            TopExp.MapShapes_s(face, TopAbs_EDGE, edges)
            for k in range(1, edges.Extent()+1):
                curve = BRepAdaptor_Curve(TopoDS.Edge_s(edges.FindKey(k)))
                a, b = curve.FirstParameter(), curve.LastParameter()
                edge_samples = []
                for j in range(257):
                    relative = sub(xyz(curve.Value(a+(b-a)*j/256)), origin)
                    z = dot(relative, axis)
                    radial = sub(relative, tuple(z*x for x in axis))
                    r = math.sqrt(dot(radial, radial))
                    samples.append((r, z))
                    # A right-handed orthonormal polar frame about the supplied axis.
                    seed = (1., 0., 0.) if abs(axis[0]) < 0.9 else (0., 1., 0.)
                    x = cross(axis, seed)
                    length = math.sqrt(dot(x, x))
                    x = tuple(v/length for v in x)
                    y = cross(axis, x)
                    angle = math.atan2(dot(radial, y), dot(radial, x))
                    if edge_samples:
                        angle += 2*math.pi*round((edge_samples[-1][1]-angle)/(2*math.pi))
                    edge_samples.append((r, angle, z))
                rs, angles, zs = zip(*edge_samples)
                if (name == "flank" and max(rs)-min(rs) < t["tolerances"]["diameter"]/2
                        and abs(angles[-1]-angles[0]) > math.pi/2
                        and abs(zs[-1]-zs[0]) > t["pitch"]/2):
                    am, zm = sum(angles)/len(angles), sum(zs)/len(zs)
                    slope = sum((a-am)*(z-zm) for a, z in zip(angles, zs))/sum((a-am)**2 for a in angles)
                    boundary_leads.append(2*math.pi*slope)
                    boundary_residuals.append(max(abs(z-zm-slope*(a-am)) for a, z in zip(angles, zs)))
            if name in ("flank", "runout"):
                heights.extend(z for _, z in samples)
            if name in radii:
                radii[name].extend(r for r, _ in samples)
        assert checked_flanks == t["flank_faces"]
        assert measured_leads and boundary_leads
        error = max(abs(x-lead) for x in boundary_leads)
        assert error <= t["tolerances"]["pitch"]*t["starts"], (feature["id"], "boundary lead", error)
        assert max(boundary_residuals) <= t["tolerances"]["axial"]
        for name, diameter in [("crest", t["major_diameter"] if t["side"] == "external" else t["minor_diameter"]),
                               ("root", t["minor_diameter"] if t["side"] == "external" else t["major_diameter"])]:
            assert max(abs(2*r-diameter) for r in radii[name]) <= t["tolerances"]["diameter"], (name, feature["id"])
        assert abs(min(heights)-t["start"]) <= t["tolerances"]["axial"], ("start", min(heights), t["start"])
        assert abs(max(heights)-t["end"]) <= t["tolerances"]["axial"], ("end", max(heights), t["end"])
        results.append({"id": feature["id"], "flanks_checked": checked_flanks,
                        "lead_samples": len(measured_leads), "lead_range": [min(measured_leads), max(measured_leads)],
                        "boundary_leads": boundary_leads, "maximum_boundary_lead_error": error,
                        "maximum_boundary_axial_residual": max(boundary_residuals),
                        "signed_material_normal_range": [min(side_signs), max(side_signs)], "crest_radius_range": [min(radii["crest"]), max(radii["crest"])],
                        "root_radius_range": [min(radii["root"]), max(radii["root"])],
                        "axial_range": [min(heights), max(heights)]})
    assert results
    return results


if __name__ == "__main__":
    source, analysis_path = map(Path, sys.argv[1:])
    analysis = json.loads(analysis_path.read_text())
    with tempfile.TemporaryDirectory() as tmp:
        if source.suffix == ".gz":
            plain = Path(tmp)/source.stem
            plain.write_bytes(gzip.decompress(source.read_bytes()))
            source = plain
        print(json.dumps(check(source, analysis), indent=2))
