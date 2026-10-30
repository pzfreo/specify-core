"""specify-core's JSON-lines service, the Rust engine's (``specify_core::serve``).

``reply`` answers one request in this process, as specify-core's ``serve.reply`` does, with
one session's parts kept for anchoring between requests. ``specify-core serve`` (or
``cli.main(["serve"])``) is the service over stdin and stdout.
"""

from __future__ import annotations

import json
from typing import Any

from . import _native

#: The protocol's version, answered to ``hello``.
PROTOCOL = 1

_SESSION = _native.Server()


def reply(request: Any) -> dict[str, Any]:
    """The reply to ``request``: ``{id, ok: true, result}`` or ``{id, ok: false, error}``."""
    return json.loads(_SESSION.reply(json.dumps(request)))
