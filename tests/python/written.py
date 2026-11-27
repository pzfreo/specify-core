"""Write the edit list approved by a test through the command line."""

import json
import subprocess
import sys
import tempfile
from pathlib import Path

from specify_pmi import api
from specify_pmi.rules import with_all_defaults
from specify_pmi.writer import WriteReport


def approved_edits(analysis, answers, *, accept_defaults=False, bom=False, skip_suggestions=()):
    """Use listed gap suggestions and preserve any values supplied by the caller."""
    fill = {key: value for key, value in answers.items() if not key.startswith("_")}
    accept = []
    if accept_defaults:
        offered = with_all_defaults(analysis, answers)
        if "part.locating" in offered and "part.locating" not in fill:
            fill["part.locating"] = offered["part.locating"]
        for row in analysis["table"]["rows"]:
            key = row["id"]
            if (row["state"] != "gap" or key in fill or key not in offered or key in skip_suggestions
                    or key == "part.process" or key.startswith(("bom.", "hole.full_thread:"))):
                continue
            if row.get("suggestion") == offered[key]:
                accept.append(key)
            else:
                fill[key] = offered[key]
    if bom:
        accept.extend(q.id for q in api.questions(analysis, answers)
                      if q.id.startswith("bom.") and q.suggestions and q.id not in fill)
    return {"fill": fill, "accept_suggestions": accept}


def write_answers(step, answers, out, *, part=None, accept_defaults=False, bom=False,
                  skip_suggestions=()) -> WriteReport:
    """Apply the given values and explicitly named approvals to the selected part."""
    analysis = api.analyse(step, part)
    edits = approved_edits(analysis, answers, accept_defaults=accept_defaults, bom=bom,
                           skip_suggestions=skip_suggestions)
    with tempfile.TemporaryDirectory() as scratch:
        given = Path(scratch) / "edits.json"
        given.write_text(json.dumps(edits), encoding="utf-8")
        args = ["write", str(step), str(given), "-o", str(out)]
        if part is not None:
            args += ["--part", str(part)]
        done = subprocess.run([sys.executable, "-m", "specify_pmi", *args], capture_output=True,
                              text=True, encoding="utf-8")
    if done.returncode == 2:
        raise ValueError(done.stderr.strip())
    if done.returncode:
        raise RuntimeError(done.stderr.strip())
    return WriteReport.from_dict(json.loads(done.stdout))
