"""所有のmetadata専用Type1で、実WASMのUnicode対応と描画code診断を検査する。"""


def stream(data, attributes=''):
    """非圧縮の所有byte列に、正確なLengthを付ける。"""
    return f'<< /Length {len(data)} {attributes} >>\nstream\n'.encode() + data + b'\nendstream'


def pdf(builtin, encoding='', unicode=False, text='A'):
    """CharStringsを含まないため、描画用fontの妥当性の証明とは区別する。"""
    program = ('%!PS-AdobeFont-1.0: OwnedBuiltin 1.0\n11 dict begin\n'
               '/FontName /OwnedBuiltin def /FontType 1 def\n'
               f'/Encoding {builtin} readonly def\ncurrentdict end\ncurrentfile eexec\n').encode()
    mapping = '/ToUnicode 8 0 R' if unicode else ''
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>',
               b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
               b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>',
               stream(f'BT /F1 12 Tf 72 700 Td ({text}) Tj ET'.encode()),
               f'<< /Type /Font /Subtype /Type1 /BaseFont /OwnedBuiltin /FontDescriptor 6 0 R {encoding} {mapping} >>'.encode(),
               b'<< /Type /FontDescriptor /FontName /OwnedBuiltin /Flags 32 /FontBBox [0 0 600 700] /Ascent 700 /Descent -200 /CapHeight 700 /StemV 80 /FontFile 7 0 R >>',
               stream(program, f'/Length1 {len(program)} /Length2 0 /Length3 0'),
               stream(b'begincmap\n1 begincodespacerange\n<00> <ff>\nendcodespacerange\n1 beginbfchar\n<41> <005a>\nendbfchar\nendcmap')]
    data = bytearray(b'%PDF-1.4\n')
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


def owned_type1_cases():
    """作者が指定したglyph名とToUnicodeの優先順位を、固定Unicode期待値で照合する。"""
    issue = [dict(kind='unsupported_font', page=0, object_number=5, reason='unverified_encoding')]
    return [
        dict(id='type1-builtin', data=pdf('256 array dup 65 /B put'), body={'B': 72}, expected_text='B', expected_issues=[]),
        dict(id='type1-differences', data=pdf('256 array dup 65 /B put dup 66 /C put', '/Encoding << /Differences [66 /D] >>', text='AB'), body={'BD': 72}, expected_text='BD', expected_issues=[]),
        dict(id='type1-opaque', data=pdf('256 array dup 65 /G41 put'), body={}, expected_text='\ufffd', expected_issues=issue),
        dict(id='type1-partial-unicode', data=pdf('256 array dup 65 /G41 put dup 66 /G42 put', unicode=True, text='AB'), body={}, expected_text='Z\ufffd', expected_issues=issue),
        dict(id='type1-complete-unicode', data=pdf('256 array dup 65 /G41 put', unicode=True), body={'Z': 72}, expected_text='Z', expected_issues=[]),
        dict(id='type1-standard-quotes', data=pdf('StandardEncoding', text=r'\047\140\256'), body={}, expected_text='\u2019\u2018\ufb01', expected_issues=[]),
    ]
