#!/usr/bin/env python3
"""Check visually declared public reading-order anchors and baseline failures.

Selected public-page geometric order, not full text/table/bidi/tagged-PDF gold.
Character inventory conservation against the prior extractor is separate from
the reading-order assertions. Source bytes and executable identities are pinned.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import subprocess
import unicodedata

from pdf_corpus import ROOT, fetch


def compact(text):
    return ''.join(unicodedata.normalize('NFC', text).split())


def positions(text, anchors):
    text = compact(text)
    result = []
    offset = 0
    for anchor in anchors:
        value = compact(anchor)
        at = text.find(value, offset)
        result.append(at)
        if at < 0:
            return result, False
        offset = at + len(value)
    return result, True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--report', type=Path, default=ROOT/'corpus/results-reading-order.json')
    args = parser.parse_args()
    args.report.unlink(missing_ok=True)
    manifest_bytes = (ROOT/'corpus/manifest.json').read_bytes()
    manifest = json.loads(manifest_bytes)
    hashes = {name: hashlib.sha256(path.read_bytes()).hexdigest() if path else None
              for name, path in [('binary', args.binary), ('baseline', args.baseline)]}
    results = []
    for entry in manifest['documents']:
        if not entry.get('reading_order'):
            continue
        fetch(entry, ROOT/'corpus/cache', offline=True)
        path = ROOT/'corpus/cache'/(entry['id']+'.pdf')
        def inspect(binary):
            return json.loads(subprocess.run([str(binary.resolve()), '--ir', str(path)],
                                             capture_output=True, check=True, timeout=120).stdout)
        current = inspect(args.binary)
        baseline = inspect(args.baseline) if args.baseline else None
        assert len(current['pages']) == entry['pages']
        inventory_equal = None
        inventory_retained = None
        utf16_adjustments = []
        if baseline is not None:
            assert len(baseline['pages']) == entry['pages']
            inventory_equal = True
            inventory_retained = True
            utf16_adjustments = []
            for now,before in zip(current['pages'],baseline['pages']):
                old_bag=Counter(compact(''.join(block['text'] for block in before['blocks'])))
                new_bag=Counter(compact(''.join(block['text'] for block in now['blocks'])))
                if old_bag==new_bag:
                    continue
                inventory_equal=False
                gold=entry.get('unicode_expansion_gold')
                adjusted=dict(old_bag)
                valid=False
                if gold and now['index']==gold['page'] and old_bag['�']==gold['literal_replacements']+gold['sdk_utf16_units'] and new_bag['�']==gold['literal_replacements']:
                    adjusted=old_bag.copy()
                    adjusted['�']-=gold['sdk_utf16_units']
                    adjusted.update(gold['scalar_counts'])
                    valid=adjusted==new_bag
                    if valid:
                        utf16_adjustments.append(dict(page=now['index'],codeunits=gold['sdk_utf16_units'],unicode_scalars=gold['unicode_scalars'],scalar_counts=gold['scalar_counts']))
                inventory_retained &= valid
        checks = []
        for gold in entry['reading_order']:
            page = gold['page']-1
            current_positions, passed = positions('\n'.join(b['text'] for b in current['pages'][page]['blocks']), gold['anchors'])
            baseline_positions, baseline_passed = None, None
            if baseline is not None:
                baseline_positions, baseline_passed = positions('\n'.join(b['text'] for b in baseline['pages'][page]['blocks']), gold['anchors'])
            layer = json.loads(subprocess.run([str(args.binary.resolve()), '--text-layer', str(path), str(page+1)],
                                              capture_output=True, check=True, timeout=120).stdout)
            layer_positions, layer_passed = positions('\n'.join(run['text'] for run in layer), gold['anchors'])
            checks.append(dict(page=page+1, anchors=gold['anchors'], passed=passed,
                               text_layer_passed=layer_passed, text_layer_positions=layer_positions,
                               baseline_passed=baseline_passed, positions=current_positions,
                               baseline_positions=baseline_positions))
        passed = inventory_retained is not False and all(c['passed'] and c['text_layer_passed'] for c in checks)
        results.append(dict(id=entry['id'], passed=passed, pages=entry['pages'],
                            all_page_character_inventory_unchanged=inventory_equal,
                            all_page_character_inventory_retained_after_declared_utf16=inventory_retained,
                            declared_utf16_inventory_adjustments=utf16_adjustments, checks=checks))
        print(('PASS ' if passed else 'FAIL ')+entry['id'], flush=True)
    assert results, 'No reading-order gold declared'
    report = dict(version=1, methodology=__doc__, manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
                  binary_sha256=hashes['binary'], baseline_binary_sha256=hashes['baseline'],
                  documents=len(results), passed=sum(r['passed'] for r in results),
                  selected_gold_pages=sum(len(r['checks']) for r in results),
                  selected_anchor_checks=sum(len(c['anchors']) for r in results for c in r['checks']),
                  baseline_failing_documents=sum(any(c['baseline_passed'] is False for c in r['checks']) for r in results) if args.baseline else None,
                  results=results)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
    return int(report['passed'] != report['documents'])


if __name__ == '__main__':
    raise SystemExit(main())
