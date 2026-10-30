import os
from pathlib import Path
import pytest
from specify_core.existing import read_existing
from specify_core.load import load
NIST = Path(os.environ.get("NIST_PMI", "NIST-PMI-STEP-Files"))


def test_nist_tolerances_read_as_drawn(tmp_path):
    """Every NIST CTC-03 tolerance reads 0 from OCCT; recovered, they are the
    file's LENGTH_MEASUREs, stated in inches, converted to millimetres."""
    path = NIST / "nist_ctc_03_asme1_ap242-e2.stp"
    if not path.is_file():
        pytest.skip(f"NIST PMI files not available at {NIST}")
    found = read_existing(load(path))
    values = sorted(round(e.value / 25.4, 4) for e in found if e.kind == "geometric_tolerance")
    assert values == [0.01, 0.01, 0.01, 0.02, 0.03, 0.03, 0.04, 0.05, 0.05, 0.06, 0.06, 0.08]


def test_nist_angle_reads_in_degrees():
    path = NIST / "nist_ctc_01_asme1_ap242-e1.stp"
    if not path.is_file():
        pytest.skip(f"NIST PMI files not available at {NIST}")
    (angle,) = [e for e in read_existing(load(path)) if e.type == "Location_Angular"]
    assert angle.value == pytest.approx(60.0)
