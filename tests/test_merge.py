"""Adding PMI to a part that already has some (merge.py).

The plate is enriched once, then enriched again: the second pass must ask only
for what the first left unstated, cite the first pass's datums, and leave the
first pass's file intact, byte for byte, around what it adds.
"""

import os
import re
from pathlib import Path

import pytest

from specify_core.api import analyse, apply, write
from specify_core.existing import datum_faces, read_existing
from specify_core.load import load
from specify_core.merge import carries_pmi
from specify_core.rules import questions

#: NIST's PMI test parts (AP242 with semantic PMI), which are not redistributed here.
#: NIST's PMI test files (Release/NIST-PMI-STEP-Files.zip in usnistgov/SFA), unzipped.
NIST_PMI = Path(os.environ.get("NIST_PMI", "NIST-PMI-STEP-Files"))


def _function(qs, diameter: str) -> str:
    return next(q.id for q in qs if q.id.startswith("hole.function") and diameter in q.prompt)


@pytest.fixture(scope="module")
def enriched(plate, tmp_path_factory):
    """The plate with datums A, B, C and a fitted, positioned Ø8 bore."""
    analysis = analyse(plate)
    qs = questions(analysis, {})
    answers = {
        "part.material": "steel",
        _function(qs, "Ø8"): "fit:H7",
        _function(qs, "6×"): "general",
    }
    out = tmp_path_factory.mktemp("merge") / "first.step"
    write(plate, apply(analysis, answers, accept_defaults=True), out)
    return out


@pytest.fixture(scope="module")
def merged(enriched, tmp_path_factory):
    analysis = analyse(enriched)
    qs = questions(analysis, {})
    answers = {_function(qs, "6×"): "clearance:M6"}
    out = tmp_path_factory.mktemp("merge") / "second.step"
    report = write(enriched, apply(analysis, answers, accept_defaults=True), out)
    return qs, report, out


def test_only_a_part_with_pmi_is_merged(plate, enriched):
    assert not carries_pmi(plate)
    assert carries_pmi(enriched)


def test_what_the_file_states_is_not_asked_again(merged):
    qs, report, _ = merged
    ids = [q.id for q in qs]
    assert not {"datum.A", "datum.B", "datum.C"} & set(ids)
    # What the first pass wrote is read back; "no coating" and "no heat
    # treatment" write nothing, so they are offered again at their defaults.
    stated = {"part.material", "part.general_tolerance", "part.surface_finish", "part.edges"}
    assert not stated & set(ids)
    assert {q.id for q in qs if q.id.startswith("part.")} <= {"part.coating", "part.heat_treatment"}
    assert not any(w.startswith(("material", "general tolerance")) for w in report.written)
    assert not any("Ø8" in q.prompt for q in qs)
    assert any("6×" in q.prompt for q in qs)


def test_the_new_pmi_cites_the_files_datums(merged):
    _, report, out = merged
    assert re.search(r"position [\d.]+ MMC on hole_patterns", " ".join(report.written))
    assert not any(w.startswith("datum ") for w in report.written)
    letters = re.findall(r"DATUM\('[^']*',[^;]*'(\w)'\);", out.read_text())
    assert sorted(letters) == ["A", "B", "C"]


def test_the_original_file_is_kept_byte_for_byte(enriched, merged):
    _, _, out = merged
    before, after = enriched.read_text(), out.read_text()
    end = before.rfind("ENDSEC;")
    assert after.startswith(before[:end]) and after.endswith(before[end:])
    old = [e.to_dict() for e in read_existing(load(enriched))]
    new = [e.to_dict() for e in read_existing(load(out))]
    assert all(e in new for e in old) and len(new) > len(old)


@pytest.fixture(scope="module")
def ctc01():
    path = NIST_PMI / "nist_ctc_01_asme1_ap242-e1.stp"
    if not path.is_file():
        pytest.skip(f"NIST PMI files not available at {NIST_PMI}")
    return path


def test_datums_on_a_set_of_faces_are_read_from_the_text(ctc01):
    """OCCT reads CTC-01's datums B and C, each a hole's two half-cylinders, with no faces."""
    assert datum_faces(ctc01) == {"A": (97,), "B": (89, 90), "C": (75, 76)}
    datums = {e.type: e.faces for e in read_existing(load(ctc01)) if e.kind == "datum"}
    assert datums == {"A": (97,), "B": (89, 90), "C": (75, 76)}


def test_a_nist_part_keeps_all_its_pmi(ctc01, tmp_path):
    """Written back through OCCT, its tolerances would read 0 and its 60° angle 3437.7°."""
    analysis = analyse(ctc01)
    asked = {q.id.split(":")[0] for q in questions(analysis, {})}
    controls = {"datum.B.control", "datum.C.control"}
    notes = {"part.surface_finish", "part.coating", "part.heat_treatment", "part.edges"}
    assert asked <= {"part.material", "part.general_tolerance", *notes, *controls}
    out = tmp_path / "ctc01.step"
    write(ctc01, apply(analysis, {"part.material": "CZ121"}, accept_defaults=True), out)
    old = [e.to_dict() for e in read_existing(load(ctc01))]
    new = [e.to_dict() for e in read_existing(load(out))]
    assert old == new


def test_a_files_own_datums_are_not_drawn_again(ctc01, tmp_path):
    out = tmp_path / "ctc01.step"
    write(ctc01, apply(analyse(ctc01), {"part.material": "CZ121"}, accept_defaults=True), out)
    assert "DRAUGHTING_CALLOUT('Datum " not in out.read_text()


def test_tolerances_written_simply_survive_the_merge(enriched, tmp_path):
    # Written as simple entities, they must still be taken across.
    analysis = analyse(enriched)
    faces = analysis["faces"]
    bore = next(f for f in faces if f["kind"] == "cylinder" and abs(f["radius"] - 4) < 1e-6)
    top = max((f for f in faces if f["kind"] == "plane"), key=lambda f: f["centroid"][2])
    kinds = {
        "runout": bore,
        "total_runout": bore,
        "parallelism": top,
    }
    answers = {
        f"extra.{n}": {"faces": [face["id"]], "kind": kind, "value": "0.02", "datums": ["A"]}
        for n, (kind, face) in enumerate(kinds.items())
    }
    out = tmp_path / "simple.step"
    write(enriched, apply(analysis, answers, accept_defaults=True), out)
    read = {e.type for e in read_existing(load(out))}
    assert {"CircularRunout", "TotalRunout", "Parallelism"} <= read


def test_a_file_with_pmi_from_elsewhere_stores_no_answers(ctc01, tmp_path):
    analysis = analyse(ctc01)
    answers = {"part.material": "CZ121"}
    out = tmp_path / "ctc01.step"
    write(ctc01, apply(analysis, answers, accept_defaults=True), out, answers=answers)
    from specify_core.resume import saved

    assert saved(out) is None
