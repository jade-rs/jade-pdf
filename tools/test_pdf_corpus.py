import hashlib
import io
from pathlib import Path
import tempfile
import unittest
import urllib.error
import ssl
from unittest.mock import MagicMock, patch

from pdf_corpus import MAX_BYTES, fetch


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

    def test_connect_timeout_retries_with_original_request_bounds(self):
        error = urllib.error.URLError(TimeoutError('_ssl.c:1129: The handshake operation timed out'))
        with patch('urllib.request.urlopen', side_effect=[error, io.BytesIO(self.data)]) as opening:
            fetch(self.entry, self.root)
        self.assertEqual(opening.call_count, 2)
        for call in opening.call_args_list:
            self.assertEqual(call.args[0].full_url, self.entry['url'])
            self.assertEqual(call.args[0].get_header('User-agent'), 'jade-pdf-corpus/1')
            self.assertEqual(call.kwargs, dict(timeout=45))
        self.assertEqual((self.root / 'fixture.pdf').read_bytes(), self.data)
        self.assertFalse((self.root / 'fixture.pdf.part').exists())

    def test_read_timeout_closes_response_and_restarts_download(self):
        response = MagicMock()
        response.__enter__.return_value = response
        response.read.side_effect = TimeoutError('The read operation timed out')
        with patch('urllib.request.urlopen', side_effect=[response, io.BytesIO(self.data)]) as opening:
            fetch(self.entry, self.root)
        response.read.assert_called_once_with(MAX_BYTES + 1)
        response.__exit__.assert_called_once()
        self.assertEqual(opening.call_count, 2)
        self.assertEqual((self.root / 'fixture.pdf').read_bytes(), self.data)
        self.assertFalse((self.root / 'fixture.pdf.part').exists())

    def test_timeout_retry_is_bounded_and_preserves_final_error(self):
        error = urllib.error.URLError(TimeoutError('handshake timed out'))
        with patch('urllib.request.urlopen', side_effect=error) as opening:
            with self.assertRaises(urllib.error.URLError) as raised:
                fetch(self.entry, self.root)
        self.assertIs(raised.exception, error)
        self.assertEqual(opening.call_count, 2)
        self.assertEqual(list(self.root.iterdir()), [])

    def test_permanent_http_and_certificate_errors_are_not_retried(self):
        errors = [
            urllib.error.HTTPError(self.entry['url'], 404, 'Not found', {}, None),
            urllib.error.HTTPError(self.entry['url'], 403, 'Forbidden', {}, None),
            urllib.error.URLError(ssl.SSLCertVerificationError('certificate verification failed')),
        ]
        for error in errors:
            with self.subTest(error=error), patch('urllib.request.urlopen', side_effect=error) as opening:
                with self.assertRaises(urllib.error.URLError) as raised:
                    fetch(self.entry, self.root)
                self.assertIs(raised.exception, error)
                self.assertEqual(opening.call_count, 1)
                self.assertEqual(list(self.root.iterdir()), [])

    def test_http_503_retries_once_and_installs_verified_bytes(self):
        body = io.BytesIO(b'Service unavailable')
        error = urllib.error.HTTPError(self.entry['url'], 503, 'Unavailable', {}, body)
        with patch('urllib.request.urlopen', side_effect=[error, io.BytesIO(self.data)]) as opening, \
                patch('pdf_corpus.time.sleep') as delay:
            fetch(self.entry, self.root)
        self.assertEqual(opening.call_count, 2)
        delay.assert_called_once_with(1)
        self.assertTrue(body.closed)
        self.assertEqual((self.root / 'fixture.pdf').read_bytes(), self.data)

    def test_repeated_http_503_reports_the_final_error(self):
        first = urllib.error.HTTPError(self.entry['url'], 503, 'Initial failure', {}, None)
        final = urllib.error.HTTPError(self.entry['url'], 503, 'Final failure', {}, None)
        with patch('urllib.request.urlopen', side_effect=[first, final]) as opening, \
                patch('pdf_corpus.time.sleep'):
            with self.assertRaises(urllib.error.HTTPError) as raised:
                fetch(self.entry, self.root)
        self.assertIs(raised.exception, final)
        self.assertEqual(opening.call_count, 2)
        self.assertEqual(list(self.root.iterdir()), [])

    def test_changed_source_after_http_503_is_not_installed(self):
        error = urllib.error.HTTPError(self.entry['url'], 503, 'Unavailable', {}, None)
        changed = self.data.replace(b'test', b'fail')
        with patch('urllib.request.urlopen', side_effect=[error, io.BytesIO(changed)]), \
                patch('pdf_corpus.time.sleep'):
            with self.assertRaisesRegex(ValueError, 'unexpected PDF'):
                fetch(self.entry, self.root)
        self.assertEqual(list(self.root.iterdir()), [])

    def test_changed_source_after_timeout_is_not_installed(self):
        error = urllib.error.URLError(TimeoutError('handshake timed out'))
        changed = self.data.replace(b'test', b'fail')
        with patch('urllib.request.urlopen', side_effect=[error, io.BytesIO(changed)]) as opening:
            with self.assertRaisesRegex(ValueError, 'unexpected PDF'):
                fetch(self.entry, self.root)
        self.assertEqual(opening.call_count, 2)
        self.assertEqual(list(self.root.iterdir()), [])

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
