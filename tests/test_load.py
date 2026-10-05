import subprocess
import sys

import pytest

from specify_core.load import load

_SIGNATURE = """
import sys, hashlib
from OCP.BRepGProp import BRepGProp
from OCP.GProp import GProp_GProps
from specify_core.load import load
lp = load(sys.argv[1]); out = []
for i in range(lp.binding.face_count):
    p = GProp_GProps(); BRepGProp.SurfaceProperties_s(lp.face(i), p); c = p.CentreOfMass()
    out.append(f"{i}:{p.Mass():.6f}:{c.X():.5f},{c.Y():.5f},{c.Z():.5f}")
print(hashlib.sha256("|".join(out).encode()).hexdigest())
"""


def test_face_indices_are_stable_across_processes(spool):
    """Face indices are the durable key for intent, so the same bytes must always
    number faces the same way, in fresh processes."""
    runs = {
        subprocess.run(
            [sys.executable, "-c", _SIGNATURE, str(spool)],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        for _ in range(3)
    }
    assert len(runs) == 1


def test_binding_identifies_the_file(plate, spool):
    a, b = load(plate), load(spool)
    assert a.binding.source_sha256 != b.binding.source_sha256
    assert a.binding.face_count == a.faces.Extent()


def test_index_of_round_trips(plate):
    loaded = load(plate)
    for i in range(loaded.binding.face_count):
        assert loaded.index_of(loaded.face(i)) == i


def test_an_assembly_holding_one_solid_is_numbered_on_the_solid(wrapped):
    """PMI attaches to the solid's label, so its faces are the ones numbered."""
    loaded = load(wrapped)
    assert loaded.binding.face_count == 7
    assert loaded.shape.ShapeType().name == "TopAbs_SOLID"
    assert all(loaded.index_of(loaded.face(i)) == i for i in range(7))


def test_nist_ctc_03_loads_through_its_assembly(ctc03):
    assert load(ctc03).binding.face_count == 139


def test_a_file_with_two_parts_is_refused(tmp_path):
    from build123d import Box, Compound, Location, export_step

    a, b = Box(1, 1, 1), Box(1, 1, 1).moved(Location((5, 0, 0)))
    path = tmp_path / "two.step"
    export_step(Compound(children=[a, b]), str(path))
    with pytest.raises(ValueError, match="expected one part"):
        load(path)
