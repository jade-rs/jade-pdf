"""固定AGL資料を検証し、heap確保不要のソート済みRust lookupを生成する。"""
import hashlib
from pathlib import Path
import json

ROOT = Path(__file__).resolve().parents[1]
PARSER = ROOT / 'crates/jade-pdf2docx-parser'
SOURCES = [
    ('AGL', ROOT / 'crates/jade-pdf-diff/resources/adobe-glyphlist.txt', 'a3b2f61ced9f3644cc0d4ecde5c59df34ca286c689d9484a43a710a81c466789'),
    ('ZAPF', PARSER / 'resources/zapfdingbats.txt', 'f6394e3cb8a447e84a1dad75d4baaf2aa7f45dc104faf369f4720e1a774ef2dc'),
]
COMMIT = '4036a9ca80a62f64f9de4f7321a9a045ad0ecfd6'


def generate():
    """出典ハッシュ・名前順・scalarを検査し、固定byteだけから同一出力を作る。"""
    tables = []
    inputs = []
    notice = None
    for name, path, expected in SOURCES:
        with path.open('rb') as stream:
            data = stream.read(80001)
        assert len(data) <= 80000 and hashlib.sha256(data).hexdigest() == expected, path
        text = data.decode('ascii')
        comments = []
        entries = []
        for line in text.splitlines():
            if line.startswith('#'):
                if not entries:
                    comments.append(line)
                continue
            if not line:
                continue
            glyph, values = line.split(';')
            assert glyph.isascii() and glyph.isalnum()
            points = [int(value, 16) for value in values.split()]
            assert points and all(0 <= value <= 0x10ffff and not 0xd800 <= value <= 0xdfff for value in points)
            assert not any(previous == glyph for previous, _ in entries)
            entries.append((glyph, ''.join('\\u{' + format(value, 'X') + '}' for value in points)))
        entries.sort(key=lambda entry: entry[0])
        if notice is None:
            notice = '\n'.join(line.removeprefix('#').removeprefix(' ') for line in comments)
        table = '/// Adobeの固定済み' + name + '名をASCII順に保存し、runtimeの索引確保を省く。\n#[rustfmt::skip]\npub(super) const ' + name + ': &[(&str, &str)] = &[\n'
        table += ''.join(f'    ("{glyph}", "{value}"),\n' for glyph, value in entries)
        table += '];\n'
        tables.append(table)
        filename = 'glyphlist.txt' if name == 'AGL' else 'zapfdingbats.txt'
        inputs.append(dict(table=name, input=str(path.relative_to(ROOT)), url=f'https://raw.githubusercontent.com/adobe-type-tools/agl-aglfn/{COMMIT}/{filename}', sha256=expected, bytes=len(data), entries=len(entries)))
    license_path = PARSER / 'licenses/adobe-glyph-list.txt'
    license_path.write_text(notice + f'\n\nSource: https://github.com/adobe-type-tools/agl-aglfn/tree/{COMMIT}\nApplies to glyphlist.txt and zapfdingbats.txt; no font programs are included.\n')
    generated = PARSER / 'src/glyph_names_data.rs'
    header = '//! tools/generate_glyph_names.pyで固定AGL資料から生成する。手動編集しない。\n'
    header += '// Copyright 2002-2019 Adobe (http://www.adobe.com/).\n'
    header += '// BSD-3-Clause: full notice retained in ../licenses/adobe-glyph-list.txt.\n'
    header += f'// Source revision: {COMMIT}\n\n'
    generated.write_text(header + '\n'.join(tables))
    report = dict(commit=COMMIT, generator='python3 tools/generate_glyph_names.py', inputs=inputs, generated=str(generated.relative_to(ROOT)), generated_sha256=hashlib.sha256(generated.read_bytes()).hexdigest(), license=str(license_path.relative_to(ROOT)), license_sha256=hashlib.sha256(license_path.read_bytes()).hexdigest())
    (PARSER / 'resources/adobe-glyph-list.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    generate()
