#!/usr/bin/env python3
"""行の結合変更について、固定コーパス全体の文字順・診断・ブロック数を対照する。"""
import argparse
import concurrent.futures
import hashlib
import json
from pathlib import Path
import subprocess
import unicodedata

from pdf_corpus import ROOT, fetch


def sequence(page):
    """空白と正準表記だけを揃え、文字順・互換文字・結合制御は保持する。"""
    return unicodedata.normalize('NFC',''.join(c for b in page['blocks'] for c in b['text'] if not c.isspace()))


def main():
    """変更の有無を記録し、文字順または診断が変われば目視による裁定を要求する。"""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--baseline',type=Path,required=True)
    parser.add_argument('--report',type=Path,default=ROOT/'corpus/results-run-grouping.json')
    args=parser.parse_args();binary=args.binary.resolve();baseline=args.baseline.resolve()
    manifest_bytes=(ROOT/'corpus/manifest.json').read_bytes();manifest=json.loads(manifest_bytes)
    fields=['sdk_unicode_mapping_errors','sdk_text_objects_without_characters','source_text_characters_missing',
            'source_text_accounting_unavailable','source_unicode_scalars_unmatched',
            'source_unicode_accounting_unavailable','reading_order_requires_review']
    def inspect(executable,path):
        return json.loads(subprocess.run([str(executable),'--ir',str(path)],capture_output=True,check=True,timeout=180).stdout)
    def compare(entry):
        fetch(entry,ROOT/'corpus/cache',offline=True);path=ROOT/'corpus/cache'/(entry['id']+'.pdf')
        old=inspect(baseline,path);new=inspect(binary,path)
        if len(old['pages'])!=entry['pages'] or len(new['pages'])!=entry['pages']:raise ValueError('Page count differs')
        rows=[]
        for i,(a,b) in enumerate(zip(old['pages'],new['pages']),1):
            before,after=sequence(a),sequence(b)
            diagnostic={k:[a.get(k),b.get(k)] for k in fields if a.get(k)!=b.get(k)}
            old_text='\n'.join(x['text'] for x in a['blocks']);new_text='\n'.join(x['text'] for x in b['blocks'])
            if old_text!=new_text or len(a['blocks'])!=len(b['blocks']) or diagnostic:
                rows.append(dict(page=i,old_blocks=len(a['blocks']),new_blocks=len(b['blocks']),
                    text_changed=old_text!=new_text,ordered_scalars_changed=before!=after,diagnostic_changes=diagnostic,
                    old_ordered_sha256=hashlib.sha256(before.encode()).hexdigest(),new_ordered_sha256=hashlib.sha256(after.encode()).hexdigest()))
        return dict(id=entry['id'],source_sha256=entry['sha256'],pages=entry['pages'],changes=rows)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:results=list(pool.map(compare,manifest['documents']))
    rows=[r for d in results for r in d['changes']]
    report=dict(documents=len(results),pages=sum(d['pages'] for d in results),changed_pages=len(rows),
        changed_ordered_scalar_pages=sum(r['ordered_scalars_changed'] for r in rows),
        changed_diagnostic_pages=sum(bool(r['diagnostic_changes']) for r in rows),
        old_blocks_on_changed_pages=sum(r['old_blocks'] for r in rows),new_blocks_on_changed_pages=sum(r['new_blocks'] for r in rows),
        results=[d for d in results if d['changes']],
        binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),baseline_sha256=hashlib.sha256(baseline.read_bytes()).hexdigest(),
        manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),source_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        scope='Native before/after ordered scalar sequence after NFC and whitespace removal, plus existing diagnostics. Unchanged output does not establish Unicode accuracy or whole-document diff precision/recall.')
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ['documents','pages','changed_pages','changed_ordered_scalar_pages','changed_diagnostic_pages']},ensure_ascii=False))
    return 3 if report['changed_ordered_scalar_pages'] or report['changed_diagnostic_pages'] else 0


if __name__=='__main__':raise SystemExit(main())
