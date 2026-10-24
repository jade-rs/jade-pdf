#!/usr/bin/env python3
"""Audit every declared visual table gold, including known failing sources.

The original SHA-pinned page renderings were manually transcribed into the
manifest. pypdf structure/MCIDs and pdfplumber characters are independent cell
association evidence, not an original-Unicode or accessibility oracle. A source
with incorrect output order remains in the totals and returns review status 3.
Source-tag association completeness is reported separately: malformed original
tags cannot invalidate a future correct ordering against the visual gold.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import subprocess
import unicodedata

import pdfplumber
import pypdf

from pdf_corpus import ROOT, fetch


def compact(value):
    return ''.join(unicodedata.normalize('NFC', value).split())


def structure_children(obj):
    value = obj.get('/K', [])
    return value if isinstance(value, list) else [value]


def structure_find(obj, role, depth=0, budget=None):
    if budget is None:
        budget = [10000]
    budget[0] -= 1
    if depth >= 64 or budget[0] < 0:
        raise ValueError('Offline structure audit budget exceeded')
    obj = obj.get_object() if hasattr(obj, 'get_object') else obj
    if not isinstance(obj, dict):
        return []
    if str(obj.get('/S', '')) == role:
        return [obj]
    return [found for child in structure_children(obj)
            for found in structure_find(child, role, depth + 1, budget)]


def marked_ids(obj, depth=0, budget=None):
    if budget is None:
        budget = [10000]
    budget[0] -= 1
    if depth >= 64 or budget[0] < 0:
        raise ValueError('Offline cell audit budget exceeded')
    obj = obj.get_object() if hasattr(obj, 'get_object') else obj
    if isinstance(obj, int):
        return [int(obj)]
    if not isinstance(obj, dict):
        return []
    if '/MCID' in obj:
        return [int(obj['/MCID'])]
    return [i for child in structure_children(obj)
            for i in marked_ids(child, depth + 1, budget)]


def inspect(binary, source, page):
    def run(args):
        return json.loads(subprocess.run([str(binary.resolve()), *args],
                                        capture_output=True, check=True,
                                        timeout=120).stdout)
    doc = run(['--ir', str(source)])
    layer = run(['--text-layer', str(source), str(page)])
    return doc, layer


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--report', type=Path, default=ROOT/'corpus/results-table-order.json')
    args = parser.parse_args()
    args.report.unlink(missing_ok=True)
    manifest_bytes = (ROOT/'corpus/manifest.json').read_bytes()
    manifest = json.loads(manifest_bytes)
    results = []
    for entry in manifest['documents']:
        golds = ([entry['table_gold']] if entry.get('table_gold') else []) + entry.get('table_golds', [])
        if not golds:
            continue
        fetch(entry, ROOT/'corpus/cache', offline=True)
        source = ROOT/'corpus/cache'/(entry['id']+'.pdf')
        for gold in golds:
            expected = [value for row in gold['rows'] for value in row]
            sequence = compact(' '.join(expected))
            current, layer = inspect(args.binary, source, gold['page'])
            page_text = lambda doc: compact(''.join(b['text'] for b in doc['pages'][gold['page']-1]['blocks']))
            ordered = sequence in page_text(current)
            layer_ordered = sequence in compact(''.join(run['text'] for run in layer))
            page_review = bool(current['pages'][gold['page']-1].get('reading_order_requires_review', False))
            layer_review_matches = all(bool(run.get('reading_order_requires_review', False)) == page_review for run in layer)
            baseline, baseline_ordered, conserved, same_runs, same_payload = None, None, None, None, None
            if args.baseline:
                baseline, baseline_layer = inspect(args.baseline, source, gold['page'])
                baseline_ordered = sequence in page_text(baseline)
                key = lambda run: json.dumps(run, ensure_ascii=False, sort_keys=True)
                same_runs = Counter(map(key, layer)) == Counter(map(key, baseline_layer))
                payload = lambda run: key({k:v for k,v in run.items() if k != 'reading_order_requires_review'})
                same_payload = Counter(map(payload, layer)) == Counter(map(payload, baseline_layer))
                conserved = len(current['pages']) == len(baseline['pages']) and all(
                    Counter(compact(''.join(b['text'] for b in now['blocks']))) ==
                    Counter(compact(''.join(b['text'] for b in before['blocks'])))
                    and now['sdk_unicode_mapping_errors'] == before['sdk_unicode_mapping_errors']
                    for now, before in zip(current['pages'], baseline['pages']))
            reader = pypdf.PdfReader(source)
            tables = structure_find(reader.root_object['/StructTreeRoot'], '/Table')
            table_index = gold.get('source_table_index', 0)
            if not 0 <= table_index < len(tables):
                raise ValueError('Declared source table index is unavailable')
            cells = []
            for row in structure_find(tables[table_index], '/TR'):
                for child in structure_children(row):
                    child = child.get_object() if hasattr(child, 'get_object') else child
                    if isinstance(child, dict) and str(child.get('/S', '')) in ['/TH', '/TD']:
                        ids = marked_ids(child)
                        # Empty padding cells in spanning headers carry no ink.
                        if ids:
                            cells.append(dict(role=str(child['/S']), mcids=ids))
            by_id = defaultdict(str)
            with pdfplumber.open(source) as independent:
                for char in independent.pages[gold['page']-1].chars:
                    if char.get('mcid') is not None:
                        by_id[char['mcid']] += char['text']
            if len(cells) != len(expected):
                raise ValueError('Nonempty structure-cell count differs from visual gold')
            for cell, value in zip(cells, expected):
                cell.update(gold=value, independent_text=''.join(by_id[i] for i in cell['mcids']))
                cell['associated_text_matches'] = compact(cell['independent_text']) == compact(value)
                if not cell['associated_text_matches']:
                    cell['matching_text_other_mcids'] = [i for i, text in by_id.items()
                                                         if compact(text) == compact(value)]
            associated = sum(cell['associated_text_matches'] for cell in cells)
            passed = ordered and layer_ordered and conserved is not False and layer_review_matches
            result = dict(id=entry['id'], source_sha256=entry['sha256'], page=gold['page'], source_table_index=table_index,
                          passed=passed, ir_order_passed=ordered, layer_order_passed=layer_ordered,
                          baseline_order_passed=baseline_ordered,
                          all_page_scalar_bags_and_diagnostics_unchanged=conserved,
                          exact_text_run_multiset_unchanged=same_runs,
                          text_geometry_span_mapping_multiset_unchanged=same_payload,
                          reading_order_requires_review=page_review,
                          reading_order_diagnostic_transport_matches=layer_review_matches,
                          declared_cells=len(expected), independently_associated_cells=associated,
                          source_tag_association_complete=associated == len(cells),
                          cell_reference=cells, known_issue=gold.get('known_issue'))
            results.append(result)
            print(('PASS ' if passed else 'REVIEW ')+entry['id'], flush=True)
    if not results:
        raise ValueError('No table gold declared')
    digest = lambda path: hashlib.sha256(Path(path).read_bytes()).hexdigest()
    report = dict(version=3, methodology=__doc__, manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
                  binary_sha256=digest(args.binary), baseline_binary_sha256=digest(args.baseline) if args.baseline else None,
                  tool_sha256=digest(__file__), pypdf_version=pypdf.__version__, pdfplumber_version=pdfplumber.__version__,
                  documents=len({row['id'] for row in results}), tables=len(results), passed=sum(row['passed'] for row in results),
                  passed_documents=sum(all(x['passed'] for x in results if x['id']==name) for name in {x['id'] for x in results}),
                  review_documents=len({row['id'] for row in results if not row['passed']}),
                  reading_order_review_documents=len({row['id'] for row in results if row['reading_order_requires_review']}),
                  source_association_review_documents=len({row['id'] for row in results if not row['source_tag_association_complete']}),
                  declared_cells=sum(row['declared_cells'] for row in results),
                  independently_associated_cells=sum(row['independently_associated_cells'] for row in results),
                  results=results)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
    print(f"{report['passed']}/{report['tables']} visual table orders passed; "
          f"{report['review_documents']} ordering failures; "
          f"{report['reading_order_review_documents']} runtime reading-order reviews; "
          f"{report['source_association_review_documents']} incomplete original tag associations")
    return 3 if report['review_documents'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
