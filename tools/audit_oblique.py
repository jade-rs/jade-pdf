#!/usr/bin/env python3
"""Audit oblique word grouping against pinned source strings and rendered labels.

Every source IR is compared with the supplied frozen baseline. Changed pages must retain every
non-whitespace scalar, mapping/reading/source-object diagnostic and metadata;
public glyph x-span/font-size multisets and PNG bytes must remain exact. This
does not expose per-glyph y through the public layer or certify full-page
logical order. All pages are additionally tested at four display rotations.
For source-verified marked replacements, explicitly recorded scalar deltas are
checked, non-period glyph x/font payloads remain exact, and source scope/anchor
evidence is required. General glyph-position preservation is not claimed for
those replacements. Selected Arabic and Japanese source lines are transformed independently with
pypdf; their complete per-page text-group multisets must survive.
"""
import argparse
from collections import Counter
import concurrent.futures
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unicodedata
from pypdf import PdfReader,PdfWriter,Transformation,__version__ as pypdf_version
from pypdf.generic import RectangleObject,NameObject,NumberObject
from pypdf._codecs import adobe_glyphs
from pdf_corpus import ROOT,fetch
from audit_text_objects import sdk_inventory
from audit_form_discovery import library_path

def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def run(exe,*args):
    p=subprocess.run([str(exe.resolve()),*map(str,args)],capture_output=True,timeout=180)
    assert p.returncode==0,p.stderr[-2000:]
    return json.loads(p.stdout)
def compact(text):return "".join(unicodedata.normalize("NFC",text).split())
def scalars(page):return Counter(compact("".join(b["text"] for b in page["blocks"])))
def glyphs(layer):
    out=Counter()
    for run in layer:
        assert len(run["text"])==len(run["chars"])
        out.update((c,s["x0"],s["x1"],run["size"]) for c,s in zip(run["text"],run["chars"]) if not c.isspace())
    return out
def layer(exe,path,number,ir):
    p=subprocess.run([str(exe.resolve()),"--text-layer",str(path),str(number)],capture_output=True,timeout=180)
    if p.returncode==0:return dict(rejected=False,runs=json.loads(p.stdout))
    assert p.returncode==2 and b"no character records" in p.stderr,p.stderr[-2000:]
    page=ir["pages"][number-1];assert not page["blocks"] and page["sdk_text_objects_without_characters"]>0
    return dict(rejected=True,sdk_text_objects_without_characters=page["sdk_text_objects_without_characters"])
def normalized(ir):
    out=json.loads(json.dumps(ir))
    for p in out["pages"]:
        if p["rotation"] in [90,270]:p["width"],p["height"]=p["height"],p["width"]
        p["rotation"]=0
    return out
def png(exe,path,number,dest):
    p=subprocess.run([str(exe.resolve()),"--render",str(path),str(number),"400",str(dest)],capture_output=True,timeout=180)
    assert p.returncode==0,p.stderr[-2000:]
    return sha(dest)
def decoded_subset_source(page):
    font=page["/Resources"]["/Font"]["/TT0"].get_object()
    assert font["/Subtype"]=="/TrueType" and "/ToUnicode" not in font
    encoding=font["/Encoding"].get_object();assert encoding["/BaseEncoding"]=="/WinAnsiEncoding"
    table={i:bytes([i]).decode("cp1252",errors="replace") for i in range(256)};index=0
    for value in encoding["/Differences"]:
        if isinstance(value,int):index=value
        else:
            assert str(value) in adobe_glyphs
            table[index]=adobe_glyphs[str(value)];index+=1
    source=""
    for args,op in page.get_contents().operations:
        if op==b"Tf":assert str(args[0])=="/TT0"
        if op==b"Tj":source+="".join(table[b] for b in args[0].original_bytes)
        assert op!=b"TJ"
    return " ".join(source.split())
def gold(entry,reader,ir):
    g=entry.get("word_group_gold")
    if not g:return None
    page=ir["pages"][g["page"]-1];texts=[b["text"] for b in page["blocks"]]
    out=dict(page=g["page"],passed=False,methodology=g["methodology"])
    if "numeric_labels" in g:
        expected=g["numeric_labels"];actual=[s for s in texts if all(c.isascii() and (c.isdigit() or c==" ") for c in s)]
        assert Counter(actual)==Counter(expected)
        decoded=decoded_subset_source(reader.pages[g["page"]-1])
        assert decoded.split("14B",1)[1].strip()=="".join(expected)
        out.update(labels=len(expected),source_font="/TT0",source_encoding="WinAnsi plus original Adobe glyph-name Differences",
                   original_encoded_string_sha256=hashlib.sha256(decoded.encode()).hexdigest(),
                   full_numeric_label_multiset_exact=True)
    elif "expected_blocks" in g:
        assert Counter(texts)==Counter(g["expected_blocks"])
        source=[str(a[0]) for a,op in reader.pages[g["page"]-1].get_contents().operations if op==b"Tj"]
        assert source==entry["declared_source_strings"]
        assert Counter(s.replace("\r","�").replace("\n","�").strip() for s in source)==Counter(g["expected_blocks"])
        out.update(labels=len(source),declared_source_strings=source,full_source_group_multiset_exact=True)
    else:
        assert all(s in texts for s in g["selected_labels"])
        declared=reader.pages[g["page"]-1].extract_text()
        assert all(s in declared for s in g["selected_labels"])
        out.update(labels=len(g["selected_labels"]),selected_labels=g["selected_labels"],selected_label_presence=True)
    out["passed"]=True
    return out

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--actual-text-evidence",type=Path)
    p.add_argument("--marked-scope-evidence",type=Path)
    p.add_argument("--allow-source-accounting-metadata",action="store_true")
    p.add_argument("--allow-source-unicode-metadata",action="store_true")
    p.add_argument("--binary",type=Path,required=True);p.add_argument("--baseline",type=Path,required=True);p.add_argument("--report",type=Path,required=True)
    args=p.parse_args();manifest_bytes=(ROOT/"corpus/manifest.json").read_bytes();manifest=json.loads(manifest_bytes)
    evidence=None;source_inventory={}
    evidence_path = args.marked_scope_evidence or args.actual_text_evidence
    assert not (args.marked_scope_evidence and args.actual_text_evidence)
    if evidence_path:
        evidence=json.loads(evidence_path.read_bytes())
        if args.marked_scope_evidence: assert evidence["baseline_sha256"]==sha(args.baseline)
        assert evidence['binary_sha256']==sha(args.binary)
        assert evidence['manifest_sha256']==hashlib.sha256(manifest_bytes).hexdigest()
        source_inventory={(x['id'],x['page']):x for x in evidence['sdk_inventories']}
    def inspect(entry):
        fetch(entry,ROOT/"corpus/cache",offline=True);source=ROOT/"corpus/cache"/(entry["id"]+".pdf")
        before=run(args.baseline,"--ir",source);after=run(args.binary,"--ir",source);reader=PdfReader(source)
        assert len(before["pages"])==len(after["pages"])==len(reader.pages)==entry["pages"]
        changed=[]
        with tempfile.TemporaryDirectory(prefix="jade-oblique-") as temp:
            directory=Path(temp)
            for a,b in zip(before["pages"],after["pages"]):
                omitted={"blocks"} | ({"source_text_characters_missing","source_text_accounting_unavailable"} if args.allow_source_accounting_metadata else set()) | ({"source_unicode_scalars_unmatched","source_unicode_accounting_unavailable"} if args.allow_source_unicode_metadata else set())
                replacement=entry.get('source_replacement_gold')
                actual_page=replacement and replacement['page']==b['index'] and not args.marked_scope_evidence
                marked_change = None
                if evidence is not None:
                    proof=next(r for r in evidence['rows'] if r['id']==entry['id'])['pages'][b['index']-1]
                    assert b==proof,(entry['id'],b['index'],'independent source evidence payload')
                    if args.marked_scope_evidence:
                        changes=next(r for r in evidence['rows'] if r['id']==entry['id'])['changes']
                        marked_change=next((r for r in changes if r['page']==b['index'] and r['blocks_changed']),None)
                    omitted |= {'sdk_text_objects_without_characters','source_text_characters_missing','source_text_accounting_unavailable','source_unicode_scalars_unmatched','source_unicode_accounting_unavailable','reading_order_requires_review'}
                assert {k:v for k,v in a.items() if k not in omitted}=={k:v for k,v in b.items() if k not in omitted},(entry["id"],b["index"],"metadata")
                if actual_page:
                    assert evidence is not None
                    assert ''.join(x['text'] for x in a['blocks']).strip()==replacement['raw_sdk_text'].strip()
                    assert ''.join(x['text'] for x in b['blocks']).replace('\n','')==entry['full_page_text'].replace('\n','')
                elif marked_change:
                    assert entry['id']=='pdfjs-actualtext-eset'
                    assert scalars(b)-scalars(a)==Counter(marked_change['added_scalars'])
                    assert scalars(a)-scalars(b)==Counter(marked_change['removed_scalars'])
                else:assert scalars(a)==scalars(b),(entry["id"],b["index"],"scalar bag")
                if a["blocks"]==b["blocks"]:continue
                number=b["index"];old_layer=layer(args.baseline,source,number,before);new_layer=layer(args.binary,source,number,after)
                assert not old_layer["rejected"] and not new_layer["rejected"]
                if actual_page:
                    raw=[r for r in new_layer['runs'] if not r.get('source_text_region')]
                    regions=[r for r in new_layer['runs'] if r.get('source_text_region')]
                    assert glyphs(old_layer['runs'])==glyphs(raw)
                    assert [r['text'] for r in regions]==[replacement['replacement']]
                elif marked_change:
                    # Marked periods replace SDK pseudo-glyph rectangles with
                    # declared scope geometry. Check every other visible scalar
                    # exactly; marked period ownership and selected anchors are
                    # separately proven in the bound source evidence report.
                    old_glyphs=Counter({k:v for k,v in glyphs(old_layer['runs']).items() if k[0]!='.'})
                    new_glyphs=Counter({k:v for k,v in glyphs(new_layer['runs']).items() if k[0]!='.'})
                    assert old_glyphs==new_glyphs,(entry['id'],number,'non-period glyph x-spans/fonts')
                else:assert glyphs(old_layer["runs"])==glyphs(new_layer["runs"]),(entry["id"],number,"glyph x-spans/fonts")
                assert png(args.baseline,source,number,directory/"old.png")==png(args.binary,source,number,directory/"new.png")
                changed.append(dict(page=number,old_blocks=len(a["blocks"]),new_blocks=len(b["blocks"]),
                                    glyph_xspan_font_multiset_exact=not bool(actual_page or marked_change),
                                    non_period_glyph_xspan_font_multiset_exact=bool(marked_change),
                                    source_replacement=bool(actual_page),marked_scope_projection=bool(marked_change),source_png_exact=True))
            word_gold=gold(entry,reader,after)
            source_layer=layer(args.binary,source,1,after)
            contents=[x.get_contents().get_data() if x.get_contents() is not None else b"" for x in reader.pages]
            rotations=[]
            for degrees in [0,90,180,270]:
                writer=PdfWriter();writer.clone_document_from_reader(reader)
                for x in writer.pages:x.rotation=degrees
                path=directory/(str(degrees)+".pdf");writer.write(path)
                control=PdfReader(path)
                assert [x.get_contents().get_data() if x.get_contents() is not None else b"" for x in control.pages]==contents
                actual=run(args.binary,"--ir",path)
                assert normalized(actual)==normalized(after),(entry["id"],degrees,"rotation IR")
                assert layer(args.binary,path,1,actual)==source_layer,(entry["id"],degrees,"rotation spans")
                if entry.get("word_group_gold"):gold(entry,control,actual)
                rotations.append(dict(rotation=degrees,pages=entry["pages"],ordered_ir_exact=True,first_page_layer_exact=True))
        print("PASS "+entry["id"],flush=True)
        return dict(id=entry["id"],source_sha256=entry["sha256"],pages=entry["pages"],passed=True,
                    serialized_ir_exact=before==after,changed_pages=changed,word_group_gold=word_gold,rotations=rotations)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:rows=list(pool.map(inspect,manifest["documents"]))
    library,sdk_hash=library_path();coverage=[]
    for entry in manifest["documents"]:
        source=ROOT/"corpus/cache"/(entry["id"]+".pdf");raw=sdk_inventory(library,source);ir=run(args.binary,"--ir",source)
        for observed,page in zip(raw,ir['pages']):
            proof=source_inventory.get((entry['id'],page['index']))
            if proof:
                assert observed['unobserved_text_objects']==proof['raw_unobserved']
                assert page.get('sdk_text_objects_without_characters',0)==proof['unobserved']
            else:assert observed['unobserved_text_objects']==page.get('sdk_text_objects_without_characters',0)
        coverage.append(dict(id=entry["id"],unobserved_objects=sum(x["unobserved_text_objects"] for x in raw),
                             review_pages=sum(x["unobserved_text_objects"]>0 for x in raw),pages=raw))
    languages=[]
    with tempfile.TemporaryDirectory(prefix="jade-oblique-language-") as temp:
        for name in ["arabic","vertical"]:
            source=ROOT/"corpus/cache"/(name+".pdf");reader=PdfReader(source);reference=run(args.baseline,"--ir",source)
            expected=[Counter(b["text"] for b in x["blocks"]) for x in reference["pages"]]
            for degrees in [45,135,225,315]:
                for unit in [1,64]:
                    writer=PdfWriter();writer.clone_document_from_reader(reader)
                    transform=Transformation().rotate(degrees).scale(1/unit).translate(300/unit,300/unit)
                    for x in writer.pages:
                        x.add_transformation(transform);x.mediabox=RectangleObject([-1000/unit,-1000/unit,2000/unit,2000/unit]);x.cropbox=x.mediabox
                        x[NameObject("/UserUnit")]=NumberObject(unit);x.rotation=90
                    path=Path(temp)/(name+str(degrees)+"-"+str(unit)+".pdf");writer.write(path)
                    ir=run(args.binary,"--ir",path)
                    assert [Counter(b["text"] for b in x["blocks"]) for x in ir["pages"]]==expected,(name,degrees,unit,"language group multiset")
                    languages.append(dict(id=name,angle=degrees,user_unit=unit,pages=len(ir["pages"]),word_groups_exact=True))
    golds=[x["word_group_gold"] for x in rows if x["word_group_gold"]]
    report=dict(version=3,actual_text_evidence_sha256=sha(args.actual_text_evidence) if args.actual_text_evidence else None,marked_scope_evidence_sha256=sha(args.marked_scope_evidence) if args.marked_scope_evidence else None,allowed_new_diagnostic_fields=(["source_text_characters_missing","source_text_accounting_unavailable"] if args.allow_source_accounting_metadata else [])+(["source_unicode_scalars_unmatched","source_unicode_accounting_unavailable"] if args.allow_source_unicode_metadata else []),retained_ir_documents=sum(not x["changed_pages"] for x in rows),methodology=__doc__,tool_sha256=sha(__file__),pypdf_version=pypdf_version,
                binary_sha256=sha(args.binary),baseline_binary_sha256=sha(args.baseline),manifest_sha256=hashlib.sha256(manifest_bytes).hexdigest(),
                sdk_sha256=sdk_hash,documents=len(rows),pages=sum(x["pages"] for x in rows),passed=len(rows),
                exact_ir_documents=sum(x["serialized_ir_exact"] for x in rows),changed_documents=sum(bool(x["changed_pages"]) for x in rows),
                changed_pages=sum(len(x["changed_pages"]) for x in rows),gold_documents=len(golds),gold_labels=sum(x["labels"] for x in golds),
                rotation_variants=len(rows)*4,all_page_rotation_checks=sum(x["pages"] for x in rows)*4,first_page_layer_rotation_checks=len(rows)*4,
                language_conditions=len(languages),language_page_conditions=sum(x["pages"] for x in languages),
                unobserved_objects=sum(x["unobserved_objects"] for x in coverage),unobserved_review_pages=sum(x["review_pages"] for x in coverage),
                unobserved_review_documents=sum(x["review_pages"]>0 for x in coverage),results=rows,language_controls=languages,sdk_object_coverage=coverage,
                limits=["Per-glyph y is not exposed in the public layer; changed-page PNGs and public x/font payloads checked.",
                        "Selected word-group multisets are not full-page logical reading order or original-Unicode precision/recall.",
                        "Loose-cell word spacing is a geometric inference; unsupported/overhanging font metrics and arbitrary bidi remain open.",
                        "Encoded glyph and decoded Unicode multisets now require review on disagreement;neither scalar multiplicity nor selected word groups prove source reading order/positions."])
    args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+"\n")
    print(f"{len(rows)} retained sources; {report['changed_documents']} regrouped sources/{report['changed_pages']} pages; {report['gold_labels']} word golds",flush=True)
if __name__=="__main__":main()
