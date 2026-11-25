"""固定したAdobe登録CID表から、確保不要のUTF-8 lookupと出典記録を生成する。"""
from pathlib import Path
import hashlib
import json
import re
import struct

ROOT = Path(__file__).resolve().parents[1]
PARSER = ROOT / 'crates/jade-pdf2docx-parser'
COMMIT = '2dd5e53fb74a01718b9dfd448a0d1cce6fff2aa5'
EXPECTED = '6a9693361647a37996312cc57071bb79f8c06411207be7c730a83fda1254cd82'


def generate():
    """全CIDとUTF-16を検証し、offsetをlittle endianで固定して再現可能に保存する。"""
    source = PARSER / 'resources/Adobe-Japan1-UCS2'
    raw = source.read_bytes()
    assert hashlib.sha256(raw).hexdigest() == EXPECTED
    mappings = {}
    mode = None
    remaining = 0
    for line in raw.decode('ascii').splitlines():
        if match := re.fullmatch(r'(\d+) begin(bfchar|bfrange)', line):
            assert mode is None
            remaining, mode = int(match[1]), match[2]
        elif line in ('endbfchar', 'endbfrange'):
            assert remaining == 0 and line == 'end' + mode
            mode = None
        elif mode:
            values = re.fullmatch(r'<([0-9a-f]{4})> <([0-9a-f]+)>(?: <([0-9a-f]+)>)?', line)
            assert values, line
            first = int(values[1], 16)
            last = first if mode == 'bfchar' else int(values[2], 16)
            destination = values[2] if mode == 'bfchar' else values[3]
            assert destination is not None and len(destination) % 4 == 0
            assert 0 <= first <= last < 23060
            for cid in range(first, last + 1):
                value = (int(destination, 16) + cid - first).to_bytes(len(destination) // 2, 'big')
                text = value.decode('utf-16-be')
                assert text and cid not in mappings and '\x00' not in text
                mappings[cid] = text
            remaining -= 1
    assert mode is None and set(mappings) == set(range(23060))
    assert [mappings[c] for c in [3284, 3722, 1952, 23058, 23059]] == ['日', '本', '語', '㋿', '㋿']
    offsets = [0]
    chunks = []
    for cid in range(23060):
        encoded = mappings[cid].encode('utf-8')
        chunks.append(encoded)
        offsets.append(offsets[-1] + len(encoded))
    outputs = {
        'adobe-japan1-ucs2.offsets': struct.pack('<' + 'I' * len(offsets), *offsets),
        'adobe-japan1-ucs2.utf8': b''.join(chunks),
    }
    generated = []
    for name, data in outputs.items():
        path = PARSER / 'resources' / name
        path.write_bytes(data)
        generated.append(dict(path=str(path.relative_to(ROOT)), bytes=len(data), sha256=hashlib.sha256(data).hexdigest()))
    notice = '\n'.join(line.removeprefix('%%Copyright:').removeprefix(' ') for line in raw.decode('ascii').splitlines() if line.startswith('%%Copyright:'))
    license_path = PARSER / 'licenses/adobe-japan1-ucs2.txt'
    license_path.write_text(notice + f'\n\nSource: https://github.com/adobe-type-tools/mapping-resources-pdf/blob/{COMMIT}/pdf2unicode/Adobe-Japan1-UCS2\nApplies to the original CID mapping and its generated lookup data; no font programs included.\n')
    report = dict(commit=COMMIT, version='10.002', source=str(source.relative_to(ROOT)), source_sha256=EXPECTED,
                  URL=f'https://raw.githubusercontent.com/adobe-type-tools/mapping-resources-pdf/{COMMIT}/pdf2unicode/Adobe-Japan1-UCS2',
                  generator='python3 tools/generate_registered_cids.py', entries=len(mappings), generated=generated,
                  license=str(license_path.relative_to(ROOT)), license_sha256=hashlib.sha256(license_path.read_bytes()).hexdigest(),
                  collection_specification_URL='https://github.com/adobe-type-tools/Adobe-Japan1/blob/b42b377d7efbc233ae2a4c1833947e04760e68d1/README.md')
    (PARSER / 'resources/adobe-japan1-ucs2.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    generate()
