import os
from pathlib import Path

import pytest

from specify_core.existing import read_existing
from specify_core.load import load
from specify_core.magnitudes import tolerance_magnitudes

#: NIST's AP242 PMI test files, with authored semantic PMI (not in the corpus).
NIST = Path(os.environ.get("NIST_PMI", "NIST-PMI-STEP-Files"))

_STEP = """ISO-10303-21;
DATA;
#1=(LENGTH_MEASURE_WITH_UNIT()MEASURE_REPRESENTATION_ITEM()
MEASURE_WITH_UNIT(LENGTH_MEASURE(0.04),#20)REPRESENTATION_ITEM(''));
#2=ANGULARITY_TOLERANCE('Angularity.1','',#1,#9,(#8));
#3=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.05),#21);
#4=(GEOMETRIC_TOLERANCE('Position.1','',#3,#9)GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE((#8))
POSITION_TOLERANCE());
#5=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.0001),#22);
#6=FLATNESS_TOLERANCE('Flatness.1','',#5,#9);
#7=FLATNESS_TOLERANCE('Twice','',#3,#9);
#10=FLATNESS_TOLERANCE('Twice','',#5,#9);
#11=FLATNESS_TOLERANCE('Furlong','',#12,#9);
#12=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#23);
#20=(CONVERSION_BASED_UNIT('inch',#24)LENGTH_UNIT()NAMED_UNIT(#25));
#21=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );
#22=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.) );
#23=(CONVERSION_BASED_UNIT('furlong',#26)LENGTH_UNIT()NAMED_UNIT(#25));
#24=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#21);
ENDSEC;
END-ISO-10303-21;
"""


def test_magnitudes_are_read_from_the_text_in_millimetres(tmp_path):
    path = tmp_path / "t.step"
    path.write_text(_STEP)
    found = tolerance_magnitudes(path)
    assert found["Angularity.1"] == pytest.approx(1.016)  # 0.04 inch
    assert found["Position.1"] == pytest.approx(0.05)  # mm, simple entity form
    assert found["Flatness.1"] == pytest.approx(0.1)  # 0.0001 m
    assert "Twice" not in found  # one name, two tolerances: not guessed
    assert "Furlong" not in found  # unit that does not resolve to a length


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
