"""A STEP specify-core wrote, opened again: its answers come back, its PMI is replaced."""

import base64
import hashlib
import json
import re

from specify_core.api import analyse, apply, write
from specify_core.load import load
from specify_core.resume import SCHEMA, fingerprint, fingerprint_of, saved


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


def test_answers_for_other_faces_are_not_resumed(pin, tmp_path):
    analysis = analyse(pin)
    answers = {"part.material": "brass"}
    out = tmp_path / "x.step"
    write(pin, apply(analysis, answers, accept_defaults=True), out, answers=answers)
    text = out.read_text()
    payload = saved(out)
    payload["faces"] = "0" * 64
    forged = base64.b64encode(json.dumps(payload).encode()).decode()
    stored = r"('pmi-assist answers',')[A-Za-z0-9+/=]*'"
    out.write_text(re.sub(stored, rf"\g<1>{forged}'", text, count=1))
    assert "resume" not in analyse(out)


def _restore(path, payload) -> None:
    """Replace the answers stored in ``path`` with ``payload``."""
    encoded = base64.b64encode(json.dumps(payload, separators=(",", ":")).encode()).decode()
    stored = r"('pmi-assist answers',')[A-Za-z0-9+/=]*'"
    path.write_text(re.sub(stored, rf"\g<1>{encoded}'", path.read_text(), count=1))


def test_the_fingerprint_does_not_depend_on_the_sign_of_zero():
    # Rounding first: -0.0004 is a zero too. The text hashed is json.dumps's, as
    # specify-core-rust hashes it (its docs/design.md, "The fingerprint").
    signed = [("plane", [-50.0, -0.0, round(-0.0004, 3)]), ("cylinder", [0.0, -30.0, 5.0])]
    unsigned = [("plane", [-50.0, 0.0, 0.0]), ("cylinder", [0.0, -30.0, 5.0])]
    assert SCHEMA == 2
    assert fingerprint_of(signed) == fingerprint_of(unsigned)
    assert fingerprint_of(unsigned) == fingerprint_of(unsigned, 1)
    assert fingerprint_of(signed, 1) != fingerprint_of(unsigned, 1)
    text = '[["plane", [-50.0, 0.0, 0.0]], ["cylinder", [0.0, -30.0, 5.0]]]'
    assert fingerprint_of(signed) == hashlib.sha256(text.encode()).hexdigest()


def test_answers_are_stored_with_the_schema_2_fingerprint(pin, tmp_path):
    answers = {"part.material": "brass"}
    out = tmp_path / "x.step"
    write(pin, apply(analyse(pin), answers, accept_defaults=True), out, answers=answers)
    payload = saved(out)
    assert payload["schema"] == 2 and payload["faces"] == fingerprint(load(out), 2)


def test_a_file_python_rewrote_resumes(spool, tmp_path):
    # Rewriting moves the signs of the spool's zero centroid components (OpenCascade's
    # integration noise), so its schema 1 fingerprint changed and it did not resume.
    answers = {"part.material": "brass"}
    out = tmp_path / "spool.step"
    write(spool, apply(analyse(spool), answers, accept_defaults=True), out, answers=answers)
    assert fingerprint(load(spool), 1) != fingerprint(load(out), 1)
    assert analyse(out)["resume"] == {"answers": answers}


def test_a_schema_1_file_resumes_when_its_own_fingerprint_matches(pin, tmp_path):
    answers = {"part.material": "brass"}
    out = tmp_path / "x.step"
    write(pin, apply(analyse(pin), answers, accept_defaults=True), out, answers=answers)
    payload = {**saved(out), "schema": 1, "faces": fingerprint(load(out), 1)}
    _restore(out, payload)
    assert analyse(out)["resume"] == {"answers": answers}

    # A schema 1 payload is checked against schema 1's fingerprint, not schema 2's.
    _restore(out, {**payload, "faces": "0" * 64})
    assert "resume" not in analyse(out)


def test_a_payload_of_another_schema_is_not_read(pin, tmp_path):
    answers = {"part.material": "brass"}
    out = tmp_path / "x.step"
    write(pin, apply(analyse(pin), answers, accept_defaults=True), out, answers=answers)
    _restore(out, {**saved(out), "schema": 3})
    assert saved(out) is None and "resume" not in analyse(out)
