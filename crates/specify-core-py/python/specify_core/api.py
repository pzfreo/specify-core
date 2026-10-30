"""The steps of specify-core's API, on the Rust engine: ``parts``, ``analyse``, ``questions``,
``apply`` and ``preview``.

The names, signatures and return shapes are specify-core's own ``api.py``; the work is done
by ``specify_core._native``. A file of several parts is specified a part at a time: ``parts``
lists them, and ``analyse`` takes the one to work on, by path or loaded once (``load``).
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

from . import _native
from .load import LoadedPart, load
from .rules import Intent, Question

__all__ = ["LoadedPart", "load", "parts", "analyse", "questions", "apply", "preview"]

ANALYSIS_SCHEMA = _native.ANALYSIS_SCHEMA


def parts(step: str | Path) -> list[dict[str, Any]]:
    """The distinct parts of ``step``: name, face count, instances and their
    placements, and whether specify-core has written the part (answers stored)."""
    return json.loads(_native.parts(_path(step)))


def analyse(step: str | Path | LoadedPart, part: int | None = None) -> dict[str, Any]:
    """Features, faces and existing PMI of ``step`` -- of its ``part``-th part, in
    an assembly -- as a JSON-safe snapshot.

    The snapshot is what gets stored: it is never re-derived for the same file,
    so its feature ids do not need to survive an engine upgrade.
    """
    if isinstance(step, LoadedPart):
        return json.loads(_native.analyse(step._handle))
    return json.loads(_native.analyse(_path(step), part))


def questions(analysis: dict[str, Any], answers: dict[str, Any]) -> list[Question]:
    records = json.loads(_native.questions(json.dumps(analysis), json.dumps(answers)))
    return [Question.from_record(r) for r in records]


def apply(
    analysis: dict[str, Any], answers: dict[str, Any], *, accept_defaults: bool = False
) -> Intent:
    """Intent from ``answers``; with ``accept_defaults``, unanswered defaults are taken.

    Defaults are resolved to a fixed point, because some questions only exist
    once an earlier default is taken.
    """
    text = _native.apply(json.dumps(analysis), json.dumps(answers), accept_defaults)
    return Intent.from_dict(json.loads(text))


def preview(analysis: dict[str, Any], answers: dict[str, Any]) -> Intent:
    """What the answers so far amount to; see the engine's ``rules::preview``."""
    return Intent.from_dict(json.loads(_native.preview(json.dumps(analysis), json.dumps(answers))))


def _path(step: Any) -> str:
    if not isinstance(step, str | os.PathLike):
        raise TypeError(f"expected a path or a LoadedPart, not {type(step).__name__}")
    return os.fspath(step)
