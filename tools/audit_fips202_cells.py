#!/usr/bin/env python3
"""FIPS202の限定変更2ページで、公開セル・原文所有・診断を変更前と照合する。"""
import argparse
import copy
import hashlib
import json
from pathlib import Path

from audit_pending_semantic import invoke
from audit_spacing_cells import records,digest
from pdf_corpus import ROOT,fetch

SOURCE_SHA='1592607831ff0908cc590632ce371c6c95e94025bb1a0c8ae90a4d0ec1ed025e'
DOCUMENT='nist-fips202-2015'


def compare(old,new,page):
    """14ページの確認済みマイナス4文字だけを等価化し、35ページは所有付き多重集合を保つ。"""
    before=copy.deepcopy(old)
    if page==14:
        valid=sum(row[0]=='\uf02d' for row in before)==4 and not any(row[0]=='\uf02d' for row in new)
        for row in before:
            if row[0]=='\uf02d':row[0]='−'
        matches=valid and before==new
        mode='Four confirmed F02D->2212 scalars only; exact ordered cell/ownership records'
    elif page==35:
        canonical=lambda rows:sorted(json.dumps(row,sort_keys=True,ensure_ascii=False,allow_nan=False) for row in rows)
        matches=canonical(before)==canonical(new)
        mode='Literal scalar/cell/ownership multiset; order is checked separately against original reading-order gold'
    else:
        raise ValueError('Unreviewed transition page')
    return dict(page=page,matches=matches,nonwhitespace_scalars=len(old),source_owned_scalars=sum(bool(row[4]) for row in old),
                before_sha256=digest(old),after_sha256=digest(new),scope=mode)


def main():
    """原本・CLI・入力一覧を固定し、変更ページが宣言した2ページ以外なら失敗させる。"""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--baseline',type=Path,required=True)
    parser.add_argument('--impact',type=Path,required=True)
    parser.add_argument('--report',type=Path,default=ROOT/'corpus/results-fips202-cells.json')
    args=parser.parse_args();binary=args.binary.resolve();baseline=args.baseline.resolve()
    sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
    impact=json.loads(args.impact.read_text());manifest_path=ROOT/'corpus/manifest.json';manifest=json.loads(manifest_path.read_text())
    if (impact['binary_sha256']!=sha(binary) or impact['baseline_sha256']!=sha(baseline)
            or impact['manifest_sha256']!=sha(manifest_path)):
        raise ValueError('Impact does not identify these artifacts')
    cohort=[(d['id'],c['page']) for d in impact['results'] for c in d['changes']]
    if sorted(cohort)!=[(DOCUMENT,14),(DOCUMENT,35)] or impact['changed_pages']!=2:
        raise ValueError('Changed-page cohort differs from the reviewed two pages')
    entry=next(e for e in manifest['documents'] if e['id']==DOCUMENT)
    if entry['sha256']!=SOURCE_SHA:raise ValueError('Original source differs')
    fetch(entry,ROOT/'corpus/cache',offline=True);source=ROOT/'corpus/cache'/(DOCUMENT+'.pdf')
    results=[]
    for page in (14,35):
        old,new=[records(invoke(exe,'--text-layer',source,page)[1]) for exe in (baseline,binary)]
        results.append(compare(old,new,page))
    report=dict(cases=len(results),matched=sum(r['matches'] for r in results),results=results,
        nonwhitespace_scalars=sum(r['nonwhitespace_scalars'] for r in results),
        source_owned_scalars=sum(r['source_owned_scalars'] for r in results),
        source_sha256=SOURCE_SHA,binary_sha256=sha(binary),baseline_sha256=sha(baseline),
        manifest_sha256=sha(manifest_path),impact_sha256=sha(args.impact),audit_sha256=sha(__file__),
        scope='Public scalar spans, run bounds/sizes, source-region ownership and diagnostics, limited to the exact two reviewed changed pages. Only four original minus aliases are normalized. Not glyph-ink truth, full-page semantic text or full diff precision/recall.')
    args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(f"FIPS202 public cells: {report['matched']}/{report['cases']} pages, {report['nonwhitespace_scalars']} nonwhitespace scalars")
    return int(report['matched']!=report['cases'])


if __name__=='__main__':raise SystemExit(main())
