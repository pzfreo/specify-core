import subprocess
import sys
import pytest
from specify_core.load import load
_SIGNATURE = """
import sys, hashlib
# Ported: each face's area and centroid from the analysis, not OpenCascade's.
from specify_core.api import analyse
out = [f"{f['id']}:{f['area']:.6f}:{f['centroid']}" for f in analyse(sys.argv[1])["faces"]]
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
    # Ported: the faces numbered are the analysis's, not OpenCascade's face map.
    from specify_core.api import analyse

    assert a.binding.face_count == len(analyse(a)["faces"])


def test_an_assembly_holding_one_solid_is_numbered_on_the_solid(wrapped):
    """PMI attaches to the solid's label, so its faces are the ones numbered."""
    loaded = load(wrapped)
    assert loaded.binding.face_count == 7
    # Ported: the analysis numbers the solid's seven faces (not OpenCascade's shape type).
    from specify_core.api import analyse

    assert len(analyse(loaded)["faces"]) == 7


def test_nist_ctc_03_loads_through_its_assembly(ctc03):
    assert load(ctc03).binding.face_count == 139


def test_a_file_with_two_parts_must_say_which(tmp_path):
    # The file specify-core builds here with build123d, committed by tools/make_parts.py.
    from conftest import PARTS

    path = PARTS / "a_file_with_two_parts_must_say_which.step"
    with pytest.raises(ValueError, match="an assembly of 2 parts; choose one"):
        load(path)
    assert load(path, part=1).binding.part == 1
    with pytest.raises(ValueError, match="there is no part 2"):
        load(path, part=2)
