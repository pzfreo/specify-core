"""Hole and turned-diameter choices, and typed specs, the Rust engine's (``specify_core::choices``):
``interpret``, ``check``, ``describe``, ``code``, ``callout`` and ``SpecError``, as specify-core's
``choices`` defines them.
"""

from __future__ import annotations

import json
from typing import Any

from . import _native


class SpecError(ValueError):
    """A typed spec that cannot be read, or that does not fit the feature."""


def interpret(
    qid: str, text: str, options, diameter: float | None, depth: float | None = None
) -> str:
    """The answer ``text`` means for question ``qid``; ``options`` are those
    offered, ``diameter`` the feature's, if it has one, and ``depth`` a blind
    hole's drill depth, against which a thread depth is checked."""
    return _native.choices_interpret(qid, text, list(options), diameter, depth)


def check(
    qid: str, value: Any, options, diameter: float | None, depth: float | None = None
) -> None:
    """Refuse an answer that is neither offered nor a spec that fits the feature."""
    _native.choices_check(qid, json.dumps(value), list(options), diameter, depth)


def describe(qid: str, options, diameter: float | None) -> list[dict[str, Any]]:
    """Each option of a hole or turned-diameter question, for choosing by
    purpose or by code; empty for any other question."""
    return json.loads(_native.choices_describe(qid, list(options), diameter))


def code(value: str) -> str:
    """An answer as an engineer would name it: ``H7``, ``M6 clearance``, ``M5 tapped``."""
    return _native.choices_code(value)


def callout(value: str, d: float, hole: bool) -> str:
    """What the answer puts on the drawing for a feature of diameter ``d``."""
    return _native.choices_callout(value, d, hole)
