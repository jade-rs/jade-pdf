#!/usr/bin/env python3
"""原本から全件転記した選択ページの本文編集を、余分な差分も含めて監査する。"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from audit_scalar_revision import invoke
from page_revision_gold import check_page_revision
from pdf_corpus import ROOT,fetch


def digest(path):
    """原本・gold・監査実装・実行成果物を識別する。"""
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    """入力一覧とgold双方のSHAを照合し、未達を成功へ読み替えず保存する。"""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--gold',type=Path,default=ROOT/'corpus/w4-page-revision-gold.json')
    parser.add_argument('--report',type=Path,default=ROOT/'corpus/results-page-revision.json')
    args=parser.parse_args();binary=args.binary.resolve();gold=json.loads(args.gold.read_text())
    manifest=ROOT/'corpus/manifest.json';entries={e['id']:e for e in json.loads(manifest.read_text())['documents']}
    results=[];errors=[]
    try:
        paths={}
        for side in ['old','new']:
            entry=entries[gold[side+'_id']]
            if entry['sha256']!=gold[side+'_sha256']:raise ValueError('Gold source SHA differs from manifest')
            fetch(entry,ROOT/'corpus/cache',offline=True)
            paths[side]=ROOT/'corpus/cache'/(entry['id']+'.pdf')
        code,diff=invoke(binary,paths['old'],paths['new'])
        check=check_page_revision(diff,gold)
        results.append(dict(id=gold['id'],comparison_exit=code,**check))
    except (ValueError,TypeError,KeyError,OSError,RuntimeError,subprocess.TimeoutExpired) as error:
        errors.append(dict(id=gold.get('id'),error=str(error)))
    report=dict(version=1,cases=1,passed=sum(r['passed'] for r in results),results=results,execution_errors=errors,
                binary_sha256=digest(binary),manifest_sha256=digest(manifest),gold_sha256=digest(args.gold),
                checker_sha256=digest(Path(__file__).with_name('page_revision_gold.py')),audit_sha256=digest(__file__),
                scope='Exhaustive logical text-edit regions on the declared original page pair only. Positional/line-wrap/format/form changes and other pages are separate. Full-page review and typed gold are independent of the engine diff; partial source contexts alone cannot establish precision/recall.')
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(f"Exhaustive selected-page text edits: {report['passed']}/{report['cases']}; errors: {len(errors)}")
    for result in results:print(f"TP={result['true_positives']} FP={result['false_positives']} FN={result['false_negatives']}")
    return 0 if report['passed']==1 and not errors else 1


if __name__=='__main__':raise SystemExit(main())
