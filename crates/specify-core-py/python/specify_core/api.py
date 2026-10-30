"""The steps of specify-core's API, on the Rust engine: ``parts``, ``analyse``, ``questions``.

The names, signatures and return shapes are specify-core's own ``api.py``; the work is done
by ``specify_core._native``. A file of several parts is specified a part at a time: ``parts``
lists them, and ``analyse`` takes the one to work on.

What differs: a ``LoadedPart`` cannot cross into the engine, so ``analyse`` takes a path (and
the part's index) where specify-core also takes a part already loaded.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

from . import _native
from .rules import Question

ANALYSIS_SCHEMA = _native.ANALYSIS_SCHEMA


def parts(step: str | Path) -> list[dict[str, Any]]:
    """The distinct parts of ``step``: name, face count, instances and their
    placements, and whether specify-core has written the part (answers stored)."""
    return json.loads(_native.parts(os.fspath(step)))


def analyse(step: str | Path, part: int | None = None) -> dict[str, Any]:
    """Features, faces and existing PMI of ``step`` -- of its ``part``-th part, in
    an assembly -- as a JSON-safe snapshot.

    The snapshot is what gets stored: it is never re-derived for the same file,
    so its feature ids do not need to survive an engine upgrade.
    """
    if not isinstance(step, str | os.PathLike):
        raise TypeError(f"analyse takes a path, not {type(step).__name__}")
    return json.loads(_native.analyse(os.fspath(step), part))


def questions(analysis: dict[str, Any], answers: dict[str, Any]) -> list[Question]:
    records = json.loads(_native.questions(json.dumps(analysis), json.dumps(answers)))
    return [Question.from_record(r) for r in records]
