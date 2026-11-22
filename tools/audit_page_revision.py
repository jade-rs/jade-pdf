#!/usr/bin/env python3
"""原本から全件転記した選択ページの本文編集を、余分な差分も含めて監査する。"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

from audit_scalar_revision import invoke
from page_revision_gold import check_page_revision
from pdf_corpus import ROOT,fetch


def check_payload(payload):
    """全実行環境のJSONに同じ網羅的goldと範囲検査を適用する。"""
    return check_page_revision(payload['diff'],payload['gold'])


def digest(path):
    """原本・gold・監査実装・実行成果物を識別する。"""
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    """入力一覧とgold双方のSHAを照合し、未達を成功へ読み替えず保存する。"""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path)
    parser.add_argument('--check-json',action='store_true')
    parser.add_argument('--gold',type=Path,help='audit only this gold; default audits every registered page gold')
    parser.add_argument('--report',type=Path,default=ROOT/'corpus/results-page-revision.json')
    args=parser.parse_args()
    if args.check_json:
        check=check_payload(json.load(sys.stdin));print(json.dumps(check,ensure_ascii=False))
        return 0 if check['passed'] else 1
    if args.binary is None:parser.error('--binary is required for source auditing')
    binary=args.binary.resolve()
    manifest=ROOT/'corpus/manifest.json';registration=json.loads(manifest.read_text())
    entries={e['id']:e for e in registration['documents']}
    refs=registration.get('page_revision_gold',[])
    jobs=[(args.gold,None)] if args.gold else [(ROOT/ref['path'],ref) for ref in refs]
    results=[];errors=[];gold_hashes={};seen=set()
    for file,ref in jobs:
        gold={}
        try:
            if ref is not None and not file.resolve().is_relative_to((ROOT/'corpus').resolve()):raise ValueError('Page gold path escapes corpus')
            gold=json.loads(file.read_text());gold_hash=digest(file)
            if ref is None:ref=next((r for r in refs if r['id']==gold['id']),None)
            if ref is not None and (gold_hash!=ref['sha256']
                or any(gold[k]!=ref[k] for k in ['id','old_id','new_id','old_sha256','new_sha256'])):
                raise ValueError('Registered page gold or original source identity changed')
            if gold['id'] in seen:raise ValueError('Duplicate registered page gold')
            seen.add(gold['id']);gold_hashes[str(file)]=gold_hash
            if gold.get('source_review') is not None:
                review=(ROOT/gold['source_review']).resolve()
                if not review.is_relative_to((ROOT/'corpus').resolve()) or digest(review)!=gold['source_review_sha256']:
                    raise ValueError('Page gold source review SHA mismatch')
            paths={}
            for side in ['old','new']:
                entry=entries[gold[side+'_id']]
                if entry['sha256']!=gold[side+'_sha256']:raise ValueError('Gold source SHA differs from manifest')
                fetch(entry,ROOT/'corpus/cache',offline=True)
                paths[side]=ROOT/'corpus/cache'/(entry['id']+'.pdf')
            code,diff=invoke(binary,paths['old'],paths['new'])
            check=check_payload(dict(diff=diff,gold=gold))
            results.append(dict(id=gold['id'],comparison_exit=code,gold_sha256=gold_hash,**check))
        except (ValueError,TypeError,KeyError,OSError,RuntimeError,subprocess.TimeoutExpired) as error:
            errors.append(dict(id=ref.get('id') if ref else gold.get('id'),error=str(error)))
    report=dict(version=2,cases=len(jobs),passed=sum(r['passed'] for r in results),results=results,execution_errors=errors,
                binary_sha256=digest(binary),manifest_sha256=digest(manifest),gold_sha256=next(iter(gold_hashes.values())) if len(jobs)==1 and gold_hashes else None,gold_file_sha256=gold_hashes,
                checker_sha256=digest(Path(__file__).with_name('page_revision_gold.py')),audit_sha256=digest(__file__),
                region_checker_sha256=digest(Path(__file__).with_name('page_region_gold.py')),
                scope='Exhaustive logical text-edit regions on the declared original page pair only. Positional/line-wrap/format/form changes and other pages are separate. Full-page review and typed gold are independent of the engine diff; partial source contexts alone cannot establish precision/recall.')
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(f"Exhaustive selected-page text edits: {report['passed']}/{report['cases']}; errors: {len(errors)}")
    for result in results:print(f"TP={result['true_positives']} FP={result['false_positives']} FN={result['false_negatives']}")
    return 0 if jobs and report['passed']==len(jobs) and not errors else 1


if __name__=='__main__':raise SystemExit(main())
