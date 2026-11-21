#!/usr/bin/env python3
"""Reproduce the self-authored rectangular CFF in appearance_fixture.rs.

Development only: uv run --with fonttools==4.60.2 python tools/generate_named_glyph_font.py
No external fonts, glyph outlines or PDF inputs are reused or emitted.
"""
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.t2CharStringPen import T2CharStringPen
import hashlib

NAMES = ['.notdef', 'A', 'B', 'C', 'u1040C', 'u2040C', 'uni040C',
         'u1F600', 'u1D400', 'u1D15E']


def generate():
    """Assign independent glyph names to identical, explicitly drawn rectangles."""
    charstrings = {}
    for name in NAMES:
        pen = T2CharStringPen(500, None)
        pen.moveTo((0, 0))
        for point in [(500, 0), (500, 700), (0, 700)]:
            pen.lineTo(point)
        pen.closePath()
        charstrings[name] = pen.getCharString()
    builder = FontBuilder(1000, isTTF=False)
    builder.setupGlyphOrder(NAMES)
    builder.setupCFF('JadeNamedGlyphs', dict(FullName='Jade Named Glyphs',
                     FamilyName='Jade Named Glyphs', Weight='Regular'), charstrings, {})
    return builder.font['CFF '].compile(builder.font)


if __name__ == '__main__':
    data = generate()
    print('sha256:', hashlib.sha256(data).hexdigest())
    text = data.hex().upper() + '>'
    for start in range(0, len(text), 88):
        print('"' + text[start:start + 88] + '",')
