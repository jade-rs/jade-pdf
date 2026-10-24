#!/usr/bin/env python3
"""Qualify official AGL mappings without treating decoder aliases as source gold.

Original text, geometry and order must remain exact against v35. Pypdf reads
used simple-font Differences independently; names absent from AGL/its naming
rules require source uncertainty. This deliberately does not recover Unicode
from nonstandard names, or certify full-page reading order/Unicode.
"""
import argparse
from collections import Counter
import concurrent.futures
import json
from pathlib import Path
from pypdf import PdfReader, __version__ as pypdf_version
from pypdf.generic import ContentStream, ByteStringObject
from audit_actual_text import DIAGNOSTICS, payload, run, sha, structure
from pdf_corpus import ROOT, fetch


def original_names(reader, page, actual_ids):
    used = Counter()

    def visit(content, resources, font=None, actual=False, depth=0):
        assert depth <= 32
        if content is None: return
        graphics, marks = [], []

        def show(value):
            if actual or font is None or '/ToUnicode' in font: return
            if font.get('/Subtype') not in ['/Type1','/MMType1','/TrueType','/Type3']: return
            encoding=font.get('/Encoding',{})
            if hasattr(encoding,'get_object'): encoding=encoding.get_object()
            if not isinstance(encoding,dict): return
            differences=encoding.get('/Differences',[])
            if hasattr(differences,'get_object'): differences=differences.get_object()
            table={};index=None
            for item in differences:
                if isinstance(item,int): index=int(item)
                else:
                    assert index is not None and 0<=index<256
                    table[index]=str(item).removeprefix('/');index+=1
            raw=bytes(value) if isinstance(value,ByteStringObject) else value.original_bytes
            for byte in raw:
                if byte in table: used[(str(font.get('/BaseFont','')),table[byte])] += 1
        for args,op in ContentStream(content,reader).operations:
            if op==b'Tf':font=resources['/Font'][args[0]].get_object()
            elif op==b'gs':
                state=resources['/ExtGState'][args[0]].get_object()
                if '/Font' in state:font=state['/Font'][0].get_object()
            elif op==b'q':graphics.append(font)
            elif op==b'Q':font=graphics.pop()
            elif op in [b'BMC',b'BDC']:
                marks.append(actual)
                if op==b'BDC':
                    d=args[1];d=d if isinstance(d,dict) else resources['/Properties'][d].get_object()
                    actual=actual or '/ActualText' in d or d.get('/MCID') in actual_ids
            elif op==b'EMC':actual=marks.pop()
            elif op in [b'Tj',b"'",b'"']:show(args[-1])
            elif op==b'TJ':
                for value in args[0]:
                    if isinstance(value,(str,bytes)):show(value)
            elif op==b'Do':
                obj=resources['/XObject'][args[0]].get_object()
                if obj['/Subtype']=='/Form':visit(obj,obj.get('/Resources',resources),font,actual,depth+1)
        assert not marks
    visit(page.get_contents(),page.get('/Resources',{}))
    return used


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for n in ['binary','baseline','marked-evidence','fixtures-dir','report']:
        parser.add_argument('--'+n,type=Path,required=True)
    args=parser.parse_args()
    resource=ROOT/'crates/jade-pdf-diff/resources/adobe-glyphlist.txt'
    pin=json.loads(resource.with_suffix('.json').read_bytes())
    assert sha(resource)==pin['sha256']=='a3b2f61ced9f3644cc0d4ecde5c59df34ca286c689d9484a43a710a81c466789'
    table=dict(line.split(';') for line in resource.read_text().splitlines() if line and not line.startswith('#'))
    assert len(table)==4281

    def decode_component(name):
        if name in table:return ''.join(chr(int(s,16)) for s in table[name].split())
        prefix=3 if name.startswith('uni') else 1 if name.startswith('u') else 0
        if not prefix:return None
        text=name[prefix:]
        if not text or any(c not in '0123456789ABCDEF' for c in text):return None
        if prefix==3:
            if len(text)%4:return None
            values=[int(text[i:i+4],16) for i in range(0,len(text),4)]
        else:
            if not 4<=len(text)<=6:return None
            values=[int(text,16)]
        if any(v>0x10ffff or 0xd800<=v<=0xdfff for v in values):return None
        return ''.join(chr(v) for v in values)

    def decode(name):
        parts=[decode_component(n) for n in name.split('.',1)[0].split('_')]
        return ''.join(parts) if all(x is not None for x in parts) else None

    manifest=ROOT/'corpus/manifest.json';m=json.loads(manifest.read_bytes())
    evidence=json.loads(args.marked_evidence.read_bytes())
    assert evidence['binary_sha256']==sha(args.binary) and evidence['manifest_sha256']==sha(manifest)

    def inspect(entry):
        fetch(entry,ROOT/'corpus/cache',offline=True)
        path=ROOT/'corpus/cache'/(entry['id']+'.pdf')
        _,before=run(args.baseline,'--ir',path);_,after=run(args.binary,'--ir',path)
        assert payload(before)==payload(after),(entry['id'],'text/geometry/order changed')
        source=next(x for x in evidence['rows'] if x['id']==entry['id'])
        assert after['pages']==source['pages']
        changes=[];unknown=[];prefix_names=[]
        reader=PdfReader(path);groups=structure(reader)
        for a,b,page in zip(before['pages'],after['pages'],reader.pages):
            delta={k:[a.get(k,0),b.get(k,0)] for k in DIAGNOSTICS if a.get(k,0)!=b.get(k,0)}
            if delta:
                assert set(delta)<= {'source_unicode_scalars_unmatched','source_unicode_accounting_unavailable'},(entry['id'],b['index'],delta)
                changes.append(dict(page=b['index'],diagnostics=delta,blocks_changed=False,added_scalars={},removed_scalars={}))
            ids=groups.get(page.indirect_reference.idnum,{})
            try:names=original_names(reader,page,ids)
            except (AssertionError,KeyError,ValueError,IndexError):
                # Unsupported existing content is not new AGL proof.
                assert not delta,(entry['id'],b['index'],'changed unsupported source')
                continue
            for (font,name),count in names.items():
                value=decode(name)
                if value is None:
                    assert b.get('source_unicode_accounting_unavailable',False),(entry['id'],b['index'],font,name)
                    unknown.append(dict(page=b['index'],font=font,glyph_name=name,encoded_occurrences=count))
                elif name.startswith('u') and name in table:
                    prefix_names.append(dict(page=b['index'],font=font,glyph_name=name,source_unicode=value,encoded_occurrences=count))
        return dict(id=entry['id'],source_sha256=entry['sha256'],pages=after['pages'],retained_payload=True,changes=changes,unknown_source_names=unknown,known_prefixed_names=prefix_names)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        rows=list(pool.map(inspect,m['documents']))
    controls=[]
    gold={'union':'∪','universal':'∀','uacute':'ú','ugrave':'ù','nonbmp':'𐐌','multicomponent':'Ļ€̈𐐌','alias':None,'surrogate':None,'lowercase':None,'mixed_unknown':None}
    for mode,expected in gold.items():
        old=args.fixtures_dir/('agl_'+mode+'-false.pdf');new=args.fixtures_dir/('agl_'+mode+'-true.pdf')
        reader=PdfReader(new);font=reader.pages[0]['/Resources']['/Font']['/F1'];name=str(font['/Encoding']['/Differences'][1])[1:]
        assert decode(name)==expected
        _,before=run(args.baseline,'--ir',new);_,after=run(args.binary,'--ir',new);a,b=before['pages'][0],after['pages'][0]
        assert a['blocks']==b['blocks'] and b.get('source_unicode_accounting_unavailable',False)==(expected is None)
        code,diff=run(args.binary,old,new,exits=(1,3));previous,old_diff=run(args.baseline,old,new,exits=(1,3))
        if mode=='multicomponent':assert b['source_unicode_scalars_unmatched']==6 and code==3
        elif expected is not None:
            assert '\n'.join(r['text'] for r in b['blocks'])==expected and b.get('source_unicode_scalars_unmatched',0)==0 and code==1
        else:assert code==3
        assert diff['summary']['pages_visual_changed']==0
        controls.append(dict(id=mode,source_sha256=[sha(old),sha(new)],glyph_name=name,declared_unicode=expected,baseline_exit=previous,current_exit=code,baseline_diagnostics={k:a.get(k,0) for k in DIAGNOSTICS},current_diagnostics={k:b.get(k,0) for k in DIAGNOSTICS},summary=diff['summary']))
    result=dict(version=1,tool_sha256=sha(__file__),manifest_sha256=sha(manifest),binary_sha256=sha(args.binary),baseline_sha256=sha(args.baseline),pypdf_version=pypdf_version,adobe_glyphlist=pin,known_names=4281,marked_scope_evidence_sha256=sha(args.marked_evidence),documents=len(rows),pages=sum(len(r['pages']) for r in rows),retained_original_payloads=len(rows),diagnostic_changed_documents=sum(bool(r['changes']) for r in rows),diagnostic_changed_pages=sum(len(r['changes']) for r in rows),unknown_source_names=sum(len(r['unknown_source_names']) for r in rows),controlled_cases=len(controls),controlled_results=controls,rows=rows,sdk_inventories=evidence['sdk_inventories'],source_count_checks=evidence['source_count_checks'],methodology=__doc__,policy='AGL entries precede uni/u rules; uppercase hexadecimal and BMP-only uni components. Unknown components require uncertainty instead of being silently dropped. Nonstandard aliases are not original Unicode gold. Zapf-specific naming is not added by this profile.')
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if isinstance(v,int)}),flush=True)


if __name__=='__main__':main()
