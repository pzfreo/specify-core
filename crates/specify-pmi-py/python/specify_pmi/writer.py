"""What a write reports: ``WriteReport``, ``VerificationError`` and ``EditsRefused``. The writing
is the Rust engine's (``api.write``, ``api.write_parts``).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class WriteReport:
    output: Path
    written: list[str] = field(default_factory=list)
    #: Intent that has no place in the file yet; it must reach the drawing another way.
    not_written: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    geometry_carrier_exemptions: list[dict[str, Any]] = field(default_factory=list)
    presentation_warnings: list[str] = field(default_factory=list)
    preserved_normalizations: list[dict[str, Any]] = field(default_factory=list)
    #: Ids of support Replace kept because PMI it could not read still uses it.
    retained_shared: list[int] = field(default_factory=list)
    #: Kernel findings about the default Notes saved view ({kind, part?, ids, entity, detail}).
    saved_view_findings: list[dict[str, Any]] = field(default_factory=list)
    #: An edit list's write (``api.write``): the characteristic numbers it retired.
    retired: list[int] = field(default_factory=list)
    #: An edit list's write: nothing changed, the file's own bytes were written.
    unchanged: bool = False
    #: Answers that only wait on another answer: not written, still open, the rest saved
    #: ({entry, id, waits_for, message}).
    waiting: list[dict[str, Any]] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        result = {
            "output": str(self.output),
            "written": self.written,
            "not_written": self.not_written,
            "warnings": self.warnings,
        }
        if self.geometry_carrier_exemptions:
            result["geometry_carrier_exemptions"] = self.geometry_carrier_exemptions
        if self.presentation_warnings:
            result["presentation_warnings"] = self.presentation_warnings
        if self.preserved_normalizations:
            result["preserved_normalizations"] = self.preserved_normalizations
        if self.retained_shared:
            result["retained_shared"] = self.retained_shared
        if self.saved_view_findings:
            result["saved_view_findings"] = self.saved_view_findings
        if self.waiting:
            result["waiting"] = self.waiting
        if self.retired:
            result["retired"] = self.retired
        if self.unchanged:
            result["unchanged"] = self.unchanged
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> WriteReport:
        return cls(
            Path(data["output"]),
            data["written"],
            data["not_written"],
            data["warnings"],
            data.get("geometry_carrier_exemptions", []),
            data.get("presentation_warnings", []),
            data.get("preserved_normalizations", []),
            data.get("retained_shared", []),
            data.get("saved_view_findings", []),
            data.get("retired", []),
            data.get("unchanged", False),
            data.get("waiting", []),
        )


class VerificationError(RuntimeError):
    pass


class EditsRefused(ValueError):
    """An edit list that cannot be applied: nothing was written. ``failures`` is every failing
    entry, ``[{"entry": ..., "reason": ...}]``."""

    def __init__(self, message: str, failures: list[dict[str, str]]):
        super().__init__(message)
        self.failures = failures
