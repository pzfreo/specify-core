"""A file Specify wrote from a set of answers, for the tests that need one: the command line's
``specify-core write STEP ANALYSIS ANSWERS -o OUT`` (the whole intent of the answers, which the
command keeps). Python's ``api.write`` applies an edit list (Specify 0.3.0, ADR P11)."""

import json
import subprocess
import sys
import tempfile
from pathlib import Path

from specify_core import api
from specify_core.writer import WriteReport


def write_answers(step, answers, out, *, part=None, accept_defaults=False, bom=False) -> WriteReport:
    """Write ``answers`` (with every default, given ``accept_defaults``) into ``step`` at ``out``;
    the command's refusal (exit 2) is a ``ValueError`` with its message."""
    with tempfile.TemporaryDirectory() as scratch:
        analysis = Path(scratch) / "analysis.json"
        given = Path(scratch) / "answers.json"
        analysis.write_text(json.dumps(api.analyse(step, part)), encoding="utf-8")
        given.write_text(json.dumps(answers), encoding="utf-8")
        args = ["write", str(step), str(analysis), str(given), "-o", str(out)]
        args += ["--accept-defaults"] if accept_defaults else []
        args += ["--bom"] if bom else []
        done = subprocess.run([sys.executable, "-m", "specify_core", *args], capture_output=True,
                              text=True, encoding="utf-8")
    if done.returncode == 2:
        raise ValueError(done.stderr.strip())
    if done.returncode:
        raise RuntimeError(done.stderr.strip())
    return WriteReport.from_dict(json.loads(done.stdout))
