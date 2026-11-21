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


def generate(profile="symbol"):
    """Create a post-3 TrueType with only a Microsoft Symbol cmap."""
    names = ['.notdef', 'lessEqual', 'less', 'sigma', 'Delta', 'mu'] if profile == 'symbol' else ['.notdef', 'ellipsis', 'ell', 'bracepart']
    encoding = ({0xF0A3: 'lessEqual', 0xF03C: 'less', 0xF073: 'sigma', 0xF044: 'Delta', 0xF06D: 'mu'} if profile == 'symbol' else {0xF04B: 'ellipsis', 0xF06C: 'ell', 0xF031: 'bracepart'})
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
    builder.setupCharacterMap(encoding)
    family, ps = ('Symbol', 'SymbolMT') if profile == 'symbol' else ('MT Extra', 'MT-Extra')
    builder.setupNameTable({'familyName': family, 'styleName': 'Regular',
                           'fullName': family, 'psName': ps,
                           'uniqueFontIdentifier': 'Jade independent rectangle test'})
    builder.setupOS2(sTypoAscender=700, sTypoDescender=0, usWinAscent=700, usWinDescent=0)
    builder.setupPost(keepGlyphNames=False)
    cmap = newTable('cmap')
    cmap.tableVersion = 0
    table = CmapSubtable.newSubtable(4)
    table.platformID, table.platEncID, table.language = 3, 0, 0
    table.cmap = encoding
    cmap.tables = [table]
    builder.font['cmap'] = cmap
    builder.font['head'].created = builder.font['head'].modified = 2082844800
    builder.font.recalcTimestamp = False
    output = BytesIO()
    builder.font.save(output)
    return output.getvalue()


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=["symbol", "mtextra"], default="symbol")
    data = generate(parser.parse_args().profile)
    print('sha256:', hashlib.sha256(data).hexdigest())
    text = data.hex().upper() + '>'
    for start in range(0, len(text), 88):
        print('"' + text[start:start + 88] + '",')
