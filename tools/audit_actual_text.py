#!/usr/bin/env python3
"""Source-declared ActualText qualification against frozen v33 and public SDK.

Pypdf reads ownership and text operators independently. Raw SDK totals are
retained separately from semantic output: declared substitutions are not
character-conservation failures, and parser agreement is not exhaustive gold.
"""
import argparse, concurrent.futures, ctypes as C, hashlib, io, json, subprocess
from collections import Counter, defaultdict
from pathlib import Path
from pypdf import PdfReader, PdfWriter, __version__ as pypdf_version
from pypdf.generic import (ContentStream, TextStringObject, ByteStringObject,
                          create_string_object, NameObject)
from pdf_corpus import ROOT, fetch
from audit_form_discovery import library_path
from audit_source_unicode import actual_source, structure_actual_source

DIAGNOSTICS = ('sdk_text_objects_without_characters', 'source_text_characters_missing',
 'source_text_accounting_unavailable', 'source_unicode_scalars_unmatched',
 'source_unicode_accounting_unavailable', 'reading_order_requires_review')

def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def run(binary, *args, exits=(0,)):
    p=subprocess.run([str(binary), *map(str,args)],capture_output=True,timeout=180)
    assert p.returncode in exits,(p.returncode,p.stderr[-2000:])
    return p.returncode,json.loads(p.stdout)
def payload(ir):
    d=json.loads(json.dumps(ir))
    for p in d['pages']:
        for k in DIAGNOSTICS:p.pop(k,None)
    return d

def structure(reader):
    """Independent original-source ownership; supported corpus groups only."""
    root=reader.trailer['/Root'].get('/StructTreeRoot');out=[];seen=set()
    if not root:return {}
    def ref(o):return getattr(o,'idnum',None)
    def walk(o,page=None,parent=None,active=None):
        indirect=ref(o)
        if indirect is not None:
            assert indirect not in seen
            seen.add(indirect)
        o=o.get_object() if hasattr(o,'get_object') else o
        if isinstance(o,list):
            for x in o:walk(x,page,parent,active)
        elif isinstance(o,int):
            if active is not None:active['refs'].append((page,page,int(o),parent))
        elif isinstance(o,dict):
            page=ref(o.get('/Pg')) or page
            if '/ActualText' in o:
                assert active is None and isinstance(o['/ActualText'],str)
                active=dict(text=str(o['/ActualText']),refs=[]);out.append(active)
            if active is not None and o.get('/Type')=='/MCR':
                active['refs'].append((page,ref(o.get('/Stm')) or page,int(o['/MCID']),parent));return
            if '/K' in o:walk(o.get('/K'),page,indirect,active)
    walk(root)
    if not out:return {}
    nums={}
    def parents(o):
        o=o.get_object()
        for k,v in zip(o.get('/Nums',[])[::2],o.get('/Nums',[])[1::2]):
            assert int(k) not in nums;nums[int(k)]=v.get_object()
        for k in o.get('/Kids',[]):parents(k)
    parents(root.get_object()['/ParentTree'])
    pages={p.indirect_reference.idnum:p for p in reader.pages};result=defaultdict(dict)
    for g in out:
        assert g['refs'] and len({r[0] for r in g['refs']})==1
        for page,owner,mcid,leaf in g['refs']:
            assert page in pages
            obj=reader.get_object(owner);key=int(obj['/StructParents'])
            assert ref(nums[key][mcid])==leaf
            assert mcid not in result[page]
            result[page][mcid]=dict(owner=owner,text=g['text'],leaf=leaf)
    return dict(result)

def minimum(reader,page,groups):
    """Original glyph lower bound excluding declared replacement bodies."""
    ids=groups.get(page.indirect_reference.idnum,{})
    def width(font):
        font=font.get_object();kind=font['/Subtype']
        if kind in ['/Type1','/MMType1','/TrueType','/Type3']:return 1
        assert kind=='/Type0'
        return 2 if font['/Encoding'] in ['/Identity-H','/Identity-V'] else 4
    def visit(content,resources,font=None,active=False,depth=0):
        assert depth<32
        if content is None:return 0
        ops=ContentStream(content,reader).operations;graphics=[];marked=[];total=0
        def shown(value):
            if active:return 0
            data=bytes(value) if isinstance(value,ByteStringObject) else value.original_bytes
            if not data:return 0
            assert font is not None and (font!=2 or len(data)%2==0)
            return (len(data)+font-1)//font
        for args,op in ops:
            if op==b'Tf':font=width(resources['/Font'][args[0]])
            elif op==b'gs':
                state=resources['/ExtGState'][args[0]].get_object()
                if '/Font' in state:font=width(state['/Font'][0])
            elif op==b'q':graphics.append(font)
            elif op==b'Q':font=graphics.pop()
            elif op in [b'Tj',b"'",b'"']:total+=shown(args[-1])
            elif op==b'TJ':total+=sum(shown(x) for x in args[0] if isinstance(x,(TextStringObject,ByteStringObject)))
            elif op in [b'BMC',b'BDC']:
                marked.append(active)
                if op==b'BDC':
                    d=args[1]
                    if not isinstance(d,dict):d=resources['/Properties'][d].get_object()
                    active=active or '/ActualText' in d or d.get('/MCID') in ids
            elif op==b'EMC':active=marked.pop()
            elif op==b'Do':
                obj=resources['/XObject'][args[0]].get_object()
                if obj['/Subtype']=='/Form':total+=visit(obj,obj.get('/Resources',resources),font,active,depth+1)
            elif op in [b'scn',b'SCN'] and any(str(x).startswith('/') for x in args):raise ValueError('pattern program')
        assert not marked
        return total
    return visit(page.get_contents(),page.get('/Resources',{}))

def sdk(library,path,groups):
    """Public SDK records outside source replacements and unobserved objects."""
    b=C.CDLL(str(library));P,I=C.c_void_p,C.c_int;U=C.c_ulong
    def api(n,r,a):
        f=getattr(b,n);f.restype=r;f.argtypes=a;return f
    init=api('FPDF_InitLibrary',None,[]);end=api('FPDF_DestroyLibrary',None,[])
    load=api('FPDF_LoadMemDocument64',P,[P,C.c_size_t,C.c_char_p]);close=api('FPDF_CloseDocument',None,[P])
    npage=api('FPDF_GetPageCount',I,[P]);pagefn=api('FPDF_LoadPage',P,[P,I]);closepage=api('FPDF_ClosePage',None,[P]);rot=api('FPDFPage_SetRotation',None,[P,I])
    textfn=api('FPDFText_LoadPage',P,[P]);closetext=api('FPDFText_ClosePage',None,[P]);count=api('FPDFText_CountChars',I,[P]);gen=api('FPDFText_IsGenerated',I,[P,I]);charobj=api('FPDFText_GetTextObject',P,[P,I])
    objects=api('FPDFPage_CountObjects',I,[P]);objfn=api('FPDFPage_GetObject',P,[P,I]);kind=api('FPDFPageObj_GetType',I,[P]);forms=api('FPDFFormObj_CountObjects',I,[P]);formobj=api('FPDFFormObj_GetObject',P,[P,U])
    marks=api('FPDFPageObj_CountMarks',I,[P]);markfn=api('FPDFPageObj_GetMark',P,[P,U]);mcidfn=api('FPDFPageObjMark_GetParamIntValue',I,[P,C.c_char_p,C.POINTER(I)])
    typ=api('FPDFPageObjMark_GetParamValueType',I,[P,C.c_char_p]);blob=api('FPDFPageObjMark_GetParamBlobValue',I,[P,C.c_char_p,P,U,C.POINTER(U)])
    data=Path(path).read_bytes();buf=C.create_string_buffer(data);init();doc=None;rows=[]
    try:
        doc=load(buf,len(data),None);assert doc
        for n,ids in enumerate(groups):
            p=pagefn(doc,n);assert p;t=None
            try:
                rot(p,0);info={};unobserved=set();covered=set();blank_declared=0
                def walk(obj,replaced=False,actual=False,blank=True,depth=0):
                    nonlocal blank_declared
                    assert obj and depth<=32 and len(info)<200_000
                    m=marks(obj);assert 0<=m<=64
                    for j in range(m):
                        mark=markfn(obj,j);ident=I()
                        if mcidfn(mark,b'MCID',C.byref(ident)) and ident.value in ids:replaced=True
                        if typ(mark,b'ActualText'):
                            actual=True;size=U();known=False
                            if blob(mark,b'ActualText',None,0,C.byref(size)) and size.value<=4096:
                                raw=C.create_string_buffer(size.value);read=U()
                                assert blob(mark,b'ActualText',raw,size.value,C.byref(read)) and read.value==size.value
                                try:
                                    value=create_string_object(raw.raw)
                                    known=isinstance(value,str) and not value.strip()
                                except Exception:pass
                            blank &= known
                    info[obj]=(replaced,actual)
                    ty=kind(obj)
                    if ty==1:
                        unobserved.add(obj)
                        if replaced or actual and blank:covered.add(obj);blank_declared+=int(actual and blank)
                    if ty==5:
                        length=forms(obj);assert 0<=length<=200_000
                        for j in range(length):walk(formobj(obj,j),replaced,actual,blank,depth+1)
                length=objects(p);assert 0<=length<=200_000
                for j in range(length):walk(objfn(p,j))
                t=textfn(p);assert t;c=count(t);outside=0;nongenerated=0
                assert 0<=c<=2_000_000
                for j in range(c):
                    obj=charobj(t,j);unobserved.discard(obj);g=gen(t,j);assert g in (0,1)
                    if not g:
                        nongenerated+=1
                        if not any(info.get(obj,(False,False))):outside+=1
                rows.append(dict(page=n+1,records=c,nongenerated=nongenerated,outside_replacements=outside,raw_unobserved=len(unobserved),source_represented_unobserved=len(unobserved&covered),unobserved=len(unobserved-covered),declared_blank_objects=blank_declared))
            finally:
                if t:closetext(t)
                closepage(p)
    finally:
        if doc:close(doc)
        end()
    return rows

def main():
    a=argparse.ArgumentParser(description=__doc__)
    for n in ['binary','baseline','report']:a.add_argument('--'+n,type=Path,required=True)
    args=a.parse_args();args.binary=args.binary.resolve();args.baseline=args.baseline.resolve()
    manifest_path=ROOT/'corpus/manifest.json';m=json.loads(manifest_path.read_bytes());library,library_sha=library_path()
    def inspect(e):
        fetch(e,ROOT/'corpus/cache',offline=True)
        path=ROOT/'corpus/cache'/(e['id']+'.pdf')
        _,old=run(args.baseline,'--ir',path);_,new=run(args.binary,'--ir',path)
        if e['id']!='pdfa-actualtext-graphics':assert payload(old)==payload(new),(e['id'],'changed original payload')
        else:
            assert '\n'.join(b['text'] for b in old['pages'][0]['blocks'])=='This is the'
            assert '\n'.join(b['text'] for b in new['pages'][0]['blocks'])==e['full_page_text']
            _,layer=run(args.binary,'--text-layer',path,'1')
            region=[r for r in layer if r.get('source_text_region')];assert len(region)==1 and region[0]['text']=='paragraph.'
            assert all(c['x0']==region[0]['bounds']['x0'] and c['x1']==region[0]['bounds']['x1'] for c in region[0]['chars'])
            assert new['pages'][0]['reading_order_requires_review']
        delta=[]
        for before,after in zip(old['pages'],new['pages']):
            changes={k:[before.get(k,False if k.endswith(('unavailable','review')) else 0),after.get(k,False if k.endswith(('unavailable','review')) else 0)] for k in DIAGNOSTICS if before.get(k,False if k.endswith(('unavailable','review')) else 0)!=after.get(k,False if k.endswith(('unavailable','review')) else 0)}
            if changes:delta.append(dict(page=after['index'],changes=changes))
        return dict(id=e['id'],source_sha256=e['sha256'],pages=new['pages'],retained_payload=e['id']!='pdfa-actualtext-graphics',diagnostic_changes=delta)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:rows=list(pool.map(inspect,m['documents']))
    checks=[];inventories=[];structures=[]
    for e,row in zip(m['documents'],rows):
        path=ROOT/'corpus/cache'/(e['id']+'.pdf');reader=PdfReader(path);groups=structure(reader)
        raw=sdk(library,path,[groups.get(p.indirect_reference.idnum,{}) for p in reader.pages])
        structures.extend(dict(id=e['id'],page_object=page,groups=ids) for page,ids in groups.items())
        for p,facts,observed in zip(reader.pages,row['pages'],raw):
            if float(p.get('/UserUnit',1))!=1:continue
            assert observed['unobserved']==facts.get('sdk_text_objects_without_characters',0),(e['id'],observed,facts.get('sdk_text_objects_without_characters',0))
            inventories.append(dict(id=e['id'],**observed))
            if facts.get('source_text_accounting_unavailable',False):continue
            try:lower=minimum(reader,p,groups)
            except (AssertionError,KeyError,ValueError,IndexError):continue
            expected=max(0,lower-observed['outside_replacements'])
            assert expected==facts.get('source_text_characters_missing',0),(e['id'],observed['page'],lower,observed,expected,facts.get('source_text_characters_missing',0))
            checks.append(dict(id=e['id'],page=observed['page'],minimum=lower,sdk_outside=observed['outside_replacements'],deficit=expected))
    target=ROOT/'corpus/wasm-runtime/actual-structure-probes';target.mkdir(exist_ok=True);controls=[]
    for name,old,new in [('structure',structure_actual_source('A'),structure_actual_source('B')),('marked-empty',actual_source(False),actual_source(True,True))]:
        paths=[]
        for label,data in [('old',old),('new',new)]:
            p=target/(name+'-'+label+'.pdf');p.write_bytes(data);paths.append(p)
        previous,old_diff=run(args.baseline,*paths,exits=((0,) if name=='structure' else (3,)))
        current,diff=run(args.binary,*paths,exits=(1,));_,ir=run(args.binary,'--ir',paths[1]);_,layer=run(args.binary,'--text-layer',paths[1],'1')
        assert not any(ir['pages'][0].get(k) for k in DIAGNOSTICS)
        text=''.join(b['text'] for b in ir['pages'][0]['blocks']);assert text==('B' if name=='structure' else '')
        if name=='structure':assert diff['summary']['blocks_changed']==1 and layer[0]['source_text_region']
        else:assert layer==[] and diff['summary']['pages_added']+diff['summary']['pages_removed']+diff['summary']['blocks_removed']>0
        controls.append(dict(id=name,source_sha256=[sha(p) for p in paths],baseline_exit=previous,current_exit=current,baseline_summary=old_diff['summary'],current_summary=diff['summary'],current_text=text))
    result=dict(version=1,manifest_sha256=sha(manifest_path),binary_sha256=sha(args.binary),baseline_sha256=sha(args.baseline),sdk_library_sha256=library_sha,pypdf_version=pypdf_version,documents=len(rows),pages=sum(len(r['pages']) for r in rows),retained_original_payloads=sum(r['retained_payload'] for r in rows),independent_source_count_checks=len(checks),independent_object_inventory_checks=len(inventories),source_structure_groups=structures,controlled_regressions=controls,source_count_checks=checks,sdk_inventories=inventories,rows=rows,methodology='Source semantic glyph bodies exclude explicitly declared ActualText. Independent pypdf operators and public SDK object marks/counts are compared only where units and supported syntax permit. Raw SDK inventories remain separate from recovered source text; no exhaustive source order/geometry claim.')
    args.report.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if isinstance(v,int)}),flush=True)
if __name__=='__main__':main()
