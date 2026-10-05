"""The child-process protocol a host such as a web app's server speaks."""

import json
import subprocess
import sys

import pytest

from specify_core.serve import reply

SERVE = "from specify_core.cli import main; raise SystemExit(main(['serve']))"


@pytest.fixture(scope="module")
def child():
    process = subprocess.Popen(
        [sys.executable, "-c", SERVE],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    def ask(request):
        process.stdin.write(json.dumps(request) + "\n")
        process.stdin.flush()
        return json.loads(process.stdout.readline())  # every stdout line is a reply

    yield ask
    process.stdin.close()
    assert process.wait(timeout=30) == 0


def test_a_session_over_the_pipe(child, plate, tmp_path):
    assert child({"id": 1, "op": "hello"}) == {"id": 1, "ok": True, "result": {"protocol": 1}}
    analysed = child({"id": 2, "op": "analyse", "step": str(plate)})
    assert analysed["ok"], analysed
    analysis = analysed["result"]["analysis"]
    assert analysed["result"]["mesh"]["binding"] == analysis["binding"]

    questions = child({"id": 3, "op": "questions", "analysis": analysis, "answers": {}})
    assert "part.material" in questions["result"]["open"]

    answers = {"part.material": "6082-T6"}
    preview = child({"id": 4, "op": "preview", "analysis": analysis, "answers": answers})
    assert preview["result"]["intent"]["part"]["material"] == "6082-T6"

    out = tmp_path / "plate-pmi.step"
    written = child(
        {
            "id": 5,
            "op": "write",
            "step": str(plate),
            "analysis": analysis,
            "answers": answers,
            "output": str(out),
            "accept_defaults": True,
        }
    )
    assert written["ok"], written
    assert "material 6082-T6" in written["result"]["report"]["written"]
    assert out.read_text().startswith("ISO-10303-21")


def test_errors_are_structured_and_the_child_survives(child):
    bad = child(
        {
            "id": 6,
            "op": "questions",
            "analysis": {"faces": [], "features": []},
            "answers": {"datum.A": [5]},
        }
    )
    assert bad["ok"] is False and bad["error"]["code"] == "invalid"
    assert child({"id": 7, "op": "hello"})["ok"]


def test_unanswered_questions_are_listed():
    out = reply(
        {
            "id": 8,
            "op": "write",
            "step": "x",
            "analysis": {"binding": {}, "faces": [], "features": [], "existing": []},
            "answers": {},
            "output": "y",
        }
    )
    assert out["error"]["code"] == "incomplete" and "part.material" in out["error"]["missing"]


def test_a_crash_inside_one_request_is_reported():
    out = reply({"id": 9, "op": "analyse", "step": "/no/such/file.step"})
    assert out["ok"] is False and out["error"]["code"] in ("failed", "invalid")
