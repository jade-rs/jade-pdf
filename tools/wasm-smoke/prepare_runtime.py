#!/usr/bin/env python3
"""Fetch checksum-pinned test runtimes and optionally build the WASM probe."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
CACHE = ROOT / 'corpus/wasm-runtime'


def download(entry, path):
    if not path.exists():
        with urllib.request.urlopen(entry['url'], timeout=60) as response:
            data = response.read(64 * 1024 * 1024 + 1)
        if len(data) > 64 * 1024 * 1024:
            raise ValueError('Runtime archive exceeds 64 MiB')
        if hashlib.sha256(data).hexdigest() != entry['sha256']:
            raise ValueError('Runtime download SHA-256 mismatch')
        path.write_bytes(data)
    if hashlib.sha256(path.read_bytes()).hexdigest() != entry['sha256']:
        raise ValueError(f'{path}: corrupt runtime cache')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build', action='store_true')
    args = parser.parse_args()
    manifest = json.loads(Path(__file__).with_name('runtime.json').read_text())
    CACHE.mkdir(parents=True, exist_ok=True)
    archive = CACHE / ('pdfium-' + manifest['pdfium']['version'] + '.tgz')
    download(manifest['pdfium'], archive)
    runtime = CACHE / 'release/node'
    runtime.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive) as source:
        for name, expected in manifest['pdfium']['members'].items():
            member = source.getmember(name)
            if not member.isfile() or member.size != expected['bytes']:
                raise ValueError('Invalid Pdfium WASM archive member')
            with source.extractfile(member) as stream:
                data = stream.read(expected['bytes'] + 1)
            if len(data) != expected['bytes'] or hashlib.sha256(data).hexdigest() != expected['sha256']:
                raise ValueError('Pdfium WASM member checksum mismatch')
            if name.endswith('.js'):
                source_js = data.decode('utf-8')
                # Fault-injection loader: same verified SDK, without the
                # character-box adapter. Never used as the normal loader.
                (runtime / 'pdfium-unprepared.esm.js').write_text(
                    wrap_pdfium(source_js, prepared=False))
                data = wrap_pdfium(source_js).encode('utf-8')
                name = 'pdfium.esm.js'
            else:
                name = 'pdfium.wasm'
            (runtime / name).write_bytes(data)
    (runtime / 'pdfium-LICENSE-NOTICES.txt').write_bytes(
        (ROOT / 'crates/jade-pdf-diff/licenses/pdfium-8111.txt').read_bytes())
    (runtime / 'jade_pdfium_adapter.mjs').write_bytes(
        (ROOT / 'crates/jade-pdf-diff/wasm/pdfium_adapter.mjs').read_bytes())
    machine = {'arm64':'aarch64', 'aarch64':'aarch64', 'x86_64':'x86_64', 'AMD64':'x86_64'}.get(platform.machine())
    suffix = {'Darwin':'apple-darwin', 'Linux':'unknown-linux-musl', 'Windows':'pc-windows-msvc'}.get(platform.system())
    triple = f'{machine}-{suffix}'
    if triple not in manifest['bindgen']['platforms']:
        raise ValueError(f'Unsupported test runner platform: {triple}')
    entry = manifest['bindgen']['platforms'][triple]
    archive = CACHE / entry['filename']
    download(entry, archive)
    prefix = entry['filename'].removesuffix('.tar.gz')
    executable = 'wasm-bindgen.exe' if platform.system() == 'Windows' else 'wasm-bindgen'
    bindgen = CACHE / prefix / executable
    bindgen.parent.mkdir(exist_ok=True)
    # Extract only the known executable, never arbitrary archive paths or links.
    with tarfile.open(archive) as source:
        member = source.getmember(prefix + '/' + executable)
        if not member.isfile():
            raise ValueError('wasm-bindgen archive entry is not a file')
        with source.extractfile(member) as stream:
            bindgen.write_bytes(stream.read())
    bindgen.chmod(0o755)
    print('Verified Pdfium and wasm-bindgen archives', flush=True)
    if args.build:
        target = Path(os.environ.get('CARGO_TARGET_DIR', str(ROOT / 'target'))).resolve()
        subprocess.run(['cargo', 'build', '--manifest-path', str(Path(__file__).with_name('Cargo.toml')),
                        '--locked', '--release', '--target', 'wasm32-unknown-unknown',
                        '--target-dir', str(target)], check=True, cwd=ROOT)
        subprocess.run([str(bindgen), '--target', 'web', '--out-dir', str(CACHE / 'probe'),
                        str(target / 'wasm32-unknown-unknown/release/jade_pdf_wasm_smoke.wasm')], check=True)


def wrap_pdfium(source, prepared=True):
    """Adapt the pinned classic Emscripten build to an isolated browser factory.

    Keep the upstream SDK source intact. Expose its table and initialization name
    expected by pdfium-render 0.8, without globals or runtime eval/CSP exceptions.
    """
    loader = '''import {preparePdfiumForJade} from './jade_pdfium_adapter.mjs';
export default function createPdfium(options = {}) {
  return new Promise((resolve, reject) => {
    var Module = {...options};
    Module.locateFile ??= name => new URL(name, import.meta.url).href;
    Module.onRuntimeInitialized = () => {
      Module._PDFium_Init = Module._FPDF_InitLibrary;
      Module.wasmExports = wasmExports;
      try { preparePdfiumForJade(Module); options.onRuntimeInitialized?.(); resolve(Module); }
      catch (error) { reject(error); }
    };
    Module.onAbort = reason => {
      try { options.onAbort?.(reason); }
      finally { reject(new Error(String(reason))); }
    };
''' + source + '\n  });\n}\n'
    if not prepared:
        loader = loader.replace('preparePdfiumForJade(Module); ', '')
    return loader


if __name__ == '__main__':
    main()
