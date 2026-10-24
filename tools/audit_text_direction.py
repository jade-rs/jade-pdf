#!/usr/bin/env python3
"""Audit baseline direction and whole-source retention against a frozen release.

Raw pinned-SDK matrices/angles explain shear; manually transcribed table gold
and independent pypdf/pdfplumber associations establish the selected reading
order. Whole-page scalar/diagnostic and geometry metadata are retained. On every
changed page, compare public visible character x-spans/font sizes and PNG bytes.
Valid source-declared surrogate pairs become Unicode scalars; invalid surrogate
fallbacks remain unknown. Literal source U+FFFD and declared duplicates remain.
Public layers expose run-level y bounds; this does not certify per-glyph y,
original Unicode, arbitrary math semantics or a complete natural-revision oracle.
"""
import argparse
from collections import Counter
import concurrent.futures
import ctypes as C
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unicodedata
import pdfplumber
from pypdf import PdfReader
from pdf_corpus import ROOT, fetch
from audit_form_discovery import library_path


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def compact(text):
    return "".join(unicodedata.normalize("NFC", text).split())


def raw_angles(source):
    library, sdk_hash = library_path()
    sdk = C.CDLL(str(library))
    class Matrix(C.Structure):
        _fields_ = [(key, C.c_float) for key in "abcdef"]
    def api(name, result, arguments):
        function = getattr(sdk, name); function.restype = result; function.argtypes = arguments
        return function
    ptr, integer = C.c_void_p, C.c_int
    init = api("FPDF_InitLibrary", None, [])
    end = api("FPDF_DestroyLibrary", None, [])
    load = api("FPDF_LoadMemDocument64", ptr, [ptr, C.c_size_t, C.c_char_p])
    load_page = api("FPDF_LoadPage", ptr, [ptr, integer])
    load_text = api("FPDFText_LoadPage", ptr, [ptr])
    count = api("FPDFText_CountChars", integer, [ptr])
    scalar = api("FPDFText_GetUnicode", C.c_uint, [ptr, integer])
    matrix = api("FPDFText_GetMatrix", integer, [ptr, integer, C.POINTER(Matrix)])
    angle = api("FPDFText_GetCharAngle", C.c_float, [ptr, integer])
    close_text = api("FPDFText_ClosePage", None, [ptr])
    close_page = api("FPDF_ClosePage", None, [ptr])
    close = api("FPDF_CloseDocument", None, [ptr])
    data = source.read_bytes(); buffer = C.create_string_buffer(data)
    init(); document = page = text = None; rows = []
    try:
        document = load(buffer, len(data), None); assert document
        page = load_page(document, 1); assert page
        text = load_text(page); assert text
        total = count(text); assert 0 <= total <= 2000000
        for index in range(total):
            if scalar(text, index) != 0x3BD:
                continue
            m = Matrix(); assert matrix(text, index, C.byref(m))
            if abs(m.b) < 1e-7 and abs(m.c) > 0.1 and angle(text,index) > 0.1:
                rows.append(dict(index=index, scalar="ν", matrix=[getattr(m,k) for k in "abcdef"],
                                 sdk_angle_radians=angle(text,index), horizontal_baseline=True))
        assert len(rows) == 5
    finally:
        if text: close_text(text)
        if page: close_page(page)
        if document: close(document)
        end()
    return dict(library_sha256=sdk_hash, source_sha256=sha(source), page=2, records=rows,
                implementation_reference=dict(url="https://raw.githubusercontent.com/chromium/pdfium/a84323421e94f484faca52dd9d027934eba42ab8/fpdfsdk/fpdf_text.cpp",
                sha256="d60f29321827b5d4637d9c3b6faebe2e582fff69f6803089afdddf78801f5ede",
                artifact="wasm-runtime/sdk-fpdf-text-8111.cpp",sdk_angle="atan2(c,a) includes x shear",baseline="The image of a horizontal text-space displacement has direction (a,b)."))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--report", type=Path, default=ROOT/"corpus/results-text-direction.json")
    args = parser.parse_args()
    manifest_path = ROOT/"corpus/manifest.json"
    manifest = json.loads(manifest_path.read_bytes())
    def run(binary, *params):
        p = subprocess.run([str(binary.resolve()), *map(str,params)], capture_output=True, timeout=180)
        assert p.returncode == 0, p.stderr[-2000:]
        return json.loads(p.stdout)
    def glyphs(layer):
        rows = []
        for run in layer:
            assert len(run["text"]) == len(run["chars"])
            rows += [(c, span["x0"], span["x1"], run["size"])
                     for c,span in zip(run["text"],run["chars"]) if not c.isspace()]
        return Counter(rows)
    def expansion_reference(entry,source):
        gold=entry.get("unicode_expansion_gold")
        if gold is None: return None
        reader=PdfReader(source)
        font=reader.pages[gold["page"]-1]["/Resources"]["/Font"][gold["font_resource"]]
        assert str(font["/BaseFont"])==gold["base_font"]
        cmap="".join(font["/ToUnicode"].get_object().get_data().decode("ascii").upper().split())
        with pdfplumber.open(source) as pdf:
            chars=pdf.pages[gold["page"]-1].chars
            counts=Counter(x["text"] for x in chars if "CambriaMath" in x["fontname"])
        for mapping in gold["mappings"]:
            assert "<"+mapping["cid_hex"]+"><"+mapping["utf16_be_hex"]+">" in cmap
            assert bytes.fromhex(mapping["utf16_be_hex"]).decode("utf-16-be")==mapping["text"]
            assert counts[mapping["text"]]==mapping["source_glyphs"]
        return gold

    def adjusted_glyphs(old_layer,new_layer,gold):
        old,new=glyphs(old_layer),glyphs(new_layer)
        if gold is None: return old==new,None
        delta=Counter()
        groups=0
        for (char,x0,x1,size),number in new.items():
            if char in gold["scalar_counts"]:
                assert number==2
                assert old[("�",x0,x1,size)]==number*2
                old[("�",x0,x1,size)]-=number*2
                if old[("�",x0,x1,size)]==0:del old[("�",x0,x1,size)]
                old[(char,x0,x1,size)]+=number
                delta[char]+=number;groups+=1
        assert dict(delta)==gold["scalar_counts"] and groups==5
        return old==new,dict(decoded_scalars=sum(delta.values()),removed_invalid_codeunit_replacements=sum(delta.values())*2,
                             source_glyph_mappings=groups,declared_scalar_counts=dict(delta))
    def inspect(entry):
        fetch(entry, ROOT/"corpus/cache", offline=True)
        source = ROOT/"corpus/cache"/(entry["id"]+".pdf")
        before = run(args.baseline,"--ir",source); after = run(args.binary,"--ir",source)
        assert len(before["pages"]) == len(after["pages"]) == entry["pages"]
        changed, review_changes = [], []
        expansion=expansion_reference(entry,source)
        with tempfile.TemporaryDirectory(prefix="jade-direction-") as temporary:
            directory = Path(temporary)
            for a,b in zip(before["pages"],after["pages"]):
                old_bag=Counter(compact("".join(x["text"] for x in a["blocks"])))
                new_bag=Counter(compact("".join(x["text"] for x in b["blocks"])))
                decoded_page=expansion is not None and b["index"]==expansion["page"]
                if decoded_page:
                    assert old_bag["�"]==expansion["literal_replacements"]+expansion["sdk_utf16_units"]
                    assert new_bag["�"]==expansion["literal_replacements"]
                    old_bag["�"]-=expansion["sdk_utf16_units"]
                    old_bag.update(expansion["scalar_counts"])
                assert old_bag==new_bag
                assert a["sdk_unicode_mapping_errors"] == b["sdk_unicode_mapping_errors"]
                metadata = lambda x:{k:v for k,v in x.items() if k not in ["blocks","reading_order_requires_review"]}
                assert metadata(a) == metadata(b)
                old_review = a.get("reading_order_requires_review",False); new_review = b.get("reading_order_requires_review",False)
                if old_review != new_review:
                    review_changes.append(dict(page=b["index"],old=old_review,new=new_review))
                if a == b:
                    continue
                number = b["index"]
                old_layer = run(args.baseline,"--text-layer",source,number)
                new_layer = run(args.binary,"--text-layer",source,number)
                retained,unicode_change=adjusted_glyphs(old_layer,new_layer,expansion if decoded_page else None)
                assert retained,(entry["id"],number,"visible glyph x-span/font payload")
                pngs = []
                for name,binary in [("old",args.baseline),("new",args.binary)]:
                    png = directory/(name+".png")
                    p = subprocess.run([str(binary.resolve()),"--render",str(source),str(number),"800",str(png)],
                                       capture_output=True,timeout=180)
                    assert p.returncode == 0, p.stderr[-2000:]
                    pngs.append(png.read_bytes())
                assert pngs[0] == pngs[1]
                changed.append(dict(page=number,visible_glyphs=sum(glyphs(new_layer).values()),
                                    glyph_xspan_size_multiset_equal=glyphs(old_layer)==glyphs(new_layer),
                                    glyph_payload_retained_after_declared_utf16_decoding=True,unicode_expansion=unicode_change,source_png_exact=True,
                                    old_blocks=len(a["blocks"]),new_blocks=len(b["blocks"])))
        print("PASS "+entry["id"],flush=True)
        return dict(id=entry["id"],source_sha256=entry["sha256"],pages=entry["pages"],
                    serialized_ir_exact=before==after,all_page_scalar_diagnostic_geometry_retained_after_declared_utf16_decoding=True,source_unicode_expansion_gold=expansion,
                    changed_pages=changed,reading_review_changes=review_changes)
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        rows = list(pool.map(inspect,manifest["documents"]))
    legacy = next(x for x in manifest["documents"] if x["id"]=="bipm-si-concise")
    source = ROOT/"corpus/cache"/(legacy["id"]+".pdf")
    report = dict(version=1,methodology=__doc__,binary_sha256=sha(args.binary),
                  baseline_binary_sha256=sha(args.baseline),manifest_sha256=sha(manifest_path),
                  tool_sha256=sha(__file__),documents=len(rows),pages=sum(x["pages"] for x in rows),
                  passed=len(rows),exact_ir_documents=sum(x["serialized_ir_exact"] for x in rows),
                  changed_documents=sum(bool(x["changed_pages"]) for x in rows),
                  changed_pages=sum(len(x["changed_pages"]) for x in rows),raw_sdk_angle_control=raw_angles(source),
                  selected_table_gold_report="results-table-order-direction.json",results=rows)
    args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+"\n")
    print(f"{report['passed']}/{report['documents']} sources retained; {report['changed_documents']} sources/{report['changed_pages']} pages regrouped")


if __name__ == "__main__":
    main()
