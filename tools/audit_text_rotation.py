#!/usr/bin/env python3
"""Verify display-rotation invariance on every page of SHA-pinned public PDFs.

pypdf rewrites the presentation rotation on every page without changing decoded
content streams. All-page IR, including block order/bounds/fonts/diagnostics, is
compared after normalizing only presentation rotation and swapped dimensions.
Every first-page text layer must retain its text, scalar spans and diagnostics.
Original first-page PNGs are checked against the frozen v29 binary. Selected
rotated variants also retain PNG bytes. This is not arbitrary-angle logical
reading order, original Unicode recovery, or exhaustive natural-revision gold.
"""
import argparse
import concurrent.futures
import ctypes as C
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
from pypdf import PdfReader, PdfWriter, __version__ as pypdf_version
from pdf_corpus import ROOT, fetch
from audit_form_discovery import library_path

def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def run(binary, *args):
    p=subprocess.run([str(binary.resolve()), *map(str,args)],capture_output=True,timeout=180)
    assert p.returncode==0,p.stderr[-2000:]
    return json.loads(p.stdout)

def normalized(ir):
    out=json.loads(json.dumps(ir))
    for p in out["pages"]:
        if p["rotation"] in [90,270]:
            p["width"],p["height"]=p["height"],p["width"]
        p["rotation"]=0
    return out

def layer(binary,source,ir):
    p=subprocess.run([str(binary.resolve()),"--text-layer",str(source),"1"],capture_output=True,timeout=180)
    if p.returncode==0:
        return dict(rejected=False,runs=json.loads(p.stdout))
    assert p.returncode==2 and b"no character records" in p.stderr,p.stderr[-2000:]
    page=ir["pages"][0]
    assert not page["blocks"] and page["sdk_text_objects_without_characters"]>0
    return dict(rejected=True,reason="no character records",
                sdk_text_objects_without_characters=page["sdk_text_objects_without_characters"])

def render(binary,source,dest):
    p=subprocess.run([str(binary.resolve()),"--render",str(source),"1","400",str(dest)],capture_output=True,timeout=180)
    assert p.returncode==0,p.stderr[-2000:]
    return sha(dest)

def raw_profile(source,neutralize):
    library,sdk_hash=library_path()
    b=C.CDLL(str(library))
    def api(name,result,args):
        f=getattr(b,name);f.restype=result;f.argtypes=args;return f
    P,I,D=C.c_void_p,C.c_int,C.c_double
    init=api("FPDF_InitLibrary",None,[]);end=api("FPDF_DestroyLibrary",None,[])
    load=api("FPDF_LoadMemDocument64",P,[P,C.c_size_t,C.c_char_p])
    load_page=api("FPDF_LoadPage",P,[P,I]);load_text=api("FPDFText_LoadPage",P,[P])
    count=api("FPDFText_CountChars",I,[P]);unicode=api("FPDFText_GetUnicode",C.c_uint,[P,I])
    generated=api("FPDFText_IsGenerated",I,[P,I]);mapping=api("FPDFText_HasUnicodeMapError",I,[P,I])
    rotation=api("FPDFPage_GetRotation",I,[P]);set_rotation=api("FPDFPage_SetRotation",None,[P,I])
    box=api("FPDFText_GetCharBox",I,[P,I,*([C.POINTER(D)]*4)])
    origin=api("FPDFText_GetCharOrigin",I,[P,I,*([C.POINTER(D)]*2)])
    close_text=api("FPDFText_ClosePage",None,[P]);close_page=api("FPDF_ClosePage",None,[P])
    close_doc=api("FPDF_CloseDocument",None,[P])
    data=Path(source).read_bytes();buf=C.create_string_buffer(data);doc=page=text=None;rows=[]
    init()
    try:
        doc=load(buf,len(data),None);assert doc
        page=load_page(doc,0);assert page
        original_rotation=rotation(page)
        if neutralize:set_rotation(page,0)
        text=load_text(page);assert text;n=count(text);assert 0<=n<=2_000_000
        for i in range(n):
            scalar=unicode(text,i);error=mapping(text,i)
            assert error in [0,1]
            if chr(scalar).isspace() or generated(text,i):continue
            bounds=[D() for _ in range(4)];position=[D(),D()]
            assert box(text,i,*[C.byref(x) for x in bounds])
            assert origin(text,i,*[C.byref(x) for x in position])
            rows.append([scalar,error,*[x.value for x in bounds],*[x.value for x in position]])
        if neutralize:
            close_text(text);text=None
            set_rotation(page,original_rotation);assert rotation(page)==original_rotation
    finally:
        if text:close_text(text)
        if page:close_page(page)
        if doc:close_doc(doc)
        end()
    return dict(library_sha256=sdk_hash,source_sha256=sha(source),original_rotation=original_rotation*90,
                neutralized=neutralize,sdk_characters=n,nongenerated_nonwhitespace_records=len(rows),
                ordered_scalar_flag_bounds_origin_sha256=hashlib.sha256(json.dumps(rows,separators=(",",":")).encode()).hexdigest())

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary",type=Path,required=True)
    parser.add_argument("--baseline",type=Path,required=True)
    parser.add_argument("--report",type=Path,required=True)
    args=parser.parse_args()
    manifest_bytes=(ROOT/"corpus/manifest.json").read_bytes();manifest=json.loads(manifest_bytes)
    baseline_ids={"pdfjs-issue14497","pdfium-rotated-text","pdfium-rotated-text-90"}
    def inspect(entry):
        fetch(entry,ROOT/"corpus/cache",offline=True)
        source=ROOT/"corpus/cache"/(entry["id"]+".pdf")
        current=run(args.binary,"--ir",source);previous=run(args.baseline,"--ir",source)
        assert current==previous,(entry["id"],"original IR retention")
        reader=PdfReader(source);assert len(reader.pages)==entry["pages"]
        contents=[p.get_contents().get_data() if p.get_contents() is not None else b"" for p in reader.pages]
        content_hashes=[hashlib.sha256(x).hexdigest() for x in contents]
        reference=normalized(current);reference_layer=layer(args.binary,source,current)
        template=None
        if entry.get("source_template"):
            gold=entry["source_template"];path=ROOT/"corpus"/gold["file"]
            assert path.stat().st_size==gold["bytes"] and sha(path)==gold["sha256"]
            strings=[str(operands[0]) for operands,operator in reader.pages[0].get_contents().operations if operator==b"Tj"]
            assert strings==entry["declared_source_strings"]
            assert reader.pages[0]["/Resources"]["/Font"]["/F1"]["/BaseFont"]=="/Times-Roman"
            template=dict(file=gold["file"],sha256=gold["sha256"],declared_strings=strings,
                          sdk_mapping_errors=current["pages"][0]["sdk_unicode_mapping_errors"],
                          replacements=sum(b["text"].count("�") for b in current["pages"][0]["blocks"]))
            assert template["sdk_mapping_errors"]==template["replacements"]==2
        checks=[]
        with tempfile.TemporaryDirectory(prefix="jade-rotation-") as temporary:
            directory=Path(temporary)
            old_png=render(args.baseline,source,directory/"old.png")
            assert old_png==render(args.binary,source,directory/"new.png")
            for degrees in [0,90,180,270]:
                writer=PdfWriter();writer.clone_document_from_reader(reader)
                for p in writer.pages:p.rotation=degrees
                target=directory/(str(degrees)+".pdf");writer.write(target)
                check_reader=PdfReader(target)
                assert [p.rotation for p in check_reader.pages]==[degrees]*entry["pages"]
                assert [p.get_contents().get_data() if p.get_contents() is not None else b"" for p in check_reader.pages]==contents
                actual=run(args.binary,"--ir",target)
                assert normalized(actual)==reference,(entry["id"],degrees,"all-page text/metadata retention")
                actual_layer=layer(args.binary,target,actual)
                assert actual_layer==reference_layer,(entry["id"],degrees,"first-page character spans")
                row=dict(rotation=degrees,variant_sha256=sha(target),content_streams_exact=True,
                         all_page_ir_payload_exact=True,first_page_layer_exact=True,pages=entry["pages"],
                         first_page_layer_rejected=actual_layer["rejected"])
                if entry["id"] in baseline_ids:
                    before=run(args.baseline,"--ir",target)
                    old_layer=layer(args.baseline,target,before)
                    row.update(baseline_ir_payload_exact=normalized(before)==reference,
                               baseline_first_page_layer_exact=old_layer==reference_layer)
                    assert render(args.baseline,target,directory/"old-rotated.png")==render(args.binary,target,directory/"new-rotated.png")
                    row["rotated_first_page_png_exact"]=True
                checks.append(row)
        print("PASS "+entry["id"]+" "+str(entry["pages"]*4)+" page rotations",flush=True)
        return dict(id=entry["id"],source_sha256=entry["sha256"],passed=True,pages=entry["pages"],
                    original_ir_exact=True,source_first_page_png_exact=True,original_content_sha256=content_hashes,
                    source_template=template,checks=checks)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        rows=list(pool.map(inspect,manifest["documents"]))
    source=ROOT/"corpus/cache/pdfjs-issue14497.pdf"
    # The previous checkpoint preserves the originally failing rotation source.
    rotated=ROOT/"corpus/variants-objects/pdfjs-issue14497-rotate.pdf"
    profiles=[raw_profile(source,False),raw_profile(rotated,False),raw_profile(rotated,True)]
    assert [p["sdk_characters"] for p in profiles]==[274,293,274]
    assert profiles[0]["ordered_scalar_flag_bounds_origin_sha256"]!=profiles[1]["ordered_scalar_flag_bounds_origin_sha256"]
    assert profiles[0]["ordered_scalar_flag_bounds_origin_sha256"]==profiles[2]["ordered_scalar_flag_bounds_origin_sha256"]
    checks=[p for row in rows for p in row["checks"]]
    report=dict(version=1,methodology=__doc__,pypdf_version=pypdf_version,tool_sha256=sha(__file__),
                binary_sha256=sha(args.binary),baseline_binary_sha256=sha(args.baseline),
                manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),documents=len(rows),
                pages=sum(r["pages"] for r in rows),source_ir_exact_documents=len(rows),
                source_first_page_png_exact_documents=len(rows),variant_checks=len(checks),
                all_page_rotation_checks=sum(p["pages"] for p in checks),
                first_page_layer_rotation_checks=len(checks),selected_variant_png_checks=sum(p.get("rotated_first_page_png_exact",False) for p in checks),
                baseline_checked_variants=sum("baseline_ir_payload_exact" in p for p in checks),
                baseline_failing_variants=sum(p.get("baseline_ir_payload_exact") is False for p in checks),
                sdk_display_matrix_profiles=profiles,results=rows,
                limits=["Original Unicode and arbitrary-angle logical word order remain open.",
                        "No recovery of characters omitted inside an SDK-observed object or before its object tree.",
                        "Every original first-page PNG and selected rotated PNGs checked,not every page raster."])
    assert report["baseline_failing_variants"]>0
    args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+"\n")
    print(f"{len(rows)} sources; {report['all_page_rotation_checks']} page rotations, {len(checks)} first-page layers exact",flush=True)

if __name__=="__main__":
    main()
