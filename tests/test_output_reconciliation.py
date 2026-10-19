"""公開済みの照合情報を読み、終了記録や元の出力を変更しない。"""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/reconcile_jade_pdf2docx_output.py'


class OutputReconciliationTest(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location('output_reconciliation', SCRIPT)
        self.module = importlib.util.module_from_spec(spec)
        with patch.object(sys, 'path', [str(SCRIPT.parent), *sys.path]):
            spec.loader.exec_module(self.module)
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name).resolve()
        self.store = self.base / 'history'
        self.store.mkdir(mode=0o700)
        self.token, self.digest = 'a' * 32, 'b' * 64
        self.unit = 'jade-pdf2docx-test-slot-1.service'
        with self.module.history.store_directory(self.store) as fd:
            self.module.history.write_record(fd, self.token + '.request.json', dict(
                schema=2, token=self.token, request_sha256=self.digest, manager='--user', service=self.unit))
        self.output = self.base / 'out.docx'

    def artifact(self, receipt=None):
        with zipfile.ZipFile(self.output, 'w') as archive:
            archive.writestr('word/document.xml', '<document>original</document>')
            if receipt is not None:
                archive.comment = receipt.encode()

    def receipt(self, token=None, digest=None):
        return 'JADE-PDF2DOCX-PUBLICATION-1\n' + (token or self.token) + ':' + (digest or self.digest)

    def inspect(self):
        return self.module.reconcile(self.store, self.token, self.output)

    def snapshot(self):
        return {p.name: p.read_bytes() for p in self.store.iterdir()}

    def test_missing_completion_and_matching_publication_are_distinct(self):
        self.artifact(self.receipt())
        before, output = self.snapshot(), self.output.read_bytes()
        with patch.object(subprocess, 'run', side_effect=AssertionError('must not launch')):
            report = self.inspect()
        self.assertEqual(report['state'], 'unknown')
        self.assertEqual(report['output'], 'unknown')
        self.assertEqual(report['publication'], 'matched')
        self.assertEqual(report['request_sha256'], self.digest)
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(self.output.read_bytes(), output)

    def test_signal_failure_is_preserved_with_matching_publication(self):
        self.artifact(self.receipt())
        self.module.history.finish(self.store, self.token, self.unit,
                                  dict(SERVICE_RESULT='signal', EXIT_CODE='killed', EXIT_STATUS='KILL'))
        report = self.inspect()
        self.assertEqual((report['state'], report['publication']), ('failed', 'matched'))
        self.assertEqual(report['exit_status'], 'KILL')

    def test_old_or_other_request_artifacts_do_not_match(self):
        for receipt in (None, self.receipt(token='c'*32), self.receipt(digest='d'*64)):
            with self.subTest(receipt=receipt):
                self.artifact(receipt)
                self.assertEqual(self.inspect()['publication'], 'unmatched')
        self.output.unlink()
        self.assertEqual(self.inspect()['publication'], 'absent')

    def test_links_fifo_and_directory_are_rejected(self):
        outside = self.base / 'foreign'
        outside.write_bytes(b'keep')
        self.output.symlink_to(outside)
        with self.assertRaises(OSError): self.inspect()
        self.output.unlink()
        os.mkfifo(self.output)
        with self.assertRaises(ValueError): self.inspect()
        self.output.unlink()
        self.output.mkdir()
        with self.assertRaises(ValueError): self.inspect()
        self.assertEqual(outside.read_bytes(), b'keep')

    def test_invalid_archive_and_trailing_bytes_are_rejected(self):
        for content in (b'old output', b'PK\x05\x06' + b'\0'*18, b'PK'*100):
            self.output.write_bytes(content)
            with self.assertRaises(ValueError): self.inspect()
        self.artifact(self.receipt())
        with self.output.open('ab') as stream: stream.write(b'partial')
        with self.assertRaises(ValueError): self.inspect()

    def test_schema_one_has_no_request_fingerprint(self):
        (self.store / (self.token + '.request.json')).unlink()
        self.module.history.prepare(self.store, self.token, '--user', self.unit)
        self.artifact(self.receipt())
        with self.assertRaisesRegex(ValueError, 'schema 2'): self.inspect()

    def test_eocd_signature_inside_comment_does_not_hide_real_footer(self):
        self.artifact('other PK\x05\x06' + '\0'*20)
        self.assertEqual(self.inspect()['publication'], 'unmatched')

    def test_output_replacement_during_read_is_not_reported_as_matching(self):
        self.artifact(self.receipt())
        replacement = self.base / 'replacement.docx'
        replacement.write_bytes(b'foreign replacement')
        original_fstat = os.fstat
        replaced = False
        def change_path(fd):
            nonlocal replaced
            result = original_fstat(fd)
            if not replaced and result.st_ino == self.output.stat().st_ino:
                replacement.replace(self.output)
                replaced = True
            return result
        with patch.object(os, 'fstat', side_effect=change_path):
            with self.assertRaisesRegex(ValueError, 'output changed'):
                self.inspect()
        self.assertTrue(replaced)
        self.assertEqual(self.output.read_bytes(), b'foreign replacement')

    def test_parent_replacement_during_read_is_not_reported_as_matching(self):
        parent = self.base / 'destination'
        parent.mkdir()
        self.output = parent / 'out.docx'
        self.artifact(self.receipt())
        original_fstat = os.fstat
        replaced = False
        def change_parent(fd):
            nonlocal replaced
            result = original_fstat(fd)
            if not replaced and result.st_ino == self.output.stat().st_ino:
                parent.rename(self.base / 'detached')
                parent.mkdir()
                self.output.write_bytes(b'foreign destination')
                replaced = True
            return result
        with patch.object(os, 'fstat', side_effect=change_parent):
            with self.assertRaisesRegex(ValueError, 'output changed'):
                self.inspect()
        self.assertTrue(replaced)
        self.assertEqual(self.output.read_bytes(), b'foreign destination')
