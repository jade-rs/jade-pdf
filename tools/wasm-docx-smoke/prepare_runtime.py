#!/usr/bin/env python3
"""Build the production PDF-to-DOCX WASM library using the pinned wasm-bindgen tool."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
CACHE = ROOT / 'corpus/wasm-runtime'


def verified_bindgen():
    """既存smoke準備処理が検証したarchiveと、使用する実行体の内容を照合する。"""
    manifest = json.loads((ROOT / 'tools/wasm-smoke/runtime.json').read_text())
    machine = {'arm64': 'aarch64', 'aarch64': 'aarch64', 'x86_64': 'x86_64', 'AMD64': 'x86_64'}[platform.machine()]
    suffix = {'Darwin': 'apple-darwin', 'Linux': 'unknown-linux-musl', 'Windows': 'pc-windows-msvc'}[platform.system()]
    entry = manifest['bindgen']['platforms'][machine + '-' + suffix]
    archive = CACHE / entry['filename']
    if not archive.is_file():
        raise FileNotFoundError('Run python3 tools/wasm-smoke/prepare_runtime.py first')
    if hashlib.sha256(archive.read_bytes()).hexdigest() != entry['sha256']:
        raise ValueError('Pinned wasm-bindgen archive checksum mismatch')
    prefix = entry['filename'].removesuffix('.tar.gz')
    name = 'wasm-bindgen.exe' if platform.system() == 'Windows' else 'wasm-bindgen'
    binary = CACHE / prefix / name
    with tarfile.open(archive) as source:
        member = source.getmember(prefix + '/' + name)
        if not member.isfile():
            raise ValueError('Pinned wasm-bindgen archive member is not a file')
        with source.extractfile(member) as stream:
            expected = hashlib.sha256(stream.read()).digest()
    if hashlib.sha256(binary.read_bytes()).digest() != expected:
        raise ValueError('Extracted wasm-bindgen executable checksum mismatch; prepare the runtime again')
    return binary


def main():
    """Cargo通知時にWASMを退避し、キャッシュ回収後も確実にglueを生成する。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=CACHE / 'docx-probe')
    parser.add_argument('--target-dir', type=Path, default=Path(os.environ.get('CARGO_TARGET_DIR', str(ROOT / 'target'))))
    args = parser.parse_args()
    bindgen = verified_bindgen()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    command = ['cargo', 'build', '-p', 'jade-pdf2docx-wasm', '--lib', '--locked', '--release',
               '--target', 'wasm32-unknown-unknown', '--target-dir', str(args.target_dir.resolve()),
               '--message-format=json']
    with tempfile.TemporaryDirectory() as temporary:
        frozen = Path(temporary) / 'jade_pdf2docx_wasm.wasm'
        process = subprocess.Popen(command, cwd=ROOT, stdout=subprocess.PIPE, text=True)
        for line in process.stdout:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                print(line, end='', flush=True)
                continue
            if event.get('reason') == 'compiler-message':
                print(event.get('message', {}).get('rendered', ''), end='', file=sys.stderr)
            if event.get('reason') == 'compiler-artifact' and event.get('target', {}).get('name') == 'jade_pdf2docx_wasm':
                for name in event.get('filenames', []):
                    if name.endswith('.wasm'):
                        shutil.copyfile(name, frozen)
        code = process.wait()
        if code:
            return code
        if not frozen.is_file():
            raise FileNotFoundError('Cargo did not report the production DOCX WASM artifact')
        raw_hash = hashlib.sha256(frozen.read_bytes()).hexdigest()
        subprocess.run([str(bindgen), '--target', 'web', '--out-name', 'jade_pdf2docx_wasm',
                        '--out-dir', str(output), str(frozen)], check=True, cwd=ROOT)
    report = dict(command=command, raw_wasm_sha256=raw_hash,
                  generated={p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                             for p in sorted(output.iterdir()) if p.is_file() and p.name != 'build.json'})
    (output / 'build.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(output=str(output), raw_wasm_sha256=raw_hash)))
    return 0


if __name__ == '__main__':
    sys.exit(main())
