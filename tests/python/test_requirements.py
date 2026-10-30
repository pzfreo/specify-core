"""Threads, knurls, material and general tolerance written into the STEP text."""
import re
import pytest
from specify_core.api import analyse, apply, write
from specify_core.load import load
from specify_core.writer import VerificationError


@pytest.fixture(scope="module")
def pin_written(pin, tmp_path_factory):
    analysis = analyse(pin)

    def feature(family, d):
        return next(
            f["id"]
            for f in analysis["features"]
            if f["family"] == family and f["record"]["diameter"] == d
        )

    answers = {
        "part.material": "CW614N leaded brass",
        f"diameter.fit:{feature('turned_steps', 6.0)}": "thread:M6",
        f"diameter.fit:{feature('turned_steps', 10.0)}": "knurl:straight",
    }
    out = tmp_path_factory.mktemp("w") / "pin-pmi.step"
    report = write(pin, apply(analysis, answers, accept_defaults=True), out)
    return analysis, report, out, re.sub(r"\s*\n\s*", " ", out.read_text())


def test_everything_is_written_and_verified(pin_written):
    _, report, _, _ = pin_written
    assert report.not_written == []
    for wanted in (
        "internal thread M5x0.8-6H",
        "external thread M6x1-6g",
        "straight knurl",
        "material CW614N leaded brass",
        "general tolerance ISO 2768-m",
    ):
        assert any(w.startswith(wanted) for w in report.written), wanted


def test_the_text_route_and_its_callouts(pin_written):
    *_, text = pin_written
    kinds = re.findall(r"PROPERTY_DEFINITION\('manufacturing requirement','([^']+)'", text)
    assert sorted(kinds) == [
        "edge condition",
        "external thread",
        "general tolerances",
        "internal thread",
        "knurl",
        "surface texture",
    ]
    callouts = re.findall(r"DRAUGHTING_CALLOUT\('([^']+)'", text)
    notes = sorted(c for c in callouts if not c.startswith("Datum "))
    assert notes == [
        "External thread requirement",
        "Internal thread requirement",
        "Knurl requirement",
    ]
    # Each callout presents what it is on: a requirement's faces, or a datum.
    assert len(re.findall(r"DRAUGHTING_MODEL_ITEM_ASSOCIATION\(", text)) == len(callouts)


def test_the_standard_constructs(pin_written):
    *_, text = pin_written
    assert len(re.findall(r"GENERAL_PROPERTY\('','user defined attribute',\$\)", text)) == 3
    assert "DESCRIPTIVE_REPRESENTATION_ITEM('fit class','6H')" in text
    assert re.search(
        r"MEASURE_REPRESENTATION_ITEM\('minimum full thread',LENGTH_MEASURE\(5\.5\)", text
    )
    assert "DESCRIPTIVE_REPRESENTATION_ITEM('tolerance class','ISO 2768-m')" in text
    assert "PROPERTY_DEFINITION('material property','material name'" in text
    assert "DEFAULT_TOLERANCE_TABLE" not in text


def test_its_callouts_read_back_as_notes_not_as_sizes_of_nothing(pin_written):
    *_, out, _ = pin_written
    existing = analyse(out)["existing"]
    assert not any(e["type"] == "CommonLabel" for e in existing)
    kinds = {e["type"] for e in existing if e["kind"] == "note"}
    assert {"internal thread", "external thread", "knurl"} <= kinds


def test_a_finish_on_faces_is_not_a_second_default(pin, tmp_path):
    analysis = analyse(pin)
    top = max(
        (f for f in analysis["faces"] if f["kind"] == "plane"), key=lambda f: f["centroid"][2]
    )
    answers = {
        "part.material": "brass",
        "extra.0": {"faces": [top["id"]], "kind": "finish", "value": "Ra 0.8"},
    }
    out = tmp_path / "finishes.step"
    write(pin, apply(analysis, answers, accept_defaults=True), out)
    text = out.read_text()
    kinds = re.findall(r"PROPERTY_DEFINITION\('manufacturing requirement','([^']+)'", text)
    assert kinds.count("surface texture") == 1 and "surface finish" in kinds
    # Read back: the default is the part's finish; the one on faces is a note.
    again = analyse(out)
    assert again["part"]["surface_finish"] == "Ra 3.2"
    assert any(e["kind"] == "note" and e["type"] == "surface finish" for e in again["existing"])


def test_a_finish_written_in_the_old_form_still_reads_back(pin_written, tmp_path):
    *_, out, _ = pin_written
    old = tmp_path / "old.step"
    old.write_text(
        out.read_text().replace(
            "'surface texture','Ra 3.2 \\X2\\00B5\\X0\\m unless otherwise specified'",
            "'general surface texture','Ra 3.2 \\X2\\00B5\\X0\\m unless otherwise stated'",
        )
    )
    assert "unless otherwise stated" in old.read_text()
    assert analyse(old)["part"]["surface_finish"] == "Ra 3.2"
