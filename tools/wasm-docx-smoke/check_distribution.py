#!/usr/bin/env python3
"""生成したDOCX WASMに、glyph表の利用条件と固定ハッシュが同梱されているか確認する。"""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
NOTICES = {
    'LICENSE-adobe-glyph-list.txt': ROOT / 'crates/jade-pdf2docx-parser/licenses/adobe-glyph-list.txt',
    'LICENSE-fonttools.txt': ROOT / 'crates/jade-pdf2docx-parser/licenses/fonttools.txt',
    'LICENSE-reportlab.txt': ROOT / 'crates/jade-pdf2docx-parser/licenses/reportlab.txt',
    'LICENSE-open-font-license-1.1.txt': ROOT / 'crates/jade-pdf2docx-parser/licenses/open-font-license-1.1.txt',
}
BINDINGS = ['jade_pdf2docx_wasm.js', 'jade_pdf2docx_wasm.d.ts',
            'jade_pdf2docx_wasm_bg.wasm', 'jade_pdf2docx_wasm_bg.wasm.d.ts']


def check(probe):
    """配布するregular fileと元noticeの完全一致を、完成build記録とも照合する。"""
    report = json.loads((probe / 'build.json').read_text())
    checked = {}
    for name in BINDINGS + list(NOTICES):
        path = probe / name
        assert path.is_file() and not path.is_symlink(), f'Missing regular distribution file: {name}'
        data = path.read_bytes()
        assert data, f'Empty distribution file: {name}'
        digest = hashlib.sha256(data).hexdigest()
        assert report['generated'].get(name) == digest, f'Distribution hash mismatch: {name}'
        if name in NOTICES:
            assert data == NOTICES[name].read_bytes(), f'License notice mismatch: {name}'
        checked[name] = digest
    return checked


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--probe', type=Path, required=True)
    print(json.dumps(dict(checked=check(parser.parse_args().probe)), indent=2))
