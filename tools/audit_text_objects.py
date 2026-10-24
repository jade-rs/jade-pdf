#!/usr/bin/env python3
"""Audit SDK object/character coverage without inventing omitted source text.

Raw pinned SDK handles independently establish only whole-object non-observation,
not original text completeness or partially omitted characters within an observed
object. Source ToUnicode probes are separately inspected with pypdf. Ordinary
whitespace objects remain conservatively reviewable until source absence can be
proved. Current versus frozen v28 IR must match after removing only the new
diagnostic; no text/geometry/font/order delta is accepted by this audit.
"""
import argparse
import concurrent.futures
import ctypes as C
import hashlib
import json
from pathlib import Path
import subprocess
from pypdf import PdfReader
from pdf_corpus import ROOT, fetch
from audit_form_discovery import library_path

def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def sdk_inventory(library, source):
    b=C.CDLL(str(library))
    def api(name, result, args):
        f=getattr(b,name); f.restype=result; f.argtypes=args; return f
    P,I=C.c_void_p,C.c_int
    init=api("FPDF_InitLibrary",None,[]); end=api("FPDF_DestroyLibrary",None,[])
    load=api("FPDF_LoadMemDocument64",P,[P,C.c_size_t,C.c_char_p])
    page_count=api("FPDF_GetPageCount",I,[P]); load_page=api("FPDF_LoadPage",P,[P,I])
    load_text=api("FPDFText_LoadPage",P,[P]); count=api("FPDFText_CountChars",I,[P])
    char_object=api("FPDFText_GetTextObject",P,[P,I]); count_objects=api("FPDFPage_CountObjects",I,[P])
    get_object=api("FPDFPage_GetObject",P,[P,I]); kind=api("FPDFPageObj_GetType",I,[P])
    form_count=api("FPDFFormObj_CountObjects",I,[P]); form_object=api("FPDFFormObj_GetObject",P,[P,C.c_ulong])
    get_text=api("FPDFTextObj_GetText",C.c_ulong,[P,P,P,C.c_ulong])
    close_text=api("FPDFText_ClosePage",None,[P]); close_page=api("FPDF_ClosePage",None,[P])
    close_doc=api("FPDF_CloseDocument",None,[P])
    data=Path(source).read_bytes(); assert len(data)<=64*1024*1024
    buffer=C.create_string_buffer(data); rows=[]; work=0
    init(); doc=None
    try:
        doc=load(buffer,len(data),None); assert doc
        for number in range(page_count(doc)):
            page=load_page(doc,number); assert page
            text=None
            try:
                text=load_text(page); assert text
                characters=count(text); assert 0<=characters<=2_000_000
                represented={char_object(text,i) for i in range(characters)}
                total=count_objects(page); assert 0<=total<=200_000
                stack=[(get_object(page,i),0) for i in range(total)]
                seen=set(); missing=[]; all_text=0
                while stack:
                    obj,depth=stack.pop(); work+=1
                    assert obj and depth<=32 and work<=200_000
                    assert obj not in seen, "Repeated SDK object handle in one page tree"
                    seen.add(obj)
                    ty=kind(obj)
                    if ty==1:
                        all_text+=1
                        if obj not in represented:
                            missing.append(dict(depth=depth,text_api_bytes=get_text(obj,text,None,0)))
                    elif ty==5:
                        children=form_count(obj); assert 0<=children<=200_000-work
                        stack.extend((form_object(obj,i),depth+1) for i in range(children))
                rows.append(dict(page=number+1,sdk_characters=characters,text_objects=all_text,
                                 unobserved_text_objects=len(missing),unobserved=missing))
            finally:
                if text: close_text(text)
                close_page(page)
    finally:
        if doc: close_doc(doc)
        end()
    return rows

def run_ir(binary, source):
    p=subprocess.run([str(binary.resolve()),"--ir",str(source)],capture_output=True,timeout=180)
    assert p.returncode==0,p.stderr[-2000:]
    return json.loads(p.stdout)

def document(objects):
    data=b"%PDF-1.7\n"; offsets=[]
    for n,obj in enumerate(objects,1):
        offsets.append(len(data)); data+=f"{n} 0 obj\n{obj}\nendobj\n".encode()
    at=len(data)
    data+=f"xref\n0 {len(objects)+1}\n0000000000 65535 f \n".encode()
    data+="".join(f"{x:010} 00000 n \n" for x in offsets).encode()
    return data+f"trailer\n<< /Size {len(objects)+1} /Root 1 0 R >>\nstartxref\n{at}\n%%EOF\n".encode()

def stream(text, attrs=""):
    return f"<< /Length {len(text.encode())} {attrs} >>\nstream\n{text}\nendstream"

def probe_source(mapped, mixed, nested):
    cmap=f"/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /IndependentSource def /CMapType 2 def 1 begincodespacerange <00> <ff> endcodespacerange 2 beginbfchar <20> <{mapped}> <43> <0043> endbfchar endcmap CMapName currentdict /CMap defineresource pop end end"
    missing="BT /F1 12 Tf 30 200 Td ( ) Tj ET"
    content=("BT /F1 12 Tf 30 150 Td (C) Tj ET " if mixed else "")+("/Outer Do" if nested else missing)
    return document([
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> /XObject << /Outer 7 0 R >> >> /Contents 5 0 R >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 6 0 R >>",
        stream(content),stream(cmap),
        stream("/Inner Do","/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /XObject << /Inner 8 0 R >> >>"),
        stream(missing,"/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >>")])

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary",type=Path,required=True)
    parser.add_argument("--baseline",type=Path,required=True)
    parser.add_argument("--report",type=Path,required=True)
    args=parser.parse_args()
    manifest_bytes=(ROOT/"corpus/manifest.json").read_bytes()
    manifest=json.loads(manifest_bytes); library,sdk_sha=library_path()
    def compare(entry):
        fetch(entry,ROOT/"corpus/cache",offline=True)
        source=ROOT/"corpus/cache"/(entry["id"]+".pdf")
        previous=run_ir(args.baseline,source); current=run_ir(args.binary,source)
        stripped=json.loads(json.dumps(current))
        for page in stripped["pages"]: page.pop("sdk_text_objects_without_characters",None)
        assert stripped==previous,entry["id"]
        return entry, current
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        parsed=list(pool.map(compare,manifest["documents"]))
    rows=[]
    for entry,ir in parsed:
        source=ROOT/"corpus/cache"/(entry["id"]+".pdf")
        raw=sdk_inventory(library,source)
        assert len(raw)==len(ir["pages"])==entry["pages"]
        for independent,page in zip(raw,ir["pages"]):
            assert independent["unobserved_text_objects"]==page.get("sdk_text_objects_without_characters",0),entry["id"]
        row=dict(id=entry["id"],sha256=entry["sha256"],passed=True,
                 prior_ir_payload_exact=True,pages=raw,
                 review_pages=sum(p["unobserved_text_objects"]>0 for p in raw),
                 unobserved_objects=sum(p["unobserved_text_objects"] for p in raw))
        rows.append(row); print("PASS "+entry["id"],flush=True)
    directory=ROOT/"corpus/wasm-runtime/text-object-source-probes"; directory.mkdir(exist_ok=True)
    probes=[]
    for mixed in [False,True]:
        for nested in [False,True]:
            name=f"mixed-{int(mixed)}-nested-{int(nested)}"
            files=[]
            for suffix,code in [("old","D83DDE00"),("new","D83DDE01")]:
                file=directory/(name+"-"+suffix+".pdf")
                data=probe_source(code,mixed,nested)
                if file.exists():assert file.read_bytes()==data
                else:file.write_bytes(data)
                reader=PdfReader(file)
                cmap=reader.pages[0]["/Resources"]["/Font"]["/F1"]["/ToUnicode"].get_object().get_data()
                assert f"<20> <{code}>".encode() in cmap
                raw=sdk_inventory(library,file)
                assert raw[0]["unobserved_text_objects"]==1
                assert raw[0]["sdk_characters"]==(1 if mixed else 0)
                ir=run_ir(args.binary,file)
                assert ir["pages"][0]["sdk_text_objects_without_characters"]==1
                assert "\n".join(b["text"] for b in ir["pages"][0]["blocks"])==("C" if mixed else "")
                layer=subprocess.run([str(args.binary.resolve()),"--text-layer",str(file),"1"],capture_output=True,timeout=120)
                if mixed:
                    assert layer.returncode==0
                    assert all(r["sdk_text_objects_without_characters"]==1 for r in json.loads(layer.stdout))
                else:assert layer.returncode==2 and b"no character records" in layer.stderr
                files.append(dict(path=str(file.relative_to(ROOT/"corpus")),sha256=sha(file),
                                  unicode=bytes.fromhex(code).decode("utf-16-be"),sdk=raw,layer_exit=layer.returncode))
            results=[]
            for label,binary,expected in [("v28",args.baseline,0),("candidate",args.binary,3)]:
                p=subprocess.run([str(binary.resolve()),str(directory/(name+"-old.pdf")),str(directory/(name+"-new.pdf"))],capture_output=True,timeout=120)
                assert p.returncode==expected,p.stderr[-2000:]
                diff=json.loads(p.stdout)
                assert all(v==0 for v in diff["summary"].values())
                assert all(p["visual"]["changed_pixels"]==0 for p in diff["pages"])
                if label=="candidate":
                    assert diff["diagnostics"]["old_pages_with_unobserved_text_objects"]==1
                    assert diff["diagnostics"]["new_pages_with_unobserved_text_objects"]==1
                results.append(dict(engine=label,exit_code=p.returncode,diagnostics=diff["diagnostics"],summary=diff["summary"]))
            probes.append(dict(id=name,passed=True,source=files,results=results))
    report=dict(version=1,methodology=__doc__,tool_sha256=sha(__file__),
                binary_sha256=sha(args.binary),baseline_binary_sha256=sha(args.baseline),
                manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),sdk_sha256=sdk_sha,
                documents=len(rows),pages=sum(len(r["pages"]) for r in rows),
                exact_prior_ir_payloads=len(rows),review_documents=sum(r["review_pages"]>0 for r in rows),
                review_pages=sum(r["review_pages"] for r in rows),
                unobserved_objects=sum(r["unobserved_objects"] for r in rows),
                source_probes=len(probes),passed_source_probes=sum(p["passed"] for p in probes),
                unresolved_scope=["Partial omissions within an SDK-observed object are not detected by this whole-object inventory.",
                                  "Source decoding is not recovered; ordinary whitespace objects remain reviewable.",
                                  "Objects discarded before the SDK page-object tree is built are outside this detector."],
                results=rows,probes=probes)
    args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+"\n")
    print(f"{len(rows)} exact IR payloads; {report['unobserved_objects']} unobserved objects over {report['review_pages']} pages",flush=True)

if __name__=="__main__":
    main()
