"""An assembly of several parts, specified a part at a time and written together."""

import base64
import json
import re

import pytest

from specify_core import api
from specify_core.cli import main
from specify_core.existing import read_existing
from specify_core.load import load
from specify_core.requirements import _anchors, entities_of, face_entities
from specify_core.serve import reply
from specify_core.writer import _file_limits, _part_items, _part_tolerance_values

PLATE, PIN = 0, 1
ANSWERS = {PLATE: {"part.material": "steel"}, PIN: {"part.material": "brass"}}


def test_each_part_is_listed_once_with_where_its_instances_are(assembly):
    plate, pin = api.parts(assembly)
    assert (plate["name"], plate["faces"], plate["instances"]) == ("plate", 13, 1)
    assert (pin["name"], pin["faces"], pin["instances"]) == ("pin", 7, 2)
    assert [[row[3] for row in p] for p in pin["placements"]] == [[-30, 0, 5], [30, 0, 5]]
    assert not plate["written"] and not pin["written"]


def test_an_assembly_is_analysed_a_part_at_a_time(assembly):
    with pytest.raises(ValueError, match="an assembly of 2 parts; choose one"):
        api.analyse(assembly)
    pin = api.analyse(assembly, PIN)
    assert pin["binding"]["part"] == PIN and len(pin["faces"]) == 7
    defaults = {q.id: q.default for q in api.questions(pin, {})}
    assert "tapped:M5" in defaults.values()
    assert len(api.mesh(assembly, PIN)["faces"]) == 7


@pytest.fixture(scope="module")
def written(assembly, tmp_path_factory):
    analyses = {k: api.analyse(assembly, k) for k in (PLATE, PIN)}
    intents = {k: api.apply(analyses[k], ANSWERS[k], accept_defaults=True) for k in analyses}
    out = tmp_path_factory.mktemp("assembly") / "written.step"
    report = api.write_parts(assembly, [(intents[k], ANSWERS[k]) for k in intents], out)
    return intents, report, out


def test_both_parts_are_written_and_verified(written):
    _, report, _ = written
    assert report.not_written == []
    assert "plate: material steel" in report.written
    assert "pin: internal thread M5x0.8-6H on holes:3.5" in report.written


def test_each_part_has_its_own_pmi(written):
    intents, _, out = written
    text = out.read_text()
    entities = {int(i): body for i, body in entities_of(text)}
    for k, intent in intents.items():
        pd = _anchors(entities, face_entities(out, k).values())["pd"]
        assert re.search(
            rf"PROPERTY_DEFINITION\('manufacturing requirement','internal thread',#{pd}\)", text
        )
        back = read_existing(load(out, part=k))
        datums = {(e.type, e.faces) for e in back if e.kind == "datum"}
        assert datums == {(d["letter"], tuple(d["faces"])) for d in intent.datums}
    # Two parts, each with its own datum A, under its own letter in the file.
    assert sorted(re.findall(r"DATUM\('','',#\d+,\.F\.,'(\w+)'\)", text)) == [
        "A",
        "A",
        "B",
        "B",
        "C",
    ]


def test_each_part_resumes_its_own_answers(written):
    *_, out = written
    assert [p["written"] for p in api.parts(out)] == [True, True]
    for k in (PLATE, PIN):
        assert api.analyse(out, k)["resume"] == {"answers": ANSWERS[k]}


def test_writing_one_part_again_alone_is_refused(written, tmp_path):
    *_, out = written
    pin = api.analyse(out, PIN)
    intent = api.apply(pin, ANSWERS[PIN], accept_defaults=True)
    with pytest.raises(ValueError, match=r"part\(s\) 0 have answers stored"):
        api.write(out, intent, tmp_path / "x.step", answers=ANSWERS[PIN])
    with pytest.raises(ValueError, match="with the answers it came from"):
        api.write(out, intent, tmp_path / "x.step")


def test_written_again_through_the_service_its_pmi_is_replaced(written, tmp_path):
    *_, out = written
    again = tmp_path / "again.step"
    parts = [{"analysis": api.analyse(out, k), "answers": ANSWERS[k]} for k in (PLATE, PIN)]
    result = reply(
        {
            "id": 1,
            "op": "write",
            "step": str(out),
            "output": str(again),
            "accept_defaults": True,
            "parts": parts,
        }
    )
    assert result["ok"], result
    count = lambda p: len(re.findall(r"= ?DATUM\(", p.read_text()))  # noqa: E731
    assert count(again) == count(out) == 5
    assert reply({"id": 2, "op": "parts", "step": str(again)})["result"]["parts"][PIN]["written"]
    analysed = reply({"id": 3, "op": "analyse", "step": str(again), "part": PIN})["result"]
    assert analysed["analysis"]["resume"] == {"answers": ANSWERS[PIN]}
    assert len(analysed["mesh"]["faces"]) == 7


def test_an_assembly_with_pmi_of_its_own_is_refused(written, tmp_path):
    *_, out = written
    foreign = tmp_path / "foreign.step"
    # The same PMI, but no longer known to be specify-core's.
    foreign.write_text(out.read_text().replace("'pmi-assist answers'", "'other'"))
    pin = api.analyse(foreign, PIN)
    with pytest.raises(ValueError, match="already has PMI not written by specify-core"):
        api.write(
            foreign,
            api.apply(pin, ANSWERS[PIN], accept_defaults=True),
            tmp_path / "x.step",
            answers=ANSWERS[PIN],
        )


def test_answers_stored_for_a_part_the_file_no_longer_has_are_refused(written, tmp_path):
    *_, out = written

    def moved(found):
        payload = json.loads(base64.b64decode(found.group(1)))
        if payload["part"] == PIN:
            payload["part"] = 5
        return f"'pmi-assist answers','{base64.b64encode(json.dumps(payload).encode()).decode()}'"

    flat = re.sub(r"\s*\n\s*", "", out.read_text())
    changed = tmp_path / "changed.step"
    changed.write_text(re.sub(r"'pmi-assist answers','([A-Za-z0-9+/=]+)'", moved, flat))
    parts = [(api.analyse(changed, k), ANSWERS[k]) for k in (PLATE, PIN)]
    intents = [(api.apply(a, q, accept_defaults=True), q) for a, q in parts]
    with pytest.raises(ValueError, match=r"answers stored for part\(s\) 5 are not for"):
        api.write_parts(changed, intents, tmp_path / "x.step")


def test_each_part_is_verified_against_its_own_pmi(written):
    *_, out = written
    text = re.sub(r"\s*\n\s*", " ", out.read_text())
    entities = {int(i): body for i, body in entities_of(text)}
    plate, pin = (_anchors(entities, face_entities(out, k).values()) for k in (PLATE, PIN))
    # The plate's clearance holes have limits; the pin has none of its own.
    assert _file_limits(text, _part_tolerance_values(entities, plate["pds"]))
    assert _file_limits(text, _part_tolerance_values(entities, pin["pds"])) == []
    assert "'brass'" in _part_items(entities, pin["pd"])
    assert "'steel'" not in _part_items(entities, pin["pd"])


def test_an_unanswered_part_is_named(assembly, tmp_path):
    parts = [{"analysis": api.analyse(assembly, k), "answers": {}} for k in (PLATE, PIN)]
    parts[PLATE]["answers"] = ANSWERS[PLATE]
    result = reply(
        {
            "id": 1,
            "op": "write",
            "step": str(assembly),
            "output": str(tmp_path / "x.step"),
            "accept_defaults": True,
            "parts": parts,
        }
    )
    assert result["error"]["code"] == "incomplete" and result["error"]["part"] == PIN
    assert result["error"]["message"].startswith("part 1: ")


def test_an_analysis_from_before_parts_still_previews_with_anchors(pin):
    analysis = api.analyse(pin)
    analysis["binding"].pop("part")
    request = {"op": "preview", "analysis": analysis, "answers": {}, "step": str(pin)}
    assert reply({"id": 1, **request})["result"]["intent"]["anchors"]


def test_the_command_line_writes_a_pair_for_each_part(assembly, tmp_path, capsys):
    pairs = []
    for k in (PLATE, PIN):
        analysis, answers = tmp_path / f"a{k}.json", tmp_path / f"q{k}.json"
        assert main(["analyse", str(assembly), "--part", str(k), "-o", str(analysis)]) == 0
        answers.write_text(json.dumps(ANSWERS[k]))
        pairs += [str(analysis), str(answers)]
    capsys.readouterr()
    out = tmp_path / "cli.step"
    assert main(["write", str(assembly), *pairs, "-o", str(out), "--accept-defaults"]) == 0
    assert any(line.startswith("pin: ") for line in json.loads(capsys.readouterr().out)["written"])


def test_an_intent_made_before_parts_were_told_apart_still_writes(pin, tmp_path):
    intent = api.apply(api.analyse(pin), {"part.material": "brass"}, accept_defaults=True)
    intent.binding.pop("part")
    assert api.write(pin, intent, tmp_path / "old.step").not_written == []
