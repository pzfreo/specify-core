"""For #87: Python's existing analyse/write/diff calls use named resume."""
import base64
import gzip
import json
import re
from pathlib import Path

from specify_core import api

FIXTURES = Path(__file__).resolve().parents[1] / "fixtures" / "revisions"
ITEM = re.compile(r"(DESCRIPTIVE_REPRESENTATION_ITEM\('pmi-assist answers',')([A-Za-z0-9+/=]+)('\))")


def test_python_named_rebuild_resume_and_diff(tmp_path):
    def open_revision(revision):
        path = tmp_path / f"{revision}.step"
        path.write_bytes(gzip.decompress((FIXTURES / f"named_plate.{revision}.step.gz").read_bytes()))
        return path

    old, new = open_revision("old"), open_revision("new")

    def answers(path):
        faces = api.analyse(path)["faces"]
        def face(feature, n):
            name = f"f:named_plate.{feature}.{n};M:named_plate.body"
            return next(f["id"] for f in faces if f.get("name") == name)
        hole, top, bottom = face("body_make_cylinder1", 1), face("body_make_box1", 6), face("body_make_box1", 5)
        return {
            "part.material": "steel", f"hole.function:holes:{hole}": "fit:H7",
            "datum.A": [bottom], "datum.A.flatness": "0.03",
            "extra.1": {"kind": "parallelism", "faces": [top, bottom], "datums": ["A"], "value": "0.1"},
        }

    before, after = answers(old), answers(new)
    assert before != after
    report = api.diff(old, new, before)["report"]
    assert report["answers"] == after and not report["unresolved"]
    written = []
    for path, given in [(old, before), (new, after)]:
        out = tmp_path / f"{path.stem}-pmi.step"
        api.write(path, api.preview(api.analyse(path), given), out, answers=given)
        assert api.analyse(out)["resume"]["answers"] == given
        written.append(out)
    # Carry the stored schema-3 evidence into a rebuilt file, as a CAD author would.
    source, target = (p.read_text() for p in written)
    payload = ITEM.search(source).group(2)
    assert json.loads(base64.b64decode(payload))["schema"] == 3
    target = ITEM.sub(lambda m: m[1] + payload + m[3], target)
    written[1].write_text(target)
    assert api.analyse(written[1])["resume"]["answers"] == after
    missing, count = re.subn(r"(ADVANCED_FACE\s*\(\s*)'f:named_plate\.body_make_cylinder1\.1;M:named_plate\.body'", r"\1''", target)
    assert count == 1
    written[1].write_text(missing)
    resumed = api.analyse(written[1])["resume"]
    assert len(resumed["unresolved"]) == 1
    assert "not in this revision" in resumed["unresolved"][0]["reason"]
