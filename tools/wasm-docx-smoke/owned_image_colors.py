"""所有二画素RGB画像の原本と、依存なしのPNG画素検証。"""
import struct
import zlib


def singleton_rgb_pdf():
    """二画素の色を、一要素DeviceRGB配列の正式xref付きPDFへ埋め込む。"""
    content = b'BT /F1 12 Tf 72 730 Td (Anchor) Tj ET q 20 0 0 10 72 650 cm /Im1 Do Q'
    image = zlib.compress(bytes([12, 34, 56, 78, 90, 123]))
    objects = [
        b'<< /Type /Catalog /Pages 2 0 R >>',
        b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 6 0 R >> >> /Contents 4 0 R >>',
        b'<< /Length %d >>\nstream\n' % len(content) + content + b'\nendstream',
        b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>',
        b'<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace [/DeviceRGB] /Filter /FlateDecode /Length %d >>\nstream\n' % len(image) + image + b'\nendstream',
    ]
    data = bytearray(b'%PDF-1.7\n')
    offsets = []
    for index, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data += b'%d 0 obj\n' % index + obj + b'\nendobj\n'
    xref = len(data)
    data += b'xref\n0 7\n0000000000 65535 f \n'
    for offset in offsets:
        data += b'%010d 00000 n \n' % offset
    data += b'trailer << /Size 7 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n' % xref
    return bytes(data)


def owned_rgb_pixels(png):
    """所有2×1・8bitRGBだけを、有効なchunk CRCと全filterに従って復号する。"""
    assert png[:8] == b'\x89PNG\r\n\x1a\n' and len(png) <= 4096
    offset, compressed, header, ended = 8, bytearray(), None, False
    while offset < len(png):
        assert offset + 12 <= len(png)
        size = int.from_bytes(png[offset:offset+4], 'big')
        kind = png[offset+4:offset+8]
        data = png[offset+8:offset+8+size]
        assert offset+size+12 <= len(png)
        assert zlib.crc32(kind+data) == int.from_bytes(png[offset+8+size:offset+12+size], 'big')
        if kind == b'IHDR':
            assert header is None
            header = struct.unpack('>IIBBBBB', data)
        elif kind == b'IDAT':
            compressed += data
        elif kind == b'IEND':
            assert not data and offset+size+12 == len(png)
            ended = True
        offset += size+12
    assert header == (2, 1, 8, 2, 0, 0, 0) and ended
    decoder = zlib.decompressobj()
    row = decoder.decompress(compressed, 8)
    assert len(row) == 7 and decoder.eof and not decoder.unused_data and not decoder.unconsumed_tail
    method, pixels = row[0], bytearray(row[1:])
    assert 0 <= method <= 4
    for index in range(6):
        left = pixels[index-3] if index >= 3 else 0
        prediction = left if method in (1, 4) else left//2 if method == 3 else 0
        pixels[index] = (pixels[index]+prediction) % 256
    return list(pixels)
