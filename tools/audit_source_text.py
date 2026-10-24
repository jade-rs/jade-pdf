#!/usr/bin/env python3
"""Check source-glyph lower bounds with pypdf and independent public SDK reads.

Lower bounds are not original Unicode gold: ligature/ActualText expansion or
surplus records can conceal omissions. A separate expansion-masked source probe
records that limitation. Existing text, geometry and order must match frozen v31
exactly after removing only the two added diagnostic fields.
"""
import argparse
from collections import Counter
import ctypes as C
import hashlib
import json
from pathlib import Path
import subprocess
import concurrent.futures
from pypdf import PdfReader, __version__ as pypdf_version
from pypdf.generic import ContentStream, ByteStringObject, TextStringObject
from pdf_corpus import ROOT, fetch
from audit_form_discovery import library_path
from audit_text_objects import document, stream

FIELDS = ('source_text_characters_missing', 'source_text_accounting_unavailable')
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def run(exe,*args,exits=(0,)):
    p=subprocess.run([str(exe),*map(str,args)],capture_output=True,timeout=180)
    assert p.returncode in exits,p.stderr[-2000:]
    return p.returncode,json.loads(p.stdout)
def stripped(ir):
    result=json.loads(json.dumps(ir))
    for p in result['pages']:
        for field in FIELDS:p.pop(field,None)
    return result

def sdk_counts(library,source):
    b=C.CDLL(str(library));P=C.c_void_p;I=C.c_int
    def api(name,result,args):
        f=getattr(b,name);f.restype=result;f.argtypes=args;return f
    init=api('FPDF_InitLibrary',None,[]);destroy=api('FPDF_DestroyLibrary',None,[])
    load=api('FPDF_LoadMemDocument64',P,[P,C.c_size_t,C.c_char_p]);close=api('FPDF_CloseDocument',None,[P])
    pages=api('FPDF_GetPageCount',I,[P]);page=api('FPDF_LoadPage',P,[P,I]);close_page=api('FPDF_ClosePage',None,[P])
    rotation=api('FPDFPage_SetRotation',None,[P,I]);text=api('FPDFText_LoadPage',P,[P]);close_text=api('FPDFText_ClosePage',None,[P])
    count=api('FPDFText_CountChars',I,[P]);generated=api('FPDFText_IsGenerated',I,[P,I])
    data=Path(source).read_bytes();buf=C.create_string_buffer(data);rows=[];doc=None;init()
    try:
        doc=load(buf,len(data),None);assert doc
        for n in range(pages(doc)):
            p=page(doc,n);t=None;assert p
            try:
                rotation(p,0);t=text(p);assert t;c=count(t);assert 0<=c<=2_000_000
                flags=[generated(t,i) for i in range(c)];assert all(x in (0,1) for x in flags)
                rows.append(dict(page=n+1,sdk_records=c,nongenerated_records=flags.count(0),generated_records=flags.count(1)))
            finally:
                if t:close_text(t)
                close_page(p)
    finally:
        if doc:close(doc)
        destroy()
    return rows

def source_minimum(reader,page):
    # Independent parser/object model; no SDK text string is used as source gold.
    def width(font):
        font=font.get_object();kind=font['/Subtype']
        if kind in ['/Type1','/MMType1','/TrueType','/Type3']:return 1
        assert kind=='/Type0',kind
        return 2 if font['/Encoding'] in ['/Identity-H','/Identity-V'] else 4
    def visit(content,resources,current=None,depth=0):
        assert depth<32
        if content is None:return 0
        ops=content.operations if hasattr(content,'operations') else ContentStream(content,reader).operations
        stack=[];total=0
        def shown(value):
            data=bytes(value) if isinstance(value,ByteStringObject) else value.original_bytes
            if not data:return 0
            assert current is not None
            assert current!=2 or len(data)%2==0
            return (len(data)+current-1)//current
        for args,op in ops:
            if op==b'Tf':current=width(resources['/Font'][args[0]])
            elif op==b'gs':
                state=resources['/ExtGState'][args[0]].get_object()
                if '/Font' in state:current=width(state['/Font'][0])
            elif op==b'q':stack.append(current)
            elif op==b'Q':current=stack.pop()
            elif op in [b'Tj',b"'",b'"']:total+=shown(args[-1])
            elif op==b'TJ':total+=sum(shown(x) for x in args[0] if isinstance(x,(TextStringObject,ByteStringObject)))
            elif op==b'Do':
                obj=resources['/XObject'][args[0]].get_object()
                if obj['/Subtype']=='/Form':total+=visit(obj,obj.get('/Resources',resources),current,depth+1)
            elif op in [b'scn',b'SCN'] and any(str(x).startswith('/') for x in args):raise ValueError('pattern programs not accounted')
        return total
    return visit(page.get_contents(),page.get('/Resources',{}))

def masked_source(revised):
    # The A glyph expands to two Unicode Cs; a surplus record masks the lost C.
    cmap='/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /Masked def /CMapType 2 def 1 begincodespacerange <00> <ff> endcodespacerange 2 beginbfchar <41> <00430043> <43> <0043> endbfchar endcmap CMapName currentdict /CMap defineresource pop end end'
    body='BT /F1 12 Tf 3 Tr 30 200 Td (A) Tj ET BT /F1 12 Tf 3 Tr 30 150 Td '+('[(C) 722 (C)] TJ' if revised else '(C) Tj')+' ET'
    return document(['<< /Type /Catalog /Pages 2 0 R >>','<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
        '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',
        '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 6 0 R >>',stream(body),stream(cmap)])

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',type=Path,required=True);p.add_argument('--baseline',type=Path,required=True);p.add_argument('--report',type=Path,required=True);args=p.parse_args()
    args.binary=args.binary.resolve();args.baseline=args.baseline.resolve()
    manifest=json.loads((ROOT/'corpus/manifest.json').read_bytes());library,sdk_sha=library_path()
    def inspect(entry):
        fetch(entry,ROOT/'corpus/cache',offline=True);source=ROOT/'corpus/cache'/(entry['id']+'.pdf')
        _,old=run(args.baseline,'--ir',source);_,new=run(args.binary,'--ir',source)
        assert stripped(new)==old,(entry['id'],'text/geometry/old diagnostics changed')
        reader=PdfReader(source);counts=sdk_counts(library,source);rows=[]
        for page,facts,raw in zip(reader.pages,new['pages'],counts):
            row=dict(page=facts['index'],**{field:facts.get(field,False if field.endswith('unavailable') else 0) for field in FIELDS},**{k:v for k,v in raw.items() if k!='page'})
            try:
                minimum=source_minimum(reader,page);row['pypdf_source_minimum']=minimum
                unit=float(page.get('/UserUnit',1));row['raw_sdk_unit']=unit
                # SDK physical normalization can change whitespace flags on a
                # nondefault unit. This independent raw oracle does not rebase it.
                if unit==1 and not row['source_text_accounting_unavailable']:
                    expected=max(0,minimum-raw['nongenerated_records'])
                    assert expected==row['source_text_characters_missing'],(entry['id'],facts['index'],expected,row)
                    row['independent_deficit_check']=True
            except (AssertionError,KeyError,ValueError,IndexError) as error:
                if isinstance(error,AssertionError) and error.args and isinstance(error.args[0],tuple):raise
                row['oracle_unavailable']=str(error)[:200]
            rows.append(row)
        print('PASS '+entry['id'],flush=True)
        return dict(id=entry['id'],source_sha256=entry['sha256'],pages=rows,retained_ir_exact=True)
    # SDK lifecycle is process global. Keep each SDK inventory sequential.
    rows=[inspect(entry) for entry in manifest['documents']]
    probes=[]
    for label,old,new in [('partial-object-duplicate',ROOT/'corpus/wasm-runtime/text-object-source-probes/partial-old.pdf',ROOT/'corpus/wasm-runtime/text-object-source-probes/partial-new.pdf')]:
        before,old_diff=run(args.baseline,old,new,exits=(0,));after,new_diff=run(args.binary,old,new,exits=(3,))
        assert old_diff['summary']==new_diff['summary'] and all(x==0 for x in new_diff['summary'].values())
        assert new_diff['diagnostics']['new_pages_with_missing_source_characters']==1
        probes.append(dict(id=label,status='review_detected',baseline_exit=before,current_exit=after,source_sha256=[sha(old),sha(new)],diagnostics=new_diff['diagnostics']))
    target=ROOT/'corpus/wasm-runtime/source-accounting-probes';target.mkdir(exist_ok=True)
    old=target/'expansion-masked-old.pdf';new=target/'expansion-masked-new.pdf';old.write_bytes(masked_source(False));new.write_bytes(masked_source(True))
    code,diff=run(args.binary,old,new,exits=(0,3))
    masked=[]
    for path in [old,new]:
        reader=PdfReader(path);_,ir=run(args.binary,'--ir',path)
        masked.append(dict(file=str(path.relative_to(ROOT/'corpus')),sha256=sha(path),source_minimum=source_minimum(reader,reader.pages[0]),source_pypdf_text=reader.pages[0].extract_text(),ir=ir,sdk=sdk_counts(library,path)))
    probes.append(dict(id='expansion-masked-partial-duplicate',status='known_lower_bound_limit',exit_code=code,diagnostics=diff['diagnostics'],summary=diff['summary'],source=masked))
    pages=[x for doc in rows for x in doc['pages']]
    report=dict(version=1,methodology=__doc__,tool_sha256=sha(__file__),manifest_sha256=sha(ROOT/'corpus/manifest.json'),binary_sha256=sha(args.binary),baseline_binary_sha256=sha(args.baseline),sdk_sha256=sdk_sha,pypdf_version=pypdf_version,documents=len(rows),pages=len(pages),retained_ir_documents=len(rows),independent_deficit_page_checks=sum(x.get('independent_deficit_check',False) for x in pages),missing_source_glyph_lower_bound=sum(x['source_text_characters_missing'] for x in pages),missing_source_review_pages=sum(x['source_text_characters_missing']>0 for x in pages),unavailable_source_accounting_pages=sum(x['source_text_accounting_unavailable'] for x in pages),results=rows,source_scope_probes=probes,
        limits=['No count equality establishes original Unicode or source exhaustiveness.','Generic composite CMaps use only a four-byte-per-code lower bound.','Patterns, unsupported syntax and resource/work limits require review.','The expansion-masked source control remains an open source-exhaustiveness case.'])
    args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if isinstance(v,int)}),flush=True)
if __name__=='__main__':main()
