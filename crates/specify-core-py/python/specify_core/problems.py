"""What may be wrong with a part's answers so far, the Rust engine's (``specify_core::problems``)."""

from __future__ import annotations

import json
from typing import Any

from . import _native


def problems(analysis: dict[str, Any], answers: dict[str, Any]) -> list[dict[str, Any]]:
    return json.loads(_native.problems(json.dumps(analysis), json.dumps(answers)))
