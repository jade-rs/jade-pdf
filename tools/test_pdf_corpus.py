import hashlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from pdf_corpus import fetch


class FetchTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.data = b'%PDF-1.7\ntest corpus download\n'
        self.entry = dict(id='fixture', url='https://example.invalid/fixture.pdf',
                          bytes=len(self.data), sha256=hashlib.sha256(self.data).hexdigest())

    def test_offline_requires_present_verified_bytes(self):
        with self.assertRaises(FileNotFoundError):
            fetch(self.entry, self.root, offline=True)
        (self.root / 'fixture.pdf').write_bytes(self.data)
        self.assertIn('verified', fetch(self.entry, self.root, offline=True))

    def test_corrupt_cache_is_rejected_without_redownloading(self):
        (self.root / 'fixture.pdf').write_bytes(self.data.replace(b'test', b'fail'))
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            fetch(self.entry, self.root)

    def test_unexpected_download_is_not_installed(self):
        with patch('urllib.request.urlopen', return_value=io.BytesIO(b'<html>not a PDF</html>')):
            with self.assertRaisesRegex(ValueError, 'unexpected PDF'):
                fetch(self.entry, self.root)
        self.assertEqual(list(self.root.iterdir()), [])

    def test_valid_download_is_installed_atomically(self):
        with patch('urllib.request.urlopen', return_value=io.BytesIO(self.data)):
            fetch(self.entry, self.root)
        self.assertEqual((self.root / 'fixture.pdf').read_bytes(), self.data)
        self.assertFalse((self.root / 'fixture.pdf.part').exists())

class SourceManifestTests(unittest.TestCase):
    def test_pending_semantic_failures_are_fetched_without_being_normal_success_cases(self):
        from pdf_corpus import sources
        manifest = dict(documents=[dict(id='normal')], robustness=[dict(id='robust')],
                        references=[dict(id='reference')], pending_semantic=[dict(id='pending')])
        self.assertEqual([entry['id'] for entry in sources(manifest)], ['normal','robust','reference','pending'])
        self.assertEqual(len(manifest['documents']), 1)

    def test_duplicate_ids_across_semantic_groups_are_rejected(self):
        from pdf_corpus import sources
        with self.assertRaisesRegex(ValueError, 'Duplicate corpus ID'):
            sources(dict(documents=[dict(id='same')], pending_semantic=[dict(id='same')]))


if __name__ == '__main__':
    unittest.main()
