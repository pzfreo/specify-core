"""The four steps: analyse, questions, apply, write -- and a mesh for picking faces.

A file of several parts is specified a part at a time: ``parts`` lists them,
``analyse`` and ``mesh`` take the one to work on, and ``write_parts`` writes
any number of them into the assembly together.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from . import mates, resume, rules
from .existing import part_settings, read_existing
from .features import describe_faces, recognise
from .load import LoadedPart, load, load_all
from .load import parts as _parts
from .mesh import mesh as _mesh
from .rules import Intent, Question
from .writer import WriteReport
from .writer import write as _write
from .writer import write_parts as _write_parts

ANALYSIS_SCHEMA = 1


def parts(step: str | Path) -> list[dict[str, Any]]:
    """The distinct parts of ``step``: name, face count, instances and their
    placements, and whether specify-core has written the part (answers stored)."""
    stored = resume.stored(Path(step))
    return [p | {"written": p["part"] in stored} for p in _parts(step)]


def analyse(step: str | Path | LoadedPart, part: int | None = None) -> dict[str, Any]:
    """Features, faces and existing PMI of ``step`` -- of its ``part``-th part, in
    an assembly -- as a JSON-safe snapshot.

    The snapshot is what gets stored: it is never re-derived for the same file,
    so its feature ids do not need to survive a Quiddity upgrade.
    """
    loaded = step if isinstance(step, LoadedPart) else load(step, part=part)
    analysis = {
        "schema": ANALYSIS_SCHEMA,
        "binding": loaded.binding.to_dict(),
        "faces": [f.to_dict() for f in describe_faces(loaded)],
        "features": [f.to_dict() for f in recognise(loaded)],
    }
    if loaded.part_count > 1:
        # How the part fits the others: suggested answers, and the faces it
        # rests on another part by (see ``mates``).
        assembly = load_all(loaded.path, gdt=False)
        analysis["mates"] = mates.mates(assembly, loaded.binding.part)
        for index, contact in mates.contacts(assembly, loaded.binding.part).items():
            analysis["faces"][int(index)]["contact"] = contact
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
    """Write ``intent`` into ``step``, on the part it was made for. Given the
    ``answers`` it came from, they are stored in the file too, so that opening it
    again resumes them (``resume``)."""
    loaded = step if isinstance(step, LoadedPart) else load(step, part=_part(intent))
    return _write(loaded, intent, output, answers=answers)


def write_parts(
    step: str | Path, intents: list[tuple[Intent, dict[str, Any] | None]], output: str | Path
) -> WriteReport:
    """Write several parts of the assembly ``step`` together: each intent on the
    part it was made for, with the answers it came from (or None)."""
    loaded = load_all(step)
    for intent, _ in intents:
        if not 0 <= _part(intent) < len(loaded):
            raise ValueError(f"{step} has no part {_part(intent)}")
    return _write_parts([(loaded[_part(i)], i, a) for i, a in intents], output)


def _part(intent: Intent) -> int:
    # An analysis made before parts were told apart is of a file of one part.
    return int(intent.binding.get("part", 0))


def mesh(step: str | Path | LoadedPart, part: int | None = None) -> dict[str, Any]:
    """Triangles of ``step`` (its ``part``-th part) ranged by the face indices its
    analysis uses, in the part's own frame."""
    loaded = step if isinstance(step, LoadedPart) else load(step, part=part)
    return _mesh(loaded)
