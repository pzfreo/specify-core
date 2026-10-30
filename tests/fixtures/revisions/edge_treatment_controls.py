"""For #67: real changes alongside a new chamfer (build123d 0.10.0).
Run: python edge_treatment_controls.py OUTDIR. All sizes are mm.
"""
import gzip
from pathlib import Path
import sys
from build123d import Axis, Box, Cylinder, GeomType, Part, Pos, export_step


def bevel_top(part):
    top = part.faces().sort_by(Axis.Z)[-1]
    return part.chamfer(1.5, None, top.edges().filter_by(GeomType.CIRCLE))


def flat(z, bevel):
    # An axial shift within the SAME support plane, not a centroid shift from trimming.
    body = Part() + Pos(0, 0, 20) * Cylinder(8, 40)
    if bevel:
        body = bevel_top(body)
    body -= Pos(0, -11, z) * Box(30, 10, 10)
    # Three distinct, fixed bores anchor correspondence independently of the moving flat.
    for x, y, r in ((-3, 0, 1), (0, 2, 0.8), (3, 0, 0.6)):
        body -= Pos(x, y, 20) * Cylinder(r, 40)
    return body


def bracket(wall, bevel):
    body = Part() + Pos(0, 0, 5) * Box(100, 50, 10)
    body += Pos(25, 0, 16) * Cylinder(8, 12)
    if wall:
        body += Pos(-45, 0, 25) * Box(10, 50, 50)
    if bevel:
        body = bevel_top(body)
    return body


out = Path(sys.argv[1])
for name, old, new in (
    ("flat_translated", flat(20, False), flat(23, True)),
    ("plate_removed", bracket(True, False), bracket(False, True)),
):
    for which, part in (("old", old), ("new", new)):
        path = out / f"{name}.{which}.step"
        export_step(part, str(path))
        path.with_suffix(".step.gz").write_bytes(gzip.compress(path.read_bytes(), mtime=0))
        path.unlink()
