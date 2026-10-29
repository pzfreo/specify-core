# Two revisions of a two-part assembly, for the assembly diff: a 20 × 10 × 2 plate with a Ø3 and
# a Ø1.6 hole at one end, and a block. The new plate is the old one turned end for end in its own
# frame (its holes at X 16, not X 4), placed as before; the block is unchanged. Read as a change:
# the plate moved. Run with build123d (0.10.0):
#   python plate_turned.py OUTDIR
# then gzip -9 OUTDIR/*.step.
import sys
from build123d import Box, Compound, Cylinder, Part, Pos, export_step

def plate(x):
    p = Part() + (Pos(10, 5, 1) * Box(20, 10, 2) - Pos(x, 5, 1) * Cylinder(1.5, 2)
                  - Pos(x, 2, 1) * Cylinder(0.8, 2))
    p.label = "plate"
    return p

def block():
    b = Part() + Pos(0, 30, 5) * Box(6, 6, 10)
    b.label = "block"
    return b

out = sys.argv[1]
for name, x in (("old", 4), ("new", 16)):
    c = Compound(children=[plate(x), block()])
    c.label = "plate_turned"
    export_step(c, f"{out}/plate_turned.{name}.step")
