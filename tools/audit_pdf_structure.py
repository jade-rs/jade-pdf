#!/usr/bin/env python3
"""Compare pinned corpus page metadata with an independent pypdf parser.

This checks page count, visible boxes, rotation, raw user-space dimensions and
UserUnit-derived physical dimensions. It does not check text or painting.
Use only the hash-pinned public corpus; this is an offline evaluation tool, not
an untrusted-input parser in the library's runtime.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from pypdf import PdfReader, __version__ as pypdf_version

from pdf_corpus import ROOT, fetch


def expected_page(page):
    media = list(map(float, page.mediabox))
    crop = list(map(float, page.cropbox))
    x0 = max(min(media[0], media[2]), min(crop[0], crop[2]))
    y0 = max(min(media[1], media[3]), min(crop[1], crop[3]))
    x1 = min(max(media[0], media[2]), max(crop[0], crop[2]))
    y1 = min(max(media[1], media[3]), max(crop[1], crop[3]))
    assert x1 > x0 and y1 > y0, 'Invalid independent page geometry'
    rotation = page.rotation % 360
    assert rotation in [0, 90, 180, 270]
    width, height = x1 - x0, y1 - y0
    if rotation in [90, 270]:
        width, height = height, width
    return dict(width=width, height=height, rotation=rotation,
                view_box=dict(x0=x0, y0=y0, x1=x1, y1=y1),
                user_unit=float(page.get('/UserUnit', 1)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/pdf-diff')
    parser.add_argument('--report', type=Path, default=ROOT / 'corpus/results-structure.json')
    args = parser.parse_args()
    manifest_bytes = (ROOT / 'corpus/manifest.json').read_bytes()
    manifest = json.loads(manifest_bytes)
    results = []
    with tempfile.TemporaryDirectory(prefix='jade-pdf-structure-') as directory:
        binary = Path(directory) / 'pdf-diff'
        shutil.copy2(args.binary, binary)
        binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
        for entry in manifest['documents']:
            result = dict(id=entry['id'], passed=False)
            try:
                fetch(entry, ROOT / 'corpus/cache', offline=True)
                source = ROOT / 'corpus/cache' / (entry['id'] + '.pdf')
                independent = [expected_page(page) for page in PdfReader(source).pages]
                process = subprocess.run([str(binary), '--ir', str(source)],
                                         capture_output=True, text=True, timeout=60, check=True)
                native = json.loads(process.stdout)['pages']
                assert len(native) == len(independent), 'Page count differs'
                errors = []
                for index, (expected, actual) in enumerate(zip(independent, native), 1):
                    assert actual['index'] == index, 'Non-contiguous native page index'
                    assert actual['rotation'] == expected['rotation'], f'Rotation differs on page {index}'
                    assert actual['user_unit'] is not None, f'UserUnit unavailable on page {index}'
                    assert abs(actual['user_unit']-expected['user_unit']) <= .000001, f'UserUnit differs on page {index}'
                    for key in ['width', 'height']:
                        error = abs(actual[key] - expected[key])
                        errors.append(error)
                        assert error <= .001, f'{key} differs on page {index}: {error}'
                        physical_error=abs(actual[key]*actual['user_unit']-expected[key]*expected['user_unit'])
                        assert physical_error <= .001, f'Physical {key} differs on page {index}: {physical_error}'
                    for key in ['x0', 'y0', 'x1', 'y1']:
                        error = abs(actual['view_box'][key] - expected['view_box'][key])
                        errors.append(error)
                        assert error <= .001, f'view_box.{key} differs on page {index}: {error}'
                result.update(passed=True, pages=len(independent), max_coordinate_error=max(errors),
                              nondefault_user_units=[dict(page=i, user_unit=p['user_unit'])
                                                     for i, p in enumerate(independent, 1)
                                                     if p['user_unit'] != 1])
            except Exception as error:
                result['error'] = str(error)
            results.append(result)
            print(('PASS ' if result['passed'] else 'FAIL ') + entry['id'], flush=True)
    passed = sum(result['passed'] for result in results)
    report = dict(version=1, manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
                  binary_sha256=binary_hash, independent_parser='pypdf ' + pypdf_version,
                  methodology=__doc__.strip(), documents=len(results), passed=passed,
                  pages=sum(r.get('pages', 0) for r in results), results=results)
    args.report.write_text(json.dumps(report, indent=2) + '\n')
    print(f'{passed}/{len(results)} passed; report: {args.report}')
    return passed != len(results)


if __name__ == '__main__':
    raise SystemExit(main())
