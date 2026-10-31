"""PMI correspondence control: retain the seam between two fused half plates.

build123d 0.11.1; identical geometry, planar faces split into two.
The unequal bores disambiguate the part alignment. PMI is constructed by the test.
"""
from pathlib import Path
import gzip
import tempfile
from build123d import Box, Cylinder, Pos, SkipClean, export_step

root = Path(__file__).parent
old = Box(100, 60, 10)
with SkipClean():
    new = (Pos(-25, 0, 0) * Box(50, 60, 10)).fuse(
        Pos(25, 0, 0) * Box(50, 60, 10)
    )
    for x, y, radius in [(-30, -15, 3), (20, 10, 5)]:
        tool = Pos(x, y, 0) * Cylinder(radius, 20)
        old = old.cut(tool)
        new = new.cut(tool)
for revision, shape in [("old", old), ("new", new)]:
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "plate.step"
        export_step(shape, path)
        (root / f"pmi_split.{revision}.step.gz").write_bytes(
            gzip.compress(path.read_bytes(), mtime=0)
        )
