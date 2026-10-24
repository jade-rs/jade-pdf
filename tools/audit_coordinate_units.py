#!/usr/bin/env python3
"""Rebase first-page coordinates while retaining physical size on pinned PDFs.

pypdf adds a uniform content/page/annotation scale and the inverse UserUnit.
This checks text/order/geometry/SDK-diagnostic covariance and the fixed-width
renderer, not arbitrary PDF, XFA, JavaScript or form-appearance equivalence.
The reference writer handles only SHA-pinned corpus data in this offline tool.
"""
import argparse
from collections import Counter
import copy
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unicodedata

from pypdf import PdfReader, PdfWriter, __version__ as pypdf_version
from pypdf.generic import ArrayObject, DictionaryObject, FloatObject, NameObject

from pdf_corpus import ROOT, fetch


def compact(text):
    return ''.join(unicodedata.normalize('NFC', text).split())


def layer(binary, path):
    result = subprocess.run([str(binary), '--text-layer', str(path), '1'],
                            capture_output=True, timeout=130)
    if result.returncode==0:
        return json.loads(result.stdout), None
    assert result.returncode==2 and b"no character records" in result.stderr, result.stderr[-2000:]
    checked=subprocess.run([str(binary),"--ir",str(path)],capture_output=True,check=True,timeout=130)
    page=json.loads(checked.stdout)["pages"][0]
    assert not page["blocks"] and page["sdk_text_objects_without_characters"]>0
    return [], dict(error=result.stderr.decode().strip(),
                    sdk_text_objects_without_characters=page["sdk_text_objects_without_characters"])


def text(runs):
    return compact(''.join(run['text'] for run in runs))


def geometry(a, b, old_unit, new_unit):
    if len(a) != len(b) or any(x['text'] != y['text'] for x, y in zip(a, b)):
        return None
    errors = []
    for x, y in zip(a, b):
        errors.extend(abs(x['bounds'][k]*old_unit-y['bounds'][k]*new_unit)
                      for k in ['x0', 'y0', 'x1', 'y1'])
        errors.append(abs(x['size']*old_unit-y['size']*new_unit))
        if len(x['chars']) != len(y['chars']):
            return None
        errors.extend(abs(s[k]*old_unit-t[k]*new_unit)
                      for s, t in zip(x['chars'], y['chars']) for k in ['x0', 'x1'])
    return max(errors, default=0.0)


def render(binary, source, target):
    subprocess.run([str(binary), '--render', str(source), '1', '400', str(target)],
                   capture_output=True, check=True, timeout=130)
    return hashlib.sha256(target.read_bytes()).hexdigest()


def rebase_page_patterns(page, factor, writer):
    # A page resource pattern's matrix maps into default page space; a content
    # cm does not change it (Adobe PDF Reference 1.5, section 4.6). Clone only
    # the page-level dictionaries so other pages keep their original resources.
    resources = page.get('/Resources')
    if resources is None:
        return 0
    resources = resources.get_object()
    patterns = resources.get('/Pattern')
    if patterns is None:
        return 0
    local = DictionaryObject(dict(resources))
    replacements = DictionaryObject()
    for name, reference in patterns.get_object().items():
        pattern = copy.copy(reference.get_object())
        pattern.indirect_reference = None
        matrix = pattern.get('/Matrix', [1, 0, 0, 1, 0, 0])
        if hasattr(matrix, 'get_object'):
            matrix = matrix.get_object()
        pattern[NameObject('/Matrix')] = ArrayObject([FloatObject(float(v)/factor) for v in matrix])
        replacements[name] = writer._add_object(pattern)
    local[NameObject('/Pattern')] = replacements
    page[NameObject('/Resources')] = local
    return len(replacements)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--report', type=Path, default=ROOT/'corpus/results-coordinate-units.json')
    parser.add_argument('--only', nargs='+')
    args = parser.parse_args()
    args.report.unlink(missing_ok=True)
    manifest_bytes = (ROOT/'corpus/manifest.json').read_bytes()
    manifest = json.loads(manifest_bytes)
    entries = manifest['documents']
    if args.only:
        assert set(args.only) <= {e['id'] for e in entries}, 'Unknown source ID'
        entries = [e for e in entries if e['id'] in args.only]
    rows = []
    with tempfile.TemporaryDirectory(prefix='jade-coordinate-units-') as temporary:
        directory = Path(temporary)
        binary = directory/'pdf-diff'
        shutil.copy2(args.binary, binary)
        binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
        baseline = None
        if args.baseline:
            baseline = directory/'baseline'
            shutil.copy2(args.baseline, baseline)
        baseline_hash = hashlib.sha256(baseline.read_bytes()).hexdigest() if baseline else None
        for entry in entries:
            row = dict(id=entry['id'], passed=False)
            start = time.monotonic()
            try:
                fetch(entry, ROOT/'corpus/cache', offline=True)
                source = ROOT/'corpus/cache'/(entry['id']+'.pdf')
                writer = PdfWriter()
                writer.clone_document_from_reader(PdfReader(source))
                page = writer.pages[0]
                value = page.get('/UserUnit', 1)
                old_unit = float(value.get_object() if hasattr(value, 'get_object') else value)
                assert 0 < old_unit <= 75000
                factor = 64.0 if old_unit*64.0 <= 75000 else 1.0/64.0
                page.scale_by(1.0/factor)
                pattern_count = rebase_page_patterns(page, factor, writer)
                new_unit = old_unit*factor
                page[NameObject('/UserUnit')] = FloatObject(new_unit)
                rebased = directory/'rebased.pdf'
                writer.write(rebased)
                (a, rejection_a), (b, rejection_b) = layer(binary, source), layer(binary, rebased)
                matching_rejection=(rejection_a is not None and rejection_b is not None
                    and rejection_a["sdk_text_objects_without_characters"]==rejection_b["sdk_text_objects_without_characters"])
                assert (rejection_a is None and rejection_b is None) or matching_rejection
                row.update(source_text_layer_rejection=rejection_a, rebased_text_layer_rejection=rejection_b,
                           matching_text_layer_rejection=matching_rejection)
                old_text, new_text = text(a), text(b)
                maximum = geometry(a, b, old_unit, new_unit)
                flags_equal = (sum(r['sdk_unicode_mapping_errors'] for r in a) ==
                               sum(r['sdk_unicode_mapping_errors'] for r in b))
                png_equal = (render(binary, source, directory/'old.png') ==
                             render(binary, rebased, directory/'new.png'))
                row.update(old_user_unit=old_unit, new_user_unit=new_unit,
                           page_patterns_rebased=pattern_count,
                           rebased_sha256=hashlib.sha256(rebased.read_bytes()).hexdigest(),
                           has_text=bool(old_text), ordered_text_equal=old_text == new_text,
                           scalar_inventory_equal=Counter(old_text) == Counter(new_text),
                           maximum_physical_geometry_error=maximum,
                           sdk_mapping_diagnostics_equal=flags_equal, png_equal=png_equal)
                if baseline:
                    (before, _), (after, _) = layer(baseline, source), layer(baseline, rebased)
                    error = geometry(before, after, old_unit, new_unit)
                    row['baseline_passed'] = (text(before) == text(after) and
                                              error is not None and error <= 0.001)
                row['text_geometry_passed'] = (not matching_rejection and row['ordered_text_equal'] and maximum is not None and
                                               maximum <= 0.001 and flags_equal)
                row['passed'] = row['text_geometry_passed'] and png_equal
                # Default appearance/font sizing and other annotation contexts
                # are not generally rebased by pypdf's page scaler. Preserve
                # their observed paint differences for adjudication, never as
                # evidence of visual equivalence or as a diagnosed SDK defect.
                row['status'] = ('text_review' if matching_rejection and png_equal else 'passed' if row['passed'] else
                                 'appearance_review' if row['text_geometry_passed'] else 'failed')
            except Exception as error:
                row['error'] = str(error)
                row['status'] = 'failed'
            row['seconds'] = round(time.monotonic()-start, 4)
            rows.append(row)
            label = {'passed': 'PASS', 'appearance_review': 'REVIEW', 'text_review':'TEXT_REVIEW', 'failed': 'FAIL'}[row['status']]
            print(label+' '+entry['id'], flush=True)
    report = dict(version=1, methodology=__doc__, binary_sha256=binary_hash,
                  baseline_binary_sha256=baseline_hash, pypdf_version=pypdf_version,
                  manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
                  tool_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  documents=len(rows), passed=sum(r['passed'] for r in rows),
                  text_geometry_passed=sum(r.get('text_geometry_passed', False) for r in rows),
                  appearance_review_documents=sum(r['status']=='appearance_review' for r in rows),
                  text_review_documents=sum(r['status']=='text_review' for r in rows),
                  failed=sum(r['status']=='failed' for r in rows),
                  baseline_failures=sum(r.get('baseline_passed') is False for r in rows) if baseline else None,
                  physical_geometry_tolerance=0.001, results=rows)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
    print(f"{report['passed']}/{len(rows)} coordinate rebases passed")
    return 1 if report['failed'] else 3 if report['appearance_review_documents'] or report['text_review_documents'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
