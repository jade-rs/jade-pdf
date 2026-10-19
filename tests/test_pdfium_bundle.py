"""固定Pdfiumのoffline配布を、改変・不完全配置・実プロセスで検証する。"""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'scripts/pdfium_bundle.py'


class PdfiumBundleTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        spec = importlib.util.spec_from_file_location('pdfium_bundle', SCRIPT)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        self.platform = self.module.host_platform()
        self.library = 'lib/libpdfium.dylib' if sys.platform == 'darwin' else 'lib/libpdfium.so'
        self.contents = {self.library: b'native library', 'LICENSE': b'upstream license',
                         'VERSION': b'7961', 'licenses/dependency.txt': b'dependency notice'}
        self.archive = self.root / 'pdfium.tgz'
        self.destination = self.root / 'bundle with spaces'

    def make_archive(self, extra=()):
        with tarfile.open(self.archive, 'w:gz') as output:
            for name, data in self.contents.items():
                member = tarfile.TarInfo(name)
                member.size = len(data)
                output.addfile(member, io.BytesIO(data))
            for member, data in extra:
                output.addfile(member, io.BytesIO(data))
        self.lock = {'schema_version': 1, 'release': 'chromium/7961', 'platforms': {
            self.platform: {
                'url': 'https://example.invalid/pdfium.tgz',
                'archive_bytes': self.archive.stat().st_size,
                'archive_sha256': hashlib.sha256(self.archive.read_bytes()).hexdigest(),
                'library': self.library,
                'files': {name: {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
                          for name, data in self.contents.items()},
            }}}

    def install(self):
        return self.module.install(self.archive, self.destination, self.lock, self.platform)

    def verify(self):
        return self.module.verify(self.destination, self.lock, self.platform)

    def test_install_preserves_all_pinned_licenses_and_verifies_without_archive(self):
        self.make_archive()
        self.assertEqual(self.install(), self.destination / self.library)
        self.archive.unlink()
        self.assertEqual(self.verify(), self.destination / self.library)
        for name, expected in self.contents.items():
            self.assertEqual((self.destination / name).read_bytes(), expected)

    def test_same_size_archive_corruption_never_creates_destination(self):
        self.make_archive()
        data = bytearray(self.archive.read_bytes())
        data[-1] ^= 1
        self.archive.write_bytes(data)
        with self.assertRaisesRegex(ValueError, 'SHA-256'):
            self.install()
        self.assertFalse(self.destination.exists())

    def test_every_pinned_file_is_checked_before_running(self):
        self.make_archive()
        self.install()
        for name, data in self.contents.items():
            with self.subTest(name=name):
                (self.destination / name).write_bytes(b'X' * len(data))
                with self.assertRaisesRegex(ValueError, 'SHA-256'):
                    self.verify()
                (self.destination / name).write_bytes(data)
        (self.destination / 'VERSION').unlink()
        with self.assertRaises((ValueError, OSError)):
            self.verify()

    def test_traversal_links_and_duplicate_members_are_rejected_even_when_archive_is_pinned(self):
        for name, kind in [('../outside', tarfile.REGTYPE), ('/absolute', tarfile.REGTYPE),
                           ('linked', tarfile.SYMTYPE), ('hard', tarfile.LNKTYPE),
                           (self.library, tarfile.REGTYPE)]:
            with self.subTest(name=name):
                member = tarfile.TarInfo(name)
                member.type = kind
                member.linkname = '../outside'
                member.size = 1 if kind == tarfile.REGTYPE else 0
                self.make_archive([(member, b'x')])
                with self.assertRaises(ValueError):
                    self.install()
                self.assertFalse(self.destination.exists())
                self.assertFalse((self.root / 'outside').exists())

    def test_existing_destination_and_symlink_remain_unchanged(self):
        self.make_archive()
        self.destination.mkdir()
        sentinel = self.destination / 'keep'
        sentinel.write_bytes(b'existing output')
        with self.assertRaises(FileExistsError):
            self.install()
        self.assertEqual(sentinel.read_bytes(), b'existing output')
        sentinel.unlink()
        self.destination.rmdir()
        self.destination.symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(FileExistsError):
            self.install()
        self.assertTrue(self.destination.is_symlink())

    def test_bundle_symlinks_and_wrong_release_are_rejected(self):
        self.make_archive()
        self.install()
        file = self.destination / 'LICENSE'
        file.unlink()
        external = self.root / 'external'
        external.write_bytes(self.contents['LICENSE'])
        file.symlink_to(external)
        with self.assertRaises(ValueError):
            self.verify()
        file.unlink()
        file.write_bytes(self.contents['LICENSE'])
        marker = self.destination / 'bundle.json'
        marker.write_text(json.dumps({'schema_version': 1, 'release': 'untrusted',
                                     'platform': self.platform}))
        with self.assertRaises(ValueError):
            self.verify()

    @unittest.skipUnless(os.name == 'posix', 'executable test helper')
    def test_launcher_passes_arguments_path_and_exit_code_and_rejects_incomplete_bundle(self):
        self.make_archive()
        self.install()
        output = self.root / 'called.json'
        binary = self.root / 'fake jade-pdf2docx'
        binary.write_text('#!' + sys.executable + '\nimport json, os, sys\n'
                          'with open(sys.argv[1], "w") as f: json.dump([os.environ["PDFIUM_LIB_PATH"], sys.argv[2:]], f)\n'
                          'sys.exit(7)\n')
        binary.chmod(0o700)
        lock_path = self.root / 'lock.json'
        lock_path.write_text(json.dumps(self.lock))
        # CLI parserとexecを実行する子だけにtest用の信頼済みlockを渡す。
        helper = ('import importlib.util, pathlib, sys\n'
                  f'spec = importlib.util.spec_from_file_location("bundle", {str(SCRIPT)!r})\n'
                  'module = importlib.util.module_from_spec(spec)\nspec.loader.exec_module(module)\n'
                  f'module.LOCK_PATH = pathlib.Path({str(lock_path)!r})\n'
                  'sys.exit(module.main(sys.argv[1:]))\n')
        command = [sys.executable, '-c', helper, 'run', '--bundle', str(self.destination),
                   '--binary', str(binary), '--', str(output), 'input with space.pdf']
        result = subprocess.run(command, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 7, result.stderr)
        library, arguments = json.loads(output.read_text())
        self.assertEqual(library, str((self.destination / self.library).resolve()))
        self.assertEqual(arguments, ['input with space.pdf'])
        output.unlink()
        (self.destination / self.library).unlink()
        result = subprocess.run(command, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn('Pdfium bundle error:', result.stderr)
        self.assertFalse(output.exists())

    def test_missing_pinned_notice_and_unexpected_notice_reject_install(self):
        self.make_archive()
        del self.lock['platforms'][self.platform]['files']['licenses/dependency.txt']
        with self.assertRaises(ValueError):
            self.install()
        self.assertFalse(self.destination.exists())
        self.make_archive()
        self.lock['platforms'][self.platform]['files']['licenses/missing.txt'] = {'bytes': 1, 'sha256': '0' * 64}
        with self.assertRaises(ValueError):
            self.install()
        self.assertFalse(self.destination.exists())

    def test_locked_release_matches_cargo_dependency_and_has_supported_targets(self):
        lock = json.loads(self.module.LOCK_PATH.read_text())
        cargo_lock = tomllib.loads((ROOT / 'Cargo.lock').read_text())
        version = next(item['version'] for item in cargo_lock['package'] if item['name'] == 'pdfium-auto')
        self.assertEqual(lock['pdfium_auto_version'], version, 'review Pdfium binary pin on dependency update')
        for target in ('linux-x86_64', 'linux-aarch64', 'macos-x86_64', 'macos-aarch64'):
            record = self.module.record_for(lock, target)
            self.assertIn('/' + lock['release'] + '/', record['url'])
            self.assertIn('licenses/pdfium.txt', record['files'])


if __name__ == '__main__':
    unittest.main()
