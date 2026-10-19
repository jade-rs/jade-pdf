#!/usr/bin/env python3
"""固定hashのPdfium archiveをoffline配置・検査し、native CLIへ引き渡す。"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import shutil
import stat
import sys
import tarfile
import tempfile

LOCK_PATH = Path(__file__).with_name('pdfium_lock.json')


def host_platform():
    systems = {'Linux': 'linux', 'Darwin': 'macos'}
    machines = {'x86_64': 'x86_64', 'amd64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}
    try:
        return systems[platform.system()] + '-' + machines[platform.machine().lower()]
    except KeyError as error:
        raise ValueError('offline Pdfium supports Linux/macOS x86_64/aarch64') from error


def safe_path(name):
    if not isinstance(name, str) or not name or '\\' in name or any(
        part in ('', '.', '..') for part in name.split('/')
    ) or PurePosixPath(name).is_absolute():
        raise ValueError(f'unsafe archive path: {name!r}')
    return name


def record_for(lock, target):
    if lock.get('schema_version') != 1:
        raise ValueError('unsupported Pdfium lock schema')
    record = lock['platforms'][target]
    files = record['files']
    if not {'LICENSE', 'VERSION', record['library']} <= files.keys() or not any(
        name.startswith('licenses/') for name in files
    ):
        raise ValueError('lock must pin library, VERSION and all upstream licenses')
    for name, expected in files.items():
        safe_path(name)
        check_expected(expected['bytes'], expected['sha256'])
    check_expected(record['archive_bytes'], record['archive_sha256'])
    return record


def check_expected(size, digest):
    if type(size) is not int or size <= 0 or len(digest) != 64 or any(
        char not in '0123456789abcdef' for char in digest
    ):
        raise ValueError('invalid pinned byte size or SHA-256')


def check_file(path, expected):
    if not stat.S_ISREG(path.lstat().st_mode):
        raise ValueError(f'expected regular file without symlink: {path}')
    if path.stat().st_size != expected['bytes']:
        raise ValueError(f'byte size mismatch: {path}')
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(chunk)
    if digest.hexdigest() != expected['sha256']:
        raise ValueError(f'SHA-256 mismatch: {path}')


def verify(bundle, lock, target):
    record = record_for(lock, target)
    bundle = Path(bundle)
    if not stat.S_ISDIR(bundle.lstat().st_mode):
        raise ValueError('bundle must be a directory without symlink')
    marker = bundle / 'bundle.json'
    if not stat.S_ISREG(marker.lstat().st_mode) or marker.stat().st_size > 4096:
        raise ValueError('invalid bundle marker')
    if json.loads(marker.read_text()) != {
        'schema_version': 1, 'release': lock['release'], 'platform': target
    }:
        raise ValueError('bundle release/platform mismatch')
    for name, expected in record['files'].items():
        path = bundle / name
        for parent in path.parents:
            if parent == bundle:
                break
            if not stat.S_ISDIR(parent.lstat().st_mode):
                raise ValueError(f'bundle directory must not be a symlink: {parent}')
        check_file(path, expected)
    return bundle / record['library']


def install(archive, destination, lock, target):
    record = record_for(lock, target)
    archive, destination = Path(archive), Path(destination)
    if os.path.lexists(destination):
        raise FileExistsError(f'destination already exists: {destination}')
    check_file(archive, {'bytes': record['archive_bytes'], 'sha256': record['archive_sha256']})
    # trusted配布先の同一filesystem上で検査を完了し、既存名を置換せずに配置する。
    with tempfile.TemporaryDirectory(prefix='.jade-pdf2docx-pdfium-', dir=destination.parent) as temporary:
        staging = Path(temporary)
        found, seen = set(), set()
        with tarfile.open(archive, mode='r|gz') as source:
            for member in source:
                name = safe_path(member.name.rstrip('/') if member.isdir() else member.name)
                if name in seen or not (member.isdir() or member.isfile()):
                    raise ValueError(f'duplicate or non-regular archive member: {name}')
                seen.add(name)
                if member.isdir():
                    continue
                is_notice = name in ('LICENSE', 'VERSION') or name.startswith('licenses/')
                if name not in record['files']:
                    if is_notice:
                        raise ValueError(f'unpinned upstream notice: {name}')
                    continue
                expected = record['files'][name]
                if member.size != expected['bytes']:
                    raise ValueError(f'archive member byte size mismatch: {name}')
                path = staging / name
                path.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(member) as input_file, path.open('xb') as output:
                    shutil.copyfileobj(input_file, output, length=1024 * 1024)
                check_file(path, expected)
                found.add(name)
        if found != record['files'].keys():
            raise ValueError(f'missing pinned files: {sorted(record["files"].keys() - found)}')
        (staging / 'bundle.json').write_text(json.dumps({
            'schema_version': 1, 'release': lock['release'], 'platform': target,
        }) + '\n')
        verify(staging, lock, target)
        # mkdirは排他的。kill途中の不完全配置は次回verify/runで拒否される。
        destination.mkdir(mode=0o700)
        try:
            for path in staging.iterdir():
                path.rename(destination / path.name)
        except BaseException:
            shutil.rmtree(destination)
            raise
    return verify(destination, lock, target)


def run(bundle, binary, arguments, lock, target):
    if target != host_platform():
        raise ValueError('bundle platform must match the execution host')
    library = verify(bundle, lock, target).resolve()
    binary = str(Path(binary).resolve(strict=True))
    environment = os.environ.copy()
    environment['PDFIUM_LIB_PATH'] = str(library)
    # execによりCLIのsignal、終了コード、supervisorのprocess境界を維持する。
    return os.execve(binary, [binary, *arguments], environment)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    setup = commands.add_parser('install', help='verify a local pinned archive and install into a new directory')
    setup.add_argument('--archive', type=Path, required=True)
    setup.add_argument('--destination', type=Path, required=True)
    setup.add_argument('--platform', choices=['linux-x86_64', 'linux-aarch64', 'macos-x86_64', 'macos-aarch64'])
    for name in ('verify', 'run'):
        command = commands.add_parser(name)
        command.add_argument('--bundle', type=Path, required=True)
        if name == 'run':
            command.add_argument('--binary', type=Path, required=True)
            command.add_argument('arguments', nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    try:
        lock = json.loads(LOCK_PATH.read_text())
        target = getattr(args, 'platform', None) or host_platform()
        if args.command == 'install':
            print(install(args.archive, args.destination, lock, target))
        elif args.command == 'verify':
            print(verify(args.bundle, lock, target))
        else:
            arguments = args.arguments[1:] if args.arguments[:1] == ['--'] else args.arguments
            return run(args.bundle, args.binary, arguments, lock, target)
        return 0
    except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
        print(f'Pdfium bundle error: {error}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
