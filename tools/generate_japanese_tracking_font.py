#!/usr/bin/env python3
"""Build independently drawn rectangles and an empty coded space; no external font data.

Development only: fonttools==4.60.2. The valid ToUnicode controls the test text;
every nonblank glyph uses the same shape to isolate logical edits from pixels.
"""
from io import BytesIO
import hashlib
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen


def generate():
    """Use full-em advances and a real empty U+0020 glyph, with fixed timestamps."""
    names=['.notdef','request','apply','book','kana','space']
    builder=FontBuilder(1000,isTTF=True)
    builder.setupGlyphOrder(names)
    glyphs={}
    for name in names:
        pen=TTGlyphPen(None)
        if name!='space':
            pen.moveTo((0,0))
            for point in [(700,0),(700,700),(0,700)]:pen.lineTo(point)
            pen.closePath()
        glyphs[name]=pen.glyph()
    builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics({name:(170 if name=='space' else 1000,0) for name in names})
    builder.setupHorizontalHeader(ascent=700,descent=0)
    builder.setupCharacterMap({0x7533:'request',0x8acb:'apply',0x66f8:'book',0x306e:'kana',0x20:'space'})
    builder.setupNameTable(dict(familyName='JadeTracking',styleName='Regular',fullName='JadeTracking',
                               psName='JadeTracking',uniqueFontIdentifier='Jade independent rectangle tracking test'))
    builder.setupOS2(sTypoAscender=700,sTypoDescender=0,usWinAscent=700,usWinDescent=0)
    builder.setupPost(keepGlyphNames=False)
    builder.font['head'].created=builder.font['head'].modified=2082844800
    builder.font.recalcTimestamp=False
    output=BytesIO();builder.font.save(output)
    return output.getvalue()


if __name__=='__main__':
    data=generate()
    print('sha256:',hashlib.sha256(data).hexdigest())
    text=data.hex().upper()+'>'
    for start in range(0,len(text),88):print('"'+text[start:start+88]+'",')
