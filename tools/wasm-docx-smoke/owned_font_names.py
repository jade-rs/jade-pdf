"""表示名だけを検査する所有metadata fixture。描画可能fontの証拠には使わない。"""
import struct


def legacy_font_pdf(cid=False):
    """正式xrefと非UTF-8名を持つ、Simple/Type0 PDFを一時生成する。"""
    name = b'OwnedCJK'
    wide = name.decode().encode('utf-16-be')
    table = struct.pack('>15H', 0, 2, 30, 1, 0, 0, 6, len(name), 0,
                        3, 1, 0x409, 6, len(wide), len(name)) + name + wide
    font = bytearray(54)
    font[:6] = struct.pack('>IH', 0x10000, 2)
    font[12:16] = b'OS/2'
    font[20:28] = struct.pack('>II', 44, 10)
    font[28:32] = b'name'
    font[36:44] = struct.pack('>II', 54, len(table))
    font[52:54] = struct.pack('>H', 8)
    font += table
    raw = b'ABCDEF+#82#6c#82#72#96#be#92#a9'
    content = b'BT /F1 12 Tf 72 700 Td ' + (b'<0041>' if cid else b'(A)') + b' Tj ET'
    def stream(data):
        return b'<< /Length %d >>\nstream\n' % len(data) + data + b'\nendstream'
    objects = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>',
        stream(content),
        (b'<< /Type /Font /Subtype /Type0 /BaseFont /%s /Encoding /Identity-H /DescendantFonts [8 0 R] /ToUnicode 9 0 R >>' % raw if cid else
         b'<< /Type /Font /Subtype /TrueType /BaseFont /%s /Encoding /WinAnsiEncoding /FontDescriptor 6 0 R /FirstChar 65 /LastChar 65 /Widths [600] >>' % raw),
        b'<< /Type /FontDescriptor /FontName /%s /FontFile2 7 0 R >>' % raw,
        stream(font),
    ]
    if cid:
        objects += [b'<< /Type /Font /Subtype /CIDFontType2 /BaseFont /%s /FontDescriptor 6 0 R /DW 600 >>' % raw,
                    stream(b'1 beginbfchar\n<0041> <0041>\nendbfchar')]
    data = bytearray(b'%PDF-1.7\n')
    offsets = []
    for index, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data += b'%d 0 obj\n' % index + obj + b'\nendobj\n'
    xref = len(data)
    data += b'xref\n0 %d\n0000000000 65535 f \n' % (len(objects) + 1)
    for offset in offsets:
        data += b'%010d 00000 n \n' % offset
    data += b'trailer << /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % (len(objects) + 1, xref)
    return bytes(data)
