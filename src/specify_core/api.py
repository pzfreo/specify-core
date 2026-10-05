"""The four steps: analyse, questions, apply, write -- and a mesh for picking faces."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from . import resume, rules
from .existing import part_settings, read_existing
from .features import describe_faces, recognise
from .load import LoadedPart, load
from .mesh import mesh as _mesh
from .rules import Intent, Question
from .writer import WriteReport
from .writer import write as _write

ANALYSIS_SCHEMA = 1


def analyse(step: str | Path | LoadedPart) -> dict[str, Any]:
    """Features, faces and existing PMI of ``step``, as a JSON-safe snapshot.

    The snapshot is what gets stored: it is never re-derived for the same file,
    so its feature ids do not need to survive a Quiddity upgrade.
    """
    loaded = step if isinstance(step, LoadedPart) else load(step)
    analysis = {
        "schema": ANALYSIS_SCHEMA,
        "binding": loaded.binding.to_dict(),
        "faces": [f.to_dict() for f in describe_faces(loaded)],
        "features": [f.to_dict() for f in recognise(loaded)],
    }
    answers = resume.answers_for(loaded)
    if answers is not None:
        # A file specify-core wrote, opened to be changed: its PMI is the answers',
        # not the file's to keep, and the answers come back to be edited.
        return analysis | {"existing": [], "part": {}, "resume": {"answers": answers}}
    return analysis | {
        "existing": [e.to_dict() for e in read_existing(loaded)],
        "part": part_settings(loaded),
    }


def questions(analysis: dict[str, Any], answers: dict[str, Any]) -> list[Question]:
    return rules.questions(analysis, answers)


def apply(
    analysis: dict[str, Any], answers: dict[str, Any], *, accept_defaults: bool = False
) -> Intent:
    """Intent from ``answers``; with ``accept_defaults``, unanswered defaults are taken.

    Defaults are resolved to a fixed point, because some questions only exist
    once an earlier default is taken.
    """
    if accept_defaults:
        answers = rules.with_all_defaults(analysis, answers)
    return rules.apply(analysis, answers)


def preview(analysis: dict[str, Any], answers: dict[str, Any]) -> Intent:
    """What the answers so far amount to, defaults standing in; see ``rules.preview``."""
    return rules.preview(analysis, answers)


def write(
    step: str | Path | LoadedPart,
    intent: Intent,
    output: str | Path,
    *,
    answers: dict[str, Any] | None = None,
) -> WriteReport:
    """Write ``intent`` into ``step``. Given the ``answers`` it came from, they are
    stored in the file too, so that opening it again resumes them (``resume``)."""
    loaded = step if isinstance(step, LoadedPart) else load(step)
    return _write(loaded, intent, output, answers=answers)


def mesh(step: str | Path | LoadedPart) -> dict[str, Any]:
    """Triangles of ``step`` ranged by the face indices its analysis uses."""
    loaded = step if isinstance(step, LoadedPart) else load(step)
    return _mesh(loaded)
