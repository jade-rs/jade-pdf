#!/usr/bin/env python3
"""目視したNISTの不等号だけを独立goldとして照合する。全文の正解率ではない。"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from pdf_corpus import ROOT, fetch
from semantic_gold import check_gold


def main():
    """固定した原本、選択ページの文字数、実際の不等号編集を期限内に検証する。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--report', type=Path, default=ROOT/'corpus/results-symbol-operators.json')
    parser.add_argument('--offline', action='store_true')
    args = parser.parse_args()
    manifest = json.loads((ROOT/'corpus/manifest.json').read_text())
    entries = {e['id']: e for e in manifest['documents']}
    rows = []
    binary = args.binary.resolve()
    for gold in manifest['symbol_operator_gold']:
        entry = entries[gold['id']]
        fetch(entry, ROOT/'corpus/cache', offline=args.offline)
        path = ROOT/'corpus/cache'/(entry['id']+'.pdf')
        run = subprocess.run([str(binary), '--ir', str(path)], capture_output=True, text=True, timeout=180, check=True)
        page = json.loads(run.stdout)['pages'][gold['page'] - 1]
        actual = '\n'.join(b['text'] for b in page['blocks'])
        check = check_gold(actual, gold)
        fragment_matches = gold['compact_fragment'] in ''.join(actual.split())
        rows.append(dict(id=entry['id'], source_sha256=entry['sha256'], page=gold['page'],
                         gold_check=check, expected_compact_fragment=gold['compact_fragment'],
                         compact_fragment_matches=fragment_matches,
                         private_f0a3=actual.count('\uf0a3'),
                         diagnostics={k: v for k, v in page.items() if k.startswith(('sdk_', 'source_')) or k == 'reading_order_requires_review'},
                         passed=check['matches'] and fragment_matches and '\uf0a3' not in actual))
    report = dict(binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                  manifest_sha256=hashlib.sha256((ROOT/'corpus/manifest.json').read_bytes()).hexdigest(),
                  source_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  semantic_gold_sha256=hashlib.sha256((ROOT/'tools/semantic_gold.py').read_bytes()).hexdigest(),
                  scope='Selected visible operator inventory and introductory bound only; not full-page accuracy, reading order, geometry, or full natural-revision precision/recall.',
                  checks=len(rows), passed=sum(row['passed'] for row in rows), results=rows)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
    print(f"Visible operator gold: {report['passed']}/{report['checks']}")
    return 0 if report['passed'] == report['checks'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
