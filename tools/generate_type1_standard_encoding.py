"""固定fontTools資料の256要素だけを読み、StandardEncoding表を再生成する。"""
import ast
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PARSER = ROOT / 'crates/jade-pdf2docx-parser'


def generate():
    """sourceを実行せずhashと要素を確認し、既存のRust表と同じbyte列を生成する。"""
    provenance = PARSER / 'resources/standard-encoding.json'
    report = json.loads(provenance.read_text())
    source = PARSER / 'resources/fonttools-standard-encoding.py.txt'
    data = source.read_bytes()
    assert len(data) == report['source_bytes']
    assert hashlib.sha256(data).hexdigest() == report['source_sha256']
    assignments = [node for node in ast.parse(data).body if isinstance(node, ast.Assign)
                   and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name)
                   and node.targets[0].id == 'StandardEncoding']
    assert len(assignments) == 1
    values = ast.literal_eval(assignments[0].value)
    assert isinstance(values, list) and len(values) == 256
    assert all(isinstance(value, str) and value.isascii()
               and all(c.isalnum() or c == '.' for c in value) for value in values)
    generated = ('//! fontToolsの固定StandardEncoding表。出典・MIT許諾はresources/standard-encoding.jsonとlicenses/fonttools.txt。\n'
                 '// Copyright (c) 2017 Just van Rossum\n\n'
                 '/// Adobe StandardEncodingの256個のglyph名を、code順に保存する。\n'
                 '#[rustfmt::skip]\npub(crate) const STANDARD: [&[u8]; 256] = [\n'
                 + ''.join(f'    b"{value}",\n' for value in values) + '];\n')
    assert hashlib.sha256(generated.encode()).hexdigest() == report['generated_sha256']
    assert hashlib.sha256((PARSER / report['license']).read_bytes()).hexdigest() == report['license_sha256']
    (PARSER / report['generated']).write_text(generated)
    report.update(input=str(source.relative_to(ROOT)), generator='python3 tools/generate_type1_standard_encoding.py')
    provenance.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    generate()
