"""What a viewer that shows only presentation is given: leaders, and datum symbols."""

import re

import pytest

from specify_core.api import analyse, apply, write
from specify_core.lettering import line


def test_letters_are_strokes_scaled_to_their_height():
    strokes, width = line("AB", 2.0)
    assert strokes and all(len(s) > 1 for s in strokes)
    assert max(y for s in strokes for _, y in s) == pytest.approx(2.0)
    assert line("AB", 4.0)[1] == pytest.approx(2 * width)


def _drawn(text: str) -> dict[str, list[tuple[float, ...]]]:
    """Callout name -> the points it is drawn with."""
    entities = dict(re.findall(r"#(\d+)\s*=\s*(.*?);(?=\s*#\d+\s*=|\s*ENDSEC)", text))
    out = {}
    for name, occurrence in re.findall(r"DRAUGHTING_CALLOUT\('([^']+)',\(#(\d+)\)\)", text):
        geometry = re.findall(r"#(\d+)", entities[occurrence])[-1]
        curve = re.findall(r"#(\d+)", entities[geometry])[0]
        coordinates = re.findall(r"#(\d+)", entities[curve])[0]
        points = re.findall(r"\(([-\d.E+]+),([-\d.E+]+),([-\d.E+]+)\)", entities[coordinates])
        out[name] = [tuple(map(float, p)) for p in points]
    return out


@pytest.fixture(scope="module")
def pin_drawn(pin, tmp_path_factory):
    analysis = analyse(pin)
    knurl = next(
        f["id"]
        for f in analysis["features"]
        if f["family"] == "turned_steps" and f["record"]["diameter"] == 10.0
    )
    answers = {"part.material": "CW614N leaded brass", f"diameter.fit:{knurl}": "knurl:straight"}
    out = tmp_path_factory.mktemp("d") / "pin-drawn.step"
    write(pin, apply(analysis, answers, accept_defaults=True), out)
    return re.sub(r"\s*\n\s*", " ", out.read_text())


def test_a_note_is_its_leader_and_no_words(pin_drawn):
    drawn = _drawn(pin_drawn)
    notes = {n: p for n, p in drawn.items() if not n.startswith("Datum ")}
    assert notes and all(len(points) == 2 for points in notes.values())
    assert "Part notes" not in drawn


def test_a_datum_symbol_presents_its_datum_feature(pin_drawn):
    names = {i: n for i, n in re.findall(r"#(\d+)\s*=\s*DRAUGHTING_CALLOUT\('([^']+)'", pin_drawn)}
    symbols = [i for i, n in names.items() if n.startswith("Datum ")]
    assert symbols
    linked = dict(
        (callout, aspect)
        for aspect, callout in re.findall(
            r"DRAUGHTING_MODEL_ITEM_ASSOCIATION\('[^']*','',#(\d+),#\d+,#(\d+)\)", pin_drawn
        )
    )
    for callout in symbols:
        assert re.search(rf"#{linked[callout]}\s*=\s*DATUM_FEATURE\(", pin_drawn)
