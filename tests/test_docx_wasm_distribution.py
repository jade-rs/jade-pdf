"""DOCX WASM配布物で、標準glyph名表のnotice欠落・改変を検出する。"""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class DocxWasmDistributionTest(unittest.TestCase):
    """実WASMの精度検証とは別に、配布checkerのnotice契約を所有byteで検査する。"""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.probe = Path(self.temporary.name)
        spec = importlib.util.spec_from_file_location(
            'docx_check_distribution', ROOT / 'tools/wasm-docx-smoke/check_distribution.py')
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        for name in self.module.BINDINGS:
            (self.probe / name).write_bytes(b'owned distribution contract test')
        for source, target in [
            ('adobe-glyph-list.txt', 'LICENSE-adobe-glyph-list.txt'),
            ('fonttools.txt', 'LICENSE-fonttools.txt'),
            ('open-font-license-1.1.txt', 'LICENSE-open-font-license-1.1.txt'),
        ]:
            (self.probe / target).write_bytes(
                (ROOT / 'crates/jade-pdf2docx-parser/licenses' / source).read_bytes())
        (self.probe / 'LICENSE-ttf-parser-0.25.1.txt').write_bytes(
            (ROOT / 'crates/jade-pdf2docx-docx/licenses/ttf-parser-0.25.1.txt').read_bytes())
        self.record_hashes()

    def record_hashes(self):
        """テスト用配布物の全byteを記録し、notice内容の検査とhash照合を区別する。"""
        (self.probe / 'build.json').write_text(json.dumps({
            'generated': {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                          for p in self.probe.iterdir() if p.name != 'build.json'}}))

    def test_missing_reportlab_notice_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, 'LICENSE-reportlab.txt'):
            self.module.check(self.probe)

    def test_missing_open_font_license_is_rejected(self):
        notice = (ROOT / 'crates/jade-pdf2docx-parser/licenses/reportlab.txt').read_bytes()
        (self.probe / 'LICENSE-reportlab.txt').write_bytes(notice)
        (self.probe / 'LICENSE-open-font-license-1.1.txt').unlink()
        self.record_hashes()
        with self.assertRaisesRegex(AssertionError, 'LICENSE-open-font-license-1.1.txt'):
            self.module.check(self.probe)

    def test_complete_reportlab_notice_is_checked(self):
        notice = (ROOT / 'crates/jade-pdf2docx-parser/licenses/reportlab.txt').read_bytes()
        (self.probe / 'LICENSE-reportlab.txt').write_bytes(notice)
        self.record_hashes()
        checked = self.module.check(self.probe)
        self.assertEqual(checked['LICENSE-reportlab.txt'], hashlib.sha256(notice).hexdigest())

    def test_updated_manifest_does_not_hide_modified_reportlab_notice(self):
        (self.probe / 'LICENSE-reportlab.txt').write_bytes(b'changed license')
        self.record_hashes()
        with self.assertRaisesRegex(AssertionError, 'License notice mismatch: LICENSE-reportlab.txt'):
            self.module.check(self.probe)

    def test_missing_font_reader_notice_is_rejected(self):
        (self.probe / 'LICENSE-reportlab.txt').write_bytes(
            (ROOT / 'crates/jade-pdf2docx-parser/licenses/reportlab.txt').read_bytes())
        (self.probe / 'LICENSE-ttf-parser-0.25.1.txt').unlink()
        self.record_hashes()
        with self.assertRaisesRegex(AssertionError, 'LICENSE-ttf-parser-0.25.1.txt'):
            self.module.check(self.probe)

    def test_updated_manifest_does_not_hide_modified_font_reader_notice(self):
        (self.probe / 'LICENSE-reportlab.txt').write_bytes(
            (ROOT / 'crates/jade-pdf2docx-parser/licenses/reportlab.txt').read_bytes())
        (self.probe / 'LICENSE-ttf-parser-0.25.1.txt').write_bytes(b'changed license')
        self.record_hashes()
        with self.assertRaisesRegex(AssertionError, 'License notice mismatch: LICENSE-ttf-parser-0.25.1.txt'):
            self.module.check(self.probe)


if __name__ == '__main__':
    unittest.main()
