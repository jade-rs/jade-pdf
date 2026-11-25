"""正式xref付きの所有LZW本文で、実際のWASM公開入口を検証する。"""


def lzw(data):
    """毎byte辞書をclearし、幅変更と独立した正規の9bit列を作る。"""
    codes = [code for byte in data for code in (256, byte)] + [257]
    bits = ''.join(f'{code:09b}' for code in codes)
    bits += '0' * (-len(bits) % 8)
    return bytes(int(bits[index:index + 8], 2) for index in range(0, len(bits), 8))


def stream(data, parameters=''):
    """所有圧縮payloadを変更せず、正確なLengthで本文へ入れる。"""
    return (f'<< /Length {len(data)} /Filter /LZWDecode {parameters} >>\nstream\n'.encode()
            + data + b'\nendstream')


def document(contents, bodies):
    """固定object番号を使い、配列・共有streamを含むPDFをメモリ内で作る。"""
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>',
               b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
               f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents {contents} >>'.encode(),
               bodies[0],
               b'<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>',
               *bodies[1:]]
    data = bytearray(b'%PDF-1.2\n')
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(data))
        data += f'{number} 0 obj\n'.encode() + body + b'\nendobj\n'
    xref = len(data)
    data += f'xref\n0 {len(objects) + 1}\n0000000000 65535 f \n'.encode()
    for offset in offsets:
        data += f'{offset:010} 00000 n \n'.encode()
    data += f'trailer << /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode()
    return bytes(data)


def owned_lzw_cases():
    """既定・EarlyChange0・間接配列の反復と、破損・未対応の拒否を比較する。"""
    text = b'BT /F1 10 Tf 72 740 Td (LZW CONTENT) Tj ET'
    return [
        dict(id='lzw-direct', data=document('4 0 R', [stream(lzw(text))]),
             body={'LZW CONTENT': 72}, expected_issues=[]),
        dict(id='lzw-early-zero', data=document('4 0 R', [stream(lzw(text), '/DecodeParms << /EarlyChange 0 >>')]),
             body={'LZW CONTENT': 72}, expected_issues=[]),
        dict(id='lzw-indirect-repeat', data=document('6 0 R', [
            stream(lzw(b'BT /F1 10 Tf 72 740 Td')), b'[4 0 R 7 0 R 7 0 R 8 0 R]',
            stream(lzw(b'(LZW) Tj')), stream(lzw(b'ET'))]),
             body={'LZWLZW': 72}, expected_issues=[]),
        dict(id='lzw-corrupt', data=document('4 0 R', [stream(b'\x80')]), error='lzw'),
        dict(id='lzw-unsupported', data=document('4 0 R', [stream(lzw(text), '/DecodeParms << /Predictor 12 >>')]), error='lzw'),
    ]
