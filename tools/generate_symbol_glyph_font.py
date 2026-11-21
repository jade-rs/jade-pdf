#!/usr/bin/env python3
"""Reproduce the independently drawn Symbol-metadata regression font.

Development only: fonttools==4.60.2. No external font data or outlines are used.
Every glyph is the same rectangle; the test checks encoding rather than shape.
"""
from io import BytesIO
import hashlib
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import newTable
from fontTools.ttLib.tables._c_m_a_p import CmapSubtable


def generate():
    """Create a post-3 TrueType with only a Microsoft Symbol cmap."""
    names = ['.notdef', 'lessEqual', 'less', 'sigma', 'Delta', 'mu']
    builder = FontBuilder(1000, isTTF=True)
    builder.setupGlyphOrder(names)
    glyphs = {}
    for name in names:
        pen = TTGlyphPen(None)
        pen.moveTo((0, 0))
        for point in [(500, 0), (500, 700), (0, 700)]:
            pen.lineTo(point)
        pen.closePath()
        glyphs[name] = pen.glyph()
    builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics({name: (500, 0) for name in names})
    builder.setupHorizontalHeader(ascent=700, descent=0)
    builder.setupCharacterMap({0xF0A3: 'lessEqual', 0xF03C: 'less',
                               0xF073: 'sigma', 0xF044: 'Delta', 0xF06D: 'mu'})
    builder.setupNameTable({'familyName': 'Symbol', 'styleName': 'Regular',
                           'fullName': 'Symbol', 'psName': 'SymbolMT',
                           'uniqueFontIdentifier': 'Jade independent rectangle test'})
    builder.setupOS2(sTypoAscender=700, sTypoDescender=0, usWinAscent=700, usWinDescent=0)
    builder.setupPost(keepGlyphNames=False)
    cmap = newTable('cmap')
    cmap.tableVersion = 0
    table = CmapSubtable.newSubtable(4)
    table.platformID, table.platEncID, table.language = 3, 0, 0
    table.cmap = {0xF0A3: 'lessEqual', 0xF03C: 'less', 0xF073: 'sigma',
                  0xF044: 'Delta', 0xF06D: 'mu'}
    cmap.tables = [table]
    builder.font['cmap'] = cmap
    builder.font['head'].created = builder.font['head'].modified = 2082844800
    builder.font.recalcTimestamp = False
    output = BytesIO()
    builder.font.save(output)
    return output.getvalue()


if __name__ == '__main__':
    data = generate()
    print('sha256:', hashlib.sha256(data).hexdigest())
    text = data.hex().upper() + '>'
    for start in range(0, len(text), 88):
        print('"' + text[start:start + 88] + '",')
