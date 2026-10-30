"""A STEP specify-core wrote, opened again: its answers come back, its PMI is replaced."""
import re
from specify_core.api import analyse, apply, write


def saved(path, part=0):
    # Ported: specify-core's resume.saved, as far as the API says: None when no answers are
    # stored for the part (parts' "written").
    from specify_core.api import parts

    return True if parts(path)[part]["written"] else None


def _counts(text: str) -> dict[str, int]:
    flat = re.sub(r"\s*\n\s*", " ", text)
    return {
        "requirements": len(re.findall(r"PROPERTY_DEFINITION\('manufacturing requirement'", flat)),
        "datums": len(re.findall(r"= ?DATUM\(", flat)),
        "tolerances": len(re.findall(r"POSITION_TOLERANCE", flat)),
        "saved": len(re.findall(r"'pmi-assist answers'", flat)) // 3,
    }


def test_a_written_file_resumes_its_answers_and_is_rewritten_not_added_to(pin, tmp_path):
    analysis = analyse(pin)
    knurl = next(
        f["id"]
        for f in analysis["features"]
        if f["family"] == "turned_steps" and f["record"]["diameter"] == 10.0
    )
    answers = {"part.material": "brass", f"diameter.fit:{knurl}": "knurl:straight"}
    first = tmp_path / "first.step"
    write(pin, apply(analysis, answers, accept_defaults=True), first, answers=answers)

    # Opened again: the answers, and nothing of the file's own PMI to keep.
    again = analyse(first)
    assert again["resume"] == {"answers": answers}
    assert again["existing"] == [] and again["part"] == {}

    # Changed and written again: the PMI is the new answers', once.
    changed = {**answers, "part.material": "steel"}
    second = tmp_path / "second.step"
    write(first, apply(again, changed, accept_defaults=True), second, answers=changed)
    before, after = _counts(first.read_text()), _counts(second.read_text())
    assert after == before and after["saved"] == 1
    assert "'steel'" in second.read_text() and "'brass'" not in second.read_text()
    assert analyse(second)["resume"] == {"answers": changed}


def test_without_answers_nothing_is_stored(pin, tmp_path):
    analysis = analyse(pin)
    out = tmp_path / "plain.step"
    write(pin, apply(analysis, {"part.material": "brass"}, accept_defaults=True), out)
    assert saved(out) is None
    assert "resume" not in analyse(out)
