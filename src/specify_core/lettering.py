"""Text as strokes, for presentation a viewer draws without a font.

AP242 presentation carries no characters, only geometry, so a note a viewer
is to show must be drawn. Single-stroke letters (Hershey's, ``fonts/``) are
polylines, as a pen plotter writes them: a fraction of the size of filled
glyphs, which would have to be triangulated.

Glyphs are in font units with the baseline at 0, the capital height 21 and
the descender at -7; ``line`` scales them to a capital height.
"""

from __future__ import annotations

from functools import cache
from pathlib import Path

Stroke = list[tuple[float, float]]

#: Capital height in font units.
CAP = 21.0


@cache
def _font() -> dict[str, tuple[float, list[Stroke]]]:
    """Character -> (advance, strokes) for ASCII 32 to 126, from James Hurt's format:
    a 5-character number, a 3-character vertex count, then pairs of characters each
    offset from 'R' -- the left and right bearings, then x, y, with " R" lifting the pen."""
    raw = (Path(__file__).parent / "fonts" / "futural.jhf").read_text().replace("\n", "")
    glyphs = []
    i = 0
    while i < len(raw):
        count = int(raw[i + 5 : i + 8])
        body = raw[i + 8 : i + 8 + 2 * count]
        i += 8 + 2 * count
        left, right = ord(body[0]) - 82, ord(body[1]) - 82
        strokes: list[Stroke] = []
        current: Stroke = []
        for k in range(2, len(body), 2):
            if body[k : k + 2] == " R":
                strokes.append(current)
                current = []
                continue
            current.append((ord(body[k]) - 82 - left, 9 - (ord(body[k + 1]) - 82)))
        strokes.append(current)
        glyphs.append((float(right - left), [s for s in strokes if len(s) > 1]))
    return {chr(32 + k): glyph for k, glyph in enumerate(glyphs[:95])}


def _glyph(char: str) -> tuple[float, list[Stroke]]:
    """A glyph; '?' for a character the font does not have."""
    font = _font()
    return font.get(char, font["?"])


def line(text: str, height: float) -> tuple[list[Stroke], float]:
    """The strokes of one line of ``text`` at capital ``height``, starting at x=0
    on the baseline, and its width."""
    scale = height / CAP
    out: list[Stroke] = []
    x = 0.0
    for char in text:
        width, strokes = _glyph(char)
        out += [[((x + px) * scale, py * scale) for px, py in s] for s in strokes]
        x += width
    return out, x * scale
