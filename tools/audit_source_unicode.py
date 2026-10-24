#!/usr/bin/env python3
"""Qualify independent source Unicode diagnostics without inventing positions.

All retained text/order/geometry and prior diagnostics must match frozen v32.
Twelve independently written expansion controls have CCC/CCCC original source
Unicode but the same CCC SDK output. ActualText overrides and an empty override
are separate controls. Selected original PDF ActualText periods are checked with
pypdf declarations and Poppler, never by treating parser consensus as full gold.
"""
import argparse,concurrent.futures,hashlib,json,subprocess
from pathlib import Path
from pypdf import PdfReader,PdfWriter,__version__ as pypdf_version
from pypdf.generic import NameObject,NumberObject
from pdf_corpus import ROOT,fetch
from audit_source_text import source_minimum,sdk_counts,masked_source
from audit_text_objects import document,stream
from audit_form_discovery import library_path
FIELDS=('source_unicode_scalars_unmatched','source_unicode_accounting_unavailable')
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def run(exe,*args,exits=(0,)):
 p=subprocess.run([str(exe),*map(str,args)],capture_output=True,timeout=180);assert p.returncode in exits,p.stderr[-2000:];return p.returncode,json.loads(p.stdout)
def strip(ir):
 d=json.loads(json.dumps(ir))
 for p in d['pages']:
  for key in FIELDS:p.pop(key,None)
 return d

def actual_source(revised,empty=False,named=False):
 replacement='<> ' if revised and empty else ('<FEFF0044>' if revised else '<FEFF0043>')
 props=f'/Properties << /P << /ActualText {replacement} >> >>'
 marked='/P' if named else f'<< /ActualText {replacement} >>'
 content=f'/Span {marked} BDC BT /F1 12 Tf 3 Tr 30 150 Td (A) Tj ET EMC'
 cmap='/CIDInit /ProcSet findresource begin 12 dict begin begincmap 1 begincodespacerange <00> <ff> endcodespacerange 1 beginbfchar <41> <0042> endbfchar endcmap end end'
 return document(['<< /Type /Catalog /Pages 2 0 R >>','<< /Type /Pages /Kids [3 0 R] /Count 1 >>',f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> {props} >> /Contents 5 0 R >>','<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 6 0 R >>',stream(content),stream(cmap)])

def structure_actual_source(replacement):
 return document(['<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >> /StructTreeRoot 6 0 R >>','<< /Type /Pages /Kids [3 0 R] /Count 1 >>','<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /StructParents 0 /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>','<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',stream('/Span << /MCID 0 >> BDC BT /F1 12 Tf 3 Tr 30 150 Td (C) Tj ET EMC'),'<< /Type /StructTreeRoot /K [7 0 R] /ParentTree 8 0 R /ParentTreeNextKey 1 >>',f'<< /Type /StructElem /S /Span /P 6 0 R /Pg 3 0 R /K 0 /ActualText ({replacement}) >>','<< /Nums [0 [7 0 R]] >>'])

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',type=Path,required=True);p.add_argument('--baseline',type=Path,required=True);p.add_argument('--report',type=Path,required=True);args=p.parse_args();args.binary=args.binary.resolve();args.baseline=args.baseline.resolve()
 manifest=json.loads((ROOT/'corpus/manifest.json').read_bytes());library,sdk_sha=library_path()
 def inspect(entry):
  fetch(entry,ROOT/'corpus/cache',offline=True);path=ROOT/'corpus/cache'/(entry['id']+'.pdf')
  _,before=run(args.baseline,'--ir',path);_,after=run(args.binary,'--ir',path);assert strip(after)==before,(entry['id'],'existing payload or diagnostics changed')
  return dict(id=entry['id'],source_sha256=entry['sha256'],retained_payload_exact=True,pages=[dict(page=p['index'],source_unicode_scalars_unmatched=p.get(FIELDS[0],0),source_unicode_accounting_unavailable=p.get(FIELDS[1],False),source_text_characters_missing=p.get('source_text_characters_missing',0),source_text_accounting_unavailable=p.get('source_text_accounting_unavailable',False)) for p in after['pages']])
 with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:rows=list(pool.map(inspect,manifest['documents']))
 # Keep the independent raw SDK library lifecycle sequential.
 encoded=[]
 for entry,row in zip(manifest['documents'],rows):
  path=ROOT/'corpus/cache'/(entry['id']+'.pdf');reader=PdfReader(path);raw=sdk_counts(library,path)
  for page,facts,sdk in zip(reader.pages,row['pages'],raw):
   if float(page.get('/UserUnit',1))!=1 or facts['source_text_accounting_unavailable']:continue
   try:minimum=source_minimum(reader,page)
   except (AssertionError,KeyError,ValueError,IndexError):continue
   expected=max(0,minimum-sdk['nongenerated_records']);assert expected==facts['source_text_characters_missing'],(entry['id'],facts['page'],expected)
   encoded.append(dict(id=entry['id'],page=facts['page'],source_minimum=minimum,sdk_nongenerated=sdk['nongenerated_records'],expected_deficit=expected))
 target=ROOT/'corpus/wasm-runtime/source-unicode-probes';target.mkdir(exist_ok=True);controls=[]
 for mapping in ['bfchar','bfrange']:
  for unit in [1,64]:
   for rotation in [0,90,270]:
    files=[];source_strings=[]
    for revised in [False,True]:
     data=masked_source(revised)
     if mapping=='bfrange':
      # Independent writer substitutes a range array for the first bfchar map.
      reader=PdfReader(__import__('io').BytesIO(data));writer=PdfWriter();writer.clone_document_from_reader(reader)
      tu=writer.pages[0]['/Resources']['/Font']['/F1']['/ToUnicode']
      content=tu.get_data().replace(b'2 beginbfchar <41> <00430043> <43> <0043> endbfchar',b'1 beginbfrange <41> <41> [<00430043>] endbfrange 1 beginbfchar <43> <0043> endbfchar')
      tu.set_data(content)
     else:
      reader=PdfReader(__import__('io').BytesIO(data));writer=PdfWriter();writer.clone_document_from_reader(reader)
     writer.pages[0].scale_by(1/unit);writer.pages[0][NameObject('/UserUnit')]=NumberObject(unit);writer.pages[0].rotation=rotation
     path=target/f'{mapping}-{unit}-{rotation}-{int(revised)}.pdf';writer.write(path);files.append(path)
     text=''.join(PdfReader(path).pages[0].extract_text().split());source_strings.append(text)
    assert source_strings==['CCC','CCCC'],source_strings
    old_code,_=run(args.baseline,*files,exits=(0,));new_code,diff=run(args.binary,*files,exits=(3,));_,ir=run(args.binary,'--ir',files[1]);page=ir['pages'][0]
    assert page.get('source_text_characters_missing',0)==0 and page.get('sdk_text_objects_without_characters',0)==0
    assert page['source_unicode_scalars_unmatched']==1 and not page.get('source_unicode_accounting_unavailable',False)
    assert ''.join(''.join(b['text'] for b in page['blocks']).split())=='CCC'
    assert all(v==0 for v in diff['summary'].values()) and diff['diagnostics']['new_pages_with_source_unicode_mismatch']==1
    controls.append(dict(id=f'{mapping}-{unit}-{rotation}',source_unicode=source_strings,source_sha256=[sha(f) for f in files],baseline_exit=old_code,current_exit=new_code,source_unicode_scalars_unmatched=1))
 actual=[]
 for named,empty in [(False,False),(True,False),(False,True)]:
  files=[]
  for revised in [False,True]:
   path=target/f'actual-{int(named)}-{int(empty)}-{int(revised)}.pdf';path.write_bytes(actual_source(revised,empty,named));files.append(path)
  old_code,_=run(args.baseline,*files,exits=(1,));code,diff=run(args.binary,*files,exits=((3,) if empty else (1,)));_,ir=run(args.binary,'--ir',files[1]);page=ir['pages'][0]
  assert not page.get('source_unicode_accounting_unavailable',False)
  assert page.get('source_unicode_scalars_unmatched',0)==int(empty)
  assert ''.join(b['text'] for b in page['blocks'])==('B' if empty else 'D')
  actual.append(dict(named=named,empty=empty,source_sha256=[sha(f) for f in files],baseline_exit=old_code,current_exit=code,source_unicode_scalars_unmatched=int(empty)))
 entry=next(e for e in manifest['documents'] if e.get('actual_text_gold'));path=ROOT/'corpus/cache'/(entry['id']+'.pdf');gold=entry['actual_text_gold'];reader=PdfReader(path);page=reader.pages[gold['page']-1]
 declared=[str(a[-1]['/ActualText']) for a,op in page.get_contents().operations if op==b'BDC' and isinstance(a[-1],dict) and '/ActualText' in a[-1]]
 assert declared.count('.')==gold['actual_text_periods']==19
 font=next(f.get_object() for f in page['/Resources']['/Font'].values() if str(f.get_object()['/BaseFont']).endswith('FedraSansAltPro-BookLF'));assert b'<2E> <0020>' in font['/ToUnicode'].get_data()
 poppler=Path('/Users/bokuweb/.cache/codex-runtimes/codex-primary-runtime/dependencies/native/poppler/poppler/bin/pdftotext')
 pp=subprocess.run([str(poppler),'-f','3','-l','3','-layout',str(path),'-'],capture_output=True,check=True,timeout=60).stdout.decode();assert pp.count('.')==gold['expected_periods']==20
 _,ir=run(args.binary,'--ir',path);sdk_periods=sum(b['text'].count('.') for b in ir['pages'][2]['blocks']);assert sdk_periods==20
 original=dict(id=entry['id'],page=3,source_sha256=sha(path),declared_actual_text_periods=19,pypdf_periods_without_actual_text=page.extract_text().count('.'),poppler_periods=pp.count('.'),native_periods=sdk_periods,source_codespace_inconsistency_reviewed=ir['pages'][2]['source_unicode_accounting_unavailable'],scope='All period multiplicities on one original page,not full-page Unicode orreading-order gold.')
 empty_file=target/'actual-0-1-1.pdf'
 empty_poppler=subprocess.run([str(poppler),str(empty_file),'-'],capture_output=True,check=True,timeout=60).stdout.decode().strip()
 assert empty_poppler==''
 actual[-1].update(poppler_text=empty_poppler,native_best_effort_text='B',status='review_detected_text_not_recovered')
 structural=[]
 for replacement in ['A','B']:
  path=target/('structure-actual-'+('old' if replacement=='A' else 'new')+'.pdf');path.write_bytes(structure_actual_source(replacement));structural.append(path)
  declared=PdfReader(path).trailer['/Root']['/StructTreeRoot']['/K'][0].get_object()['/ActualText'];assert str(declared)==replacement
 old_code,_=run(args.baseline,*structural,exits=(0,));code,diff=run(args.binary,*structural,exits=(0,))
 assert all(value==0 for value in diff['summary'].values())
 _,struct_ir=run(args.binary,'--ir',structural[1]);assert struct_ir['pages'][0]['blocks'][0]['text']=='C'
 structure_scope=dict(status='known_undetected_source_structure_actualtext',declared_replacements=['A','B'],source_sha256=[sha(path) for path in structural],files=[str(path.relative_to(ROOT/'corpus')) for path in structural],baseline_exit=old_code,current_exit=code,diagnostics=diff['diagnostics'],summary=diff['summary'],scope='Source-exhaustiveness control,not natural-revision semantic gold. StructElem ActualText isnot yet accounted;glyphC remains inSDK/IR despitechanged declaredreplacement.')
 pages=[p for d in rows for p in d['pages']]
 report=dict(version=1,methodology=__doc__,tool_sha256=sha(__file__),manifest_sha256=sha(ROOT/'corpus/manifest.json'),binary_sha256=sha(args.binary),baseline_binary_sha256=sha(args.baseline),sdk_sha256=sdk_sha,pypdf_version=pypdf_version,documents=len(rows),pages=len(pages),retained_payload_documents=len(rows),independent_encoded_deficit_page_checks=len(encoded),unicode_mismatch_pages=sum(p['source_unicode_scalars_unmatched']>0 for p in pages),unicode_scalar_discrepancies=sum(p['source_unicode_scalars_unmatched'] for p in pages),unicode_unavailable_pages=sum(p['source_unicode_accounting_unavailable'] for p in pages),expansion_controls=len(controls),actual_text_controls=len(actual),results=rows,encoded_deficit_checks=encoded,expansion_results=controls,actual_text_results=actual,original_actual_text_gold=original,known_structure_scope=structure_scope,limits=['Multiset equality doesnot prove source order,positions orfulloriginalUnicode.','Unknown/inherited/custom composite CMaps andmalformedUTF16 remainexplicitreview.','NFD andknownLatinligature/long-s equivalences areaccepted bydiagnosticnormalization.','Structure-tree ActualText andtextinpatterns are notindependently recovered.'])
 args.report.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if isinstance(v,int)}),flush=True)
if __name__=='__main__':main()
