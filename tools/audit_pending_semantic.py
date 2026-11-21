#!/usr/bin/env python3
"""未達の意味goldを範囲別に測り、診断できた失敗と正しい抽出を区別する。"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

from pdf_corpus import ROOT, fetch
from semantic_gold import check_gold, normalize_text


def same_text(actual, expected):
    """順序・記号・結合制御を保持し、正準表記と連続空白だけを揃える。"""
    return normalize_text(actual) == normalize_text(expected)


def invoke(binary, *arguments):
    """CLI の期限付き実行から、終了コードとJSONを回収する。"""
    result = subprocess.run([str(binary), *map(str, arguments)], capture_output=True,
                            text=True, timeout=180)
    if result.returncode not in (0, 1, 3):
        raise RuntimeError(result.stderr[-2000:])
    return result.returncode, json.loads(result.stdout)


def main():
    """未達文字列を成功率から除外せず、要レビューの報告を別途出力する。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--report', type=Path, default=ROOT/'corpus/results-pending-semantic-deseret.json')
    args = parser.parse_args()
    binary = args.binary.resolve()
    manifest = json.loads((ROOT/'corpus/manifest.json').read_text())
    rows, errors = [], []
    for entry in manifest.get('pending_semantic', []):
        try:
            fetch(entry, ROOT/'corpus/cache', offline=True)
            source = ROOT/'corpus/cache'/(entry['id']+'.pdf')
            _, ir = invoke(binary, '--ir', source)
            page = ir['pages'][entry['gold']['page'] - 1]
            actual = '\n'.join(block['text'] for block in page['blocks'])
            gold_check = check_gold(actual, entry['gold'])
            matches = gold_check['matches']
            with tempfile.TemporaryDirectory(prefix='jade-semantic-gold-') as directory:
                rewrite = Path(directory)/'equivalent.pdf'
                rewrite.write_bytes(source.read_bytes() + b'\n% equivalent source rewrite\n')
                code, diff = invoke(binary, source, rewrite)
            false_changes = any(diff['summary'].values())
            rows.append(dict(id=entry['id'], page=entry['gold']['page'],
                             source_sha256=entry['sha256'], gold_check=gold_check,
                             semantic_gold_matches=matches,
                             comparison_exit=code, mismatch_is_guarded=not matches and code == 3,
                             false_changes_on_equivalent_rewrite=false_changes,
                             diagnostics=diff['diagnostics']))
        except (OSError, ValueError, KeyError, RuntimeError, subprocess.TimeoutExpired) as error:
            errors.append(dict(id=entry['id'], error=str(error)))
    correct = sum(row['semantic_gold_matches'] for row in rows)
    failed = sum(not row['semantic_gold_matches'] for row in rows)
    unsafe = sum((not row['semantic_gold_matches'] and row['comparison_exit'] != 3)
                 or row['false_changes_on_equivalent_rewrite'] for row in rows)
    by_kind = {}
    for row in rows:
        count = by_kind.setdefault(row['gold_check']['kind'], dict(cases=0, correct=0, failed=0))
        count['cases'] += 1
        count['correct' if row['semantic_gold_matches'] else 'failed'] += 1
    report = dict(version=2, cases=len(manifest.get('pending_semantic', [])),
                  gold_by_kind=by_kind,
                  correct_cases=correct, failed_cases=failed,
                  guarded_failures=sum(row['mismatch_is_guarded'] for row in rows),
                  unguarded_or_false_change_failures=unsafe,
                  execution_errors=errors, results=rows,
                  binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                  manifest_sha256=hashlib.sha256((ROOT/'corpus/manifest.json').read_bytes()).hexdigest(),
                  gold_checker_sha256=hashlib.sha256((ROOT/'tools/semantic_gold.py').read_bytes()).hexdigest(),
                  method='Gold kind is explicit: full-page text uses NFC and whitespace collapse while retaining order, marks and joiners; scalar inventories count literal declared ranges only and do not assess page prose, order or geometry. Guarded failures are failures, not correct extractions.')
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
    print(json.dumps({key: report[key] for key in ('cases','gold_by_kind','correct_cases','failed_cases','guarded_failures','unguarded_or_false_change_failures','execution_errors')}, ensure_ascii=False))
    return 1 if errors or unsafe else 3 if failed else 0


if __name__ == '__main__':
    raise SystemExit(main())
