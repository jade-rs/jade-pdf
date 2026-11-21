#!/usr/bin/env python3
"""通常資料の独立したページ全文goldを厳格に照合する。未達群とは別集計。"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

from audit_pending_semantic import invoke
from pdf_corpus import ROOT, fetch
from semantic_gold import check_gold, page_has_text_review


def regular_cases(manifest):
    """通常資料に明示されたgoldだけを選び、旧一ページ形式も範囲を保持する。"""
    cases = []
    for entry in manifest.get('documents', []):
        if 'gold' in entry and 'full_page_text' in entry:
            raise ValueError(f"{entry['id']}: ambiguous page gold declarations")
        if 'gold' in entry:
            cases.append((entry, entry['gold']))
        elif 'full_page_text' in entry:
            if type(entry.get('pages')) is not int or entry['pages'] != 1:
                raise ValueError(f"{entry['id']}: legacy full_page_text requires one page")
            cases.append((entry, dict(page=1, kind='full_page_text', text=entry['full_page_text'])))
    return cases


def check_page(ir, gold):
    """1始まりの明示ページ全体を照合し、範囲外やboolを拒否する。"""
    page = gold.get('page')
    if type(page) is not int or not 1 <= page <= len(ir['pages']):
        raise ValueError('Gold page must be a one-based integer within the extracted document')
    return check_gold('\n'.join(block['text'] for block in ir['pages'][page - 1]['blocks']), gold)


def main():
    """原本SHAと選択ページを固定し、通常goldの不一致は診断の有無によらず失敗させる。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--report', type=Path, default=ROOT/'corpus/results-page-gold.json')
    args = parser.parse_args()
    binary = args.binary.resolve()
    manifest_path = ROOT/'corpus/manifest.json'
    manifest = json.loads(manifest_path.read_text())
    cases = regular_cases(manifest)
    rows, errors = [], []
    if not cases:
        errors.append(dict(error='No regular document page gold declared'))
    for entry, gold in cases:
        try:
            fetch(entry, ROOT/'corpus/cache', offline=True)
            source = ROOT/'corpus/cache'/(entry['id']+'.pdf')
            extraction_exit, ir = invoke(binary, '--ir', source)
            if len(ir['pages']) != entry['pages']:
                raise ValueError('Extracted page count differs from pinned source metadata')
            check = check_page(ir, gold)
            with tempfile.TemporaryDirectory(prefix='jade-page-gold-') as directory:
                rewrite = Path(directory)/'equivalent.pdf'
                rewrite.write_bytes(source.read_bytes() + b'\n% equivalent source rewrite\n')
                code, diff = invoke(binary, source, rewrite)
            rows.append(dict(id=entry['id'], page=gold['page'], source_sha256=entry['sha256'],
                             extraction_exit=extraction_exit, gold_check=check,
                             selected_page_has_text_review=page_has_text_review(ir['pages'][gold['page'] - 1]),
                             equivalent_rewrite_exit=code,
                             false_changes_on_equivalent_rewrite=any(diff['summary'].values()),
                             diagnostics=diff['diagnostics']))
        except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.TimeoutExpired) as error:
            errors.append(dict(id=entry['id'], error=str(error)))
    by_kind = {}
    for row in rows:
        count = by_kind.setdefault(row['gold_check']['kind'], dict(cases=0, correct=0, failed=0))
        count['cases'] += 1
        count['correct' if row['gold_check']['matches'] else 'failed'] += 1
    report = dict(version=1, cohort='documents', cases=len(cases), gold_by_kind=by_kind,
                  correct_cases=sum(row['gold_check']['matches'] for row in rows),
                  failed_cases=sum(not row['gold_check']['matches'] for row in rows),
                  false_change_cases=sum(row['false_changes_on_equivalent_rewrite'] for row in rows),
                  execution_errors=errors, results=rows,
                  binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                  manifest_sha256=hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
                  audit_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  gold_checker_sha256=hashlib.sha256((ROOT/'tools/semantic_gold.py').read_bytes()).hexdigest(),
                  method='Only explicitly declared gold in the regular documents cohort. Full-page text compares all selected-page text using NFC and whitespace collapse, retaining order, marks and joiners. No substring match. No claim about other pages or highlight geometry. Unresolved pending_semantic cases remain separately audited and are not included or relabelled here. Equivalent rewrite agreement is not semantic accuracy.')
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
    print(json.dumps({key: report[key] for key in ('cohort', 'cases', 'gold_by_kind', 'correct_cases',
                                                 'failed_cases', 'false_change_cases', 'execution_errors')}, ensure_ascii=False))
    return int(bool(errors or report['failed_cases'] or report['false_change_cases']))


if __name__ == '__main__':
    raise SystemExit(main())
