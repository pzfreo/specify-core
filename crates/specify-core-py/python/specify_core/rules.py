"""The questions a part needs: ``Question``, as specify-core's ``rules`` defines it.

The questions are the Rust engine's (``specify_core._native``); this is the type they come
back as, field for field, so a caller of specify-core's Python engine reads them unchanged.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

#: How much each question needs a person, so a part can be reviewed rather
#: than interrogated: ``required`` has no default and must be answered;
#: ``check`` has a default the rules doubt, with a ``reason``; ``routine`` has
#: a default from a standard or the geometry, accepted unless changed;
#: ``refine`` is an optional tightening that defaults to none or general.
ATTENTION = ("required", "check", "routine", "refine")


@dataclass(frozen=True)
class Question:
    id: str
    kind: str  # choice | text | faces
    prompt: str
    faces: tuple[int, ...]  # what to highlight
    default: Any = None
    options: tuple[str, ...] = ()
    why: str = ""
    feature: str | None = None
    #: For a position: the datum frame it is measured from, e.g. ``A|B|C``.
    frame: str | None = None
    #: How much a person needs to look at it; see ``ATTENTION``.
    attention: str = "routine"
    #: Why a ``check`` question was flagged.
    reason: str = ""
    #: When identical features are asked about once: each one's id and faces.
    #: Each still gets its own requirements.
    parts: tuple[tuple[str, tuple[int, ...]], ...] = ()
    #: For a text question: common answers to offer as it is typed; any text is accepted.
    suggestions: tuple[dict[str, Any], ...] = ()
    #: For a hole or turned diameter: each option's purpose, label, code and callout.
    choices: tuple[dict[str, Any], ...] = ()
    #: The feature's diameter, against which a typed spec is checked.
    diameter: float | None = None
    #: For a blind hole: its drill depth, against which a typed thread depth is checked.
    depth: float | None = None
    #: Why the default is what it is, in a sentence: shown as a choice made for
    #: the person, to keep or change.
    basis: str = ""
    #: When the default comes from how the part fits another in its assembly: why.
    mate: str = ""

    def to_dict(self) -> dict[str, Any]:
        out = {
            "id": self.id,
            "kind": self.kind,
            "prompt": self.prompt,
            "faces": list(self.faces),
            "default": list(self.default) if isinstance(self.default, tuple) else self.default,
            "why": self.why,
        }
        if self.options:
            out["options"] = list(self.options)
        if self.feature:
            out["feature"] = self.feature
        if self.frame:
            out["frame"] = self.frame
        out["attention"] = self.attention
        if self.reason:
            out["reason"] = self.reason
        if self.parts:
            out["features"] = [feature for feature, _ in self.parts]
        if self.suggestions:
            out["suggestions"] = list(self.suggestions)
        if self.choices:
            out["choices"] = list(self.choices)
        if self.basis:
            out["basis"] = self.basis
        return out

    @classmethod
    def from_record(cls, record: dict[str, Any]) -> Question:
        """A question from the engine's record of every field (JSON: lists for tuples)."""
        default = record["default"]
        return cls(
            id=record["id"],
            kind=record["kind"],
            prompt=record["prompt"],
            faces=tuple(record["faces"]),
            # A datum's default is its faces, a tuple in Python's rules.
            default=tuple(default) if isinstance(default, list) else default,
            options=tuple(record["options"]),
            why=record["why"],
            feature=record["feature"],
            frame=record["frame"],
            attention=record["attention"],
            reason=record["reason"],
            parts=tuple((id_, tuple(faces)) for id_, faces in record["parts"]),
            suggestions=tuple(record["suggestions"]),
            choices=tuple(record["choices"]),
            diameter=record["diameter"],
            depth=record["depth"],
            basis=record["basis"],
            mate=record["mate"],
        )
