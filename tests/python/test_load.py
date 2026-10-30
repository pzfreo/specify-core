import subprocess
import sys
import pytest
from specify_core.load import load


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
