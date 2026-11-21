#!/usr/bin/env python3
"""空白が変わった全ページで、非空白文字・字面セル・原文領域の所有を旧CLIと照合する。"""
import argparse
import concurrent.futures
import hashlib
import json
from pathlib import Path

from audit_pending_semantic import invoke
from pdf_corpus import ROOT, fetch


def records(layer):
    """空白以外の各スカラーに、公開セルとその位置を所有する原文領域を結び付ける。"""
    rows=[]
    for run in layer:
        if len(run['text']) != len(run['chars']):
            raise ValueError('Public scalar/span count differs')
        regions=run.get('source_regions', [])
        for region in regions:
            start,end=region['start'],region['end']
            if type(start) is not int or type(end) is not int or not 0<=start<=end<=len(run['text']):
                raise ValueError('Invalid source ownership range')
        diagnostics={key:value for key,value in run.items() if key.startswith(('sdk_','source_','reading_'))
                     and key not in ('source_regions',)}
        for i,(scalar,span) in enumerate(zip(run['text'],run['chars'])):
            if scalar.isspace():continue
            owners=[dict(bounds=region['bounds'],anchor=region.get('anchor',False))
                    for region in regions if region['start']<=i<region['end']]
            rows.append([scalar,span,run['bounds'],run['size'],owners,diagnostics])
    return rows


def digest(value):
    """非有限なJSON数値を拒否し、順序を保った記録を固定する。"""
    return hashlib.sha256(json.dumps(value,sort_keys=True,ensure_ascii=False,allow_nan=False).encode()).hexdigest()


def main():
    """固定した全対照ページを検査し、一文字でも位置・順序・所有が変われば失敗させる。"""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--baseline',type=Path,required=True)
    parser.add_argument('--impact',type=Path,required=True)
    parser.add_argument('--report',type=Path,default=ROOT/'corpus/results-spacing-cells.json')
    args=parser.parse_args();binary=args.binary.resolve();baseline=args.baseline.resolve()
    hash_file=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
    impact=json.loads(args.impact.read_text());manifest=json.loads((ROOT/'corpus/manifest.json').read_text())
    if (impact['binary_sha256']!=hash_file(binary) or impact['baseline_sha256']!=hash_file(baseline)
            or impact['manifest_sha256']!=hash_file(ROOT/'corpus/manifest.json')):
        raise ValueError('Impact report does not identify these binaries and this manifest')
    entries={e['id']:e for e in manifest['documents']}
    cases=[(entries[d['id']],change['page']) for d in impact['results'] for change in d['changes']]
    if len(cases)!=impact['changed_pages'] or not cases:raise ValueError('No complete changed-page cohort')
    def inspect(case):
        entry,page=case
        fetch(entry,ROOT/'corpus/cache',offline=True)
        source=ROOT/'corpus/cache'/(entry['id']+'.pdf')
        old,new=[records(invoke(exe,'--text-layer',source,page)[1]) for exe in (baseline,binary)]
        return dict(id=entry['id'],page=page,source_sha256=entry['sha256'],nonwhitespace_scalars=len(old),
                    source_owned_nonwhitespace_scalars=sum(bool(row[4]) for row in old),
                    old_sha256=digest(old),new_sha256=digest(new),matches=old==new)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:results=list(pool.map(inspect,cases))
    report=dict(version=1,cases=len(results),matched=sum(r['matches'] for r in results),
                nonwhitespace_scalars=sum(r['nonwhitespace_scalars'] for r in results),results=results,
                source_owned_nonwhitespace_scalars=sum(r['source_owned_nonwhitespace_scalars'] for r in results),
                binary_sha256=hash_file(binary),baseline_sha256=hash_file(baseline),
                manifest_sha256=hash_file(ROOT/'corpus/manifest.json'),impact_sha256=hash_file(args.impact),
                audit_sha256=hash_file(__file__),scope='All pages changed by the paired native extraction audit. Literal nonwhitespace scalar sequence, public character spans, run bounds/font sizes, diagnostics and per-character source-region ownership must remain exact. Source-region indices may shift with removed blanks but ownership must not. Not semantic text gold, glyph-ink truth or full diff precision/recall.')
    args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(f"Public cells/ownership: {report['matched']}/{report['cases']} pages, {report['nonwhitespace_scalars']} literal nonwhitespace scalars")
    return int(report['matched']!=report['cases'])


if __name__=='__main__':raise SystemExit(main())
