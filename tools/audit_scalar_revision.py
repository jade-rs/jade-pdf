#!/usr/bin/env python3
"""固定原本の一文字改訂を、差分区間と公開文字レイヤーの局所位置で検証する。"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

from pdf_corpus import ROOT, fetch
from scalar_revision_gold import check_revision, check_span, rectangle


def check_payload(payload):
    """nativeまたはWASMの同じ出力契約へ、独立goldを同じ条件で適用する。"""
    gold=payload['gold'];diff=payload['diff'];edit=check_revision(diff,gold)
    spans={side:check_span(payload[side+'_layer'],gold,side,edit.get(side+'_block')) for side in ['old','new']}
    containing={}
    for side in ['old','new']:
        try:
            box=rectangle(edit.get(side+'_bounds'));cell=rectangle(gold['source_cells'][side])
            x=cell[0]/2+cell[2]/2;y=cell[1]/2+cell[3]/2
            containing[side]=box[0]<=x<=box[2] and box[1]<=y<=box[3]
        except ValueError:containing[side]=False
    return dict(passed=edit['passed'] and all(s['passed'] for s in spans.values()) and all(containing.values()),
                scalar_edit=edit,character_cells=spans,reported_block_contains_source_cell=containing,
                diagnostics=diff.get('diagnostics'),comparison=diff.get('comparison'),
                scope='One independently reviewed scalar edit and horizontal character-cell positions only; not whole-document precision/recall or pixel-outline highlight gold.')


def invoke(binary,*args):
    """固定入力のCLI出力を期限内に読み、要レビューの終了コードも区別して残す。"""
    result=subprocess.run([str(binary),*map(str,args)],capture_output=True,text=True,timeout=180)
    if result.returncode not in [0,1,3]:raise RuntimeError(result.stderr[-2000:])
    return result.returncode,json.loads(result.stdout)


def main():
    """通常の原本監査と、ブラウザーが回収したJSON出力の同一gold検査を提供する。"""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path)
    parser.add_argument('--report',type=Path,default=ROOT/'corpus/results-scalar-revision.json')
    parser.add_argument('--check-json',action='store_true')
    args=parser.parse_args()
    if args.check_json:
        result=check_payload(json.load(sys.stdin));print(json.dumps(result,ensure_ascii=False))
        return 0 if result['passed'] else 1
    if args.binary is None:parser.error('--binary is required for source auditing')
    binary=args.binary.resolve();manifest_bytes=(ROOT/'corpus/manifest.json').read_bytes();manifest=json.loads(manifest_bytes)
    entries={e['id']:e for e in manifest['documents']};results=[];errors=[]
    for gold in manifest.get('scalar_revision_gold',[]):
        try:
            paths={}
            for side in ['old','new']:
                entry=entries[gold[side+'_id']]
                if entry['sha256']!=gold[side+'_sha256']:raise ValueError('Gold source SHA differs from manifest')
                fetch(entry,ROOT/'corpus/cache',offline=True)
                paths[side]=ROOT/'corpus/cache'/(entry['id']+'.pdf')
            code,diff=invoke(binary,paths['old'],paths['new'])
            layers={side:invoke(binary,'--text-layer',paths[side],gold[side+'_page'])[1] for side in ['old','new']}
            check=check_payload(dict(gold=gold,diff=diff,old_layer=layers['old'],new_layer=layers['new']))
            results.append(dict(id=gold['id'],comparison_exit=code,old_sha256=gold['old_sha256'],new_sha256=gold['new_sha256'],**check))
        except (ValueError,KeyError,RuntimeError,OSError,subprocess.TimeoutExpired) as error:
            errors.append(dict(id=gold.get('id'),error=str(error)))
    report=dict(cases=len(manifest.get('scalar_revision_gold',[])),passed=sum(r['passed'] for r in results),results=results,execution_errors=errors,
                binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
                checker_sha256=hashlib.sha256((ROOT/'tools/scalar_revision_gold.py').read_bytes()).hexdigest(),
                source_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                scope='Selected natural-revision scalar and character-cell gold only; not whole-page or full-document precision/recall.')
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(f"Scoped natural revision: {report['passed']}/{report['cases']}; execution errors: {len(errors)}")
    return 0 if report['cases'] and report['passed']==report['cases'] and not errors else 1


if __name__=='__main__':raise SystemExit(main())
