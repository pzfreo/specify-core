"""Text in a Part 21 file as ISO 10303-21 (second edition) allows it.

The files OCCT writes declare the second edition (``FILE_DESCRIPTION(..,
'2;1')``), whose strings are ASCII: any other character is written as a
``\\X2\\`` (UCS-2) or ``\\X4\\`` (UCS-4) escape. OCCT writes a material name
as raw UTF-8, as specify-core wrote "µm" -- which a conforming reader rejects.
Non-ASCII may appear only inside strings, so escaping every such character
in the file escapes exactly those.
"""

from __future__ import annotations

import re
from pathlib import Path

_RUN = re.compile(r"[^\x00-\x7f]+")
_X2 = re.compile(r"\\X2\\((?:[0-9A-Fa-f]{4})+)\\X0\\")
_X4 = re.compile(r"\\X4\\((?:[0-9A-Fa-f]{8})+)\\X0\\")


def escape(text: str) -> str:
    """``text`` with each run of non-ASCII characters as one escape."""

    def run(match: re.Match[str]) -> str:
        chars = match.group(0)
        if all(ord(c) <= 0xFFFF for c in chars):
            return "\\X2\\" + "".join(f"{ord(c):04X}" for c in chars) + "\\X0\\"
        return "\\X4\\" + "".join(f"{ord(c):08X}" for c in chars) + "\\X0\\"

    return _RUN.sub(run, text)


def unescape(text: str) -> str:
    """A string's text as written in a file, with its escapes read."""

    def x2(match: re.Match[str]) -> str:
        hexes = match.group(1)
        return "".join(chr(int(hexes[i : i + 4], 16)) for i in range(0, len(hexes), 4))

    def x4(match: re.Match[str]) -> str:
        hexes = match.group(1)
        return "".join(chr(int(hexes[i : i + 8], 16)) for i in range(0, len(hexes), 8))

    return _X4.sub(x4, _X2.sub(x2, text))


def escape_file(path: Path) -> None:
    """Rewrite ``path`` with every non-ASCII character escaped."""
    text = path.read_text(encoding="utf-8", errors="replace")
    escaped = escape(text)
    if escaped != text:
        path.write_text(escaped, encoding="ascii")
