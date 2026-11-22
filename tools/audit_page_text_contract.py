#!/usr/bin/env python3
"""全自然改訂ペアで、ページ本文の原文範囲・全文再構成と従来の差分の保存を監査する。"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

from audit_scalar_revision import invoke
from page_revision_gold import observations,page_observations
from pdf_corpus import ROOT,fetch


def digest(path):
    """実行成果物と固定入力のバイト列を識別する。"""
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    """未計算の本文を成功とせず、意味goldと区別した構造保存の結果を残す。"""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--baseline',type=Path)
    parser.add_argument('--report',type=Path,default=ROOT/'corpus/results-page-text-contract.json')
    args=parser.parse_args();binary=args.binary.resolve();baseline=args.baseline.resolve() if args.baseline else None
    manifest=ROOT/'corpus/manifest.json';registration=json.loads(manifest.read_text())
    entries={e['id']:e for e in registration['documents']};pairs=registration['revision_pairs'];rows=[];errors=[]
    for pair in pairs:
        try:
            files=[]
            for side in ['old','new']:
                entry=entries[pair[side]];fetch(entry,ROOT/'corpus/cache',offline=True)
                files.append(ROOT/'corpus/cache'/(entry['id']+'.pdf'))
            code,diff=invoke(binary,*files);pages=0;groups=0;limited=0
            for page in diff['pages']:
                _,texts,_=observations(page);page_observations(page,texts);view=page.pop('text_diff')
                pages+=1;groups+=len(view['groups']);limited+=bool(view.get('alignment_limited'))
            diff['diagnostics'].pop('page_text_alignment_limited_pages',None)
            if limited:raise ValueError('Page text alignment budget reached')
            if baseline:
                before_code,before=invoke(baseline,*files)
                if before_code!=code or before!=diff:raise ValueError('Existing raw diff, geometry, form, visual or diagnostic evidence changed')
            rows.append(dict(id=pair['id'],passed=True,pages=pages,text_groups=groups,alignment_limited_pages=limited,
                comparison_exit=code,raw_diff_exactly_preserved=True if baseline else None,
                ordered_source_coverage_and_segment_reconstruction=True,
                old_sha256=entries[pair['old']]['sha256'],new_sha256=entries[pair['new']]['sha256']))
        except (ValueError,TypeError,KeyError,OSError,RuntimeError,subprocess.TimeoutExpired) as error:
            errors.append(dict(id=pair['id'],error=str(error)))
    report=dict(version=1,cases=len(pairs),passed=sum(r['passed'] for r in rows),pages=sum(r['pages'] for r in rows),
        results=rows,execution_errors=errors,binary_sha256=digest(binary),baseline_binary_sha256=digest(baseline) if baseline else None,
        manifest_sha256=digest(manifest),checker_sha256=digest(ROOT/'tools/page_revision_gold.py'),audit_sha256=digest(__file__),
        scope='All registered natural revision pairs: ordered source coverage and segment reconstruction contracts only, not independent semantic precision/recall. Optional baseline also requires exact preservation of raw diff evidence. Exhaustive edit gold remains limited to W-4 2024/2025 page 1.')
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(f"Natural page-text contract: {report['passed']}/{report['cases']}; pages: {report['pages']}; errors: {len(errors)}")
    return 0 if pairs and report['passed']==len(pairs) and not errors else 1


if __name__=='__main__':raise SystemExit(main())
