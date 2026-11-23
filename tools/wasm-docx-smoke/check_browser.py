"""公開PDFバイト列WASM APIの実際のDOCX座標を、所有fixtureの原点と比較する。"""
from pathlib import Path
import tempfile,subprocess,hashlib,json,zipfile,xml.etree.ElementTree as E,sys,argparse
import os
from owned_lzw import owned_lzw_cases
from owned_type1_encoding import owned_type1_cases
from owned_font_names import legacy_font_pdf
from owned_image_colors import singleton_rgb_pdf, owned_rgb_pixels
from owned_table_order import owned_table_order_cases, assert_owned_table_rows
from owned_cell_lines import owned_cell_lines, assert_cell_lines, owned_cell_row_heights, owned_overfull_cell, owned_single_line_styles
ROOT=Path(__file__).resolve().parents[2]
TOOLS=Path(__file__).resolve().parent

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--probe',required=True,type=Path);parser.add_argument('--output',required=True,type=Path);parser.add_argument('--engine',choices=['chromium','firefox','webkit'],default=os.environ.get('JADE_PDF_BROWSER_ENGINE','chromium'));args=parser.parse_args();q=TOOLS;out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
    def pdf(content,width=612,height=792):
     objects=['<< /Type /Catalog /Pages 2 0 R >>','<< /Type /Pages /Kids [3 0 R] /Count 1 >>',f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>',f'<< /Length {len(content.encode())} >>\nstream\n{content}\nendstream','<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>'];data=bytearray(b'%PDF-1.7\n');offsets=[]
     for i,obj in enumerate(objects,1):offsets.append(len(data));data+=f'{i} 0 obj\n{obj}\nendobj\n'.encode()
     xref=len(data);data+=b'xref\n0 6\n0000000000 65535 f \n'
     for offset in offsets:data+=f'{offset:010} 00000 n \n'.encode()
     data+=f'trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode();return bytes(data)
    anchor='BT /F1 10 Tf 72 740 Td (Anchor) Tj ET';heading='BT /F1 10 Tf 60 650 Td ('+'x'*85+') Tj ET';offset='BT /F1 10 Tf 84 600 Td (Offset body) Tj ET';grid='100 500 m 300 500 l S 100 460 m 300 460 l S 100 460 m 100 500 l S 200 460 m 200 500 l S 300 460 m 300 500 l S BT /F1 10 Tf 110 480 Td (A) Tj ET BT /F1 10 Tf 210 480 Td (B) Tj ET'
    cases=[dict(id='plain',data=pdf(anchor),body={'Anchor':72}),dict(id='body-overhang',data=pdf(anchor+' '+heading+' '+offset),body={'Anchor':72,'Offset body':84}),dict(id='graphic-left',data=pdf(anchor+' 30 700 m 40 700 l S'),body={'Anchor':72}),dict(id='table-overhang',data=pdf(anchor+' '+heading+' '+grid),body={},table=100),dict(id='centre-control',data=pdf(anchor+' BT /F1 10 Tf 93.5 650 Td ('+'x'*85+') Tj ET'),body={'Anchor':72},centre=306),dict(id='word-limit',data=pdf(anchor,2000,1000),body={'Anchor':72*1584/2000},page=(31680,15840),resized=True),dict(id='empty',data=b'',error='header')]
    pitch=' '.join(f'BT /F1 10 Tf 72 {740-i*20} Td (Line {i+1}) Tj ET' for i in range(4));cases.append(dict(id='line-pitch',data=pdf(pitch),body={},pitch=20))
    for cid in [False, True]:cases.append(dict(id='legacy-cid' if cid else 'legacy-simple',data=legacy_font_pdf(cid),body={},font='OwnedCJK'))
    cases.append(dict(id='singleton-rgb',data=singleton_rgb_pdf(),body={},pixels=[12,34,56,78,90,123]))
    cases.extend(owned_table_order_cases(pdf))
    cases.append(owned_cell_lines(pdf))
    cases.append(owned_cell_row_heights(pdf))
    cases.append(owned_overfull_cell(pdf))
    cases.append(owned_single_line_styles(pdf))
    cases.extend(owned_lzw_cases())
    cases.extend(owned_type1_cases())
    visible='BT /F1 10 Tf 20 80 Td (VISIBLE TEXT) Tj ET '
    for name,path,painted,graphics in [('painted','10 10 m 20 40 40 40 60 10 c f',True,0),('cancelled','10 10 m 20 40 40 40 60 10 c n',False,0),('rule','10 10 m 20 40 40 40 60 40 c 60 10 l S',True,0),('cancelled-then-rule','10 10 m 20 40 40 40 60 10 c n 10 10 m 60 10 l S',False,1)]:
     cases.append(dict(id='curve-'+name,data=pdf(visible+path,100,100),page=(2000,2000),expected_issues=[dict(kind='unsupported_curved_path',page=0)] if painted else [],expected_graphics=graphics,retained_text='VISIBLE TEXT'))
    manifest=json.loads((ROOT/'corpus/manifest.json').read_text());entry=next(e for e in manifest['pending_semantic'] if e['id']=='nta-invoice-domestic-english-2026');data=((ROOT/'corpus/cache')/(entry['id']+'.pdf')).read_bytes();assert len(data)==entry['bytes'] and hashlib.sha256(data).hexdigest()==entry['sha256'];cases.append(dict(id='encrypted',data=data,error='encrypt'))
    with tempfile.TemporaryDirectory() as temp:
     descriptors=[]
     for case in cases:
      file=Path(temp)/(case['id']+'.pdf');file.write_bytes(case['data']);descriptors.append(dict(id=case['id'],input=str(file),sha256=hashlib.sha256(case['data']).hexdigest()))
     file=Path(temp)/'manifest.json';file.write_text(json.dumps(dict(cases=descriptors)));subprocess.run(['node',str(q/'run_browser.mjs'),str(file),str(args.probe.resolve()),str(out),args.engine],check=True,cwd=ROOT)
    raw=json.loads((out/'browser-raw.json').read_text());assert len(raw['rows'])==len(cases);n={'w':'http://schemas.openxmlformats.org/wordprocessingml/2006/main'};W='{'+n['w']+'}';results=[]
    for case,row in zip(cases,raw['rows']):
     assert case['id']==row['id'];inspection=row['inspection'];issues=inspection.get('value',{}).get('quality_issues',[])
     if case.get('resized'):assert any(i['kind']=='page_resized_for_word' for i in issues)
     assert [o['mode'] for o in row['outputs']]==['default','strict']
     for output in row['outputs']:
      checks=[];failure=None
      try:
       if 'expected_issues' in case:assert issues==case['expected_issues'],issues
       if 'expected_graphics' in case:assert inspection['value']['graphics_count']==case['expected_graphics'],inspection
       if 'rows' in case or case.get('cell_lines') or 'row_heights' in case or case.get('overfull_cell') or 'single_line_styles' in case:assert not issues,issues
       if case.get('error'):
        assert case['error'] in output.get('error','').lower(),output;assert case['error'] in inspection.get('error','').lower(),inspection;checks.append('explicit rejection')
       elif output['mode']=='strict' and issues:
        assert 'error' in output,output;checks.append('detected quality loss rejected')
       else:
        assert 'file' in output,output
        with zipfile.ZipFile(output['file']) as z:
         doc=E.fromstring(z.read('word/document.xml'))
         if 'expected_text' in case:
          actual_text=''.join(t.text or '' for t in doc.findall('.//w:t',n));assert actual_text==case['expected_text'],dict(expected=case['expected_text'],actual=actual_text);checks.append('authored Type1 glyph Unicode and per-code priority')
         if 'rows' in case:checks.append(assert_owned_table_rows(doc,case['rows']))
         if case.get('cell_lines'):checks.append(assert_cell_lines(doc))
         if 'single_line_styles' in case:
          cells=doc.findall('.//w:tbl/w:tr/w:tc',n);actual=[[int(s.attrib[W+'val']) for s in c.findall('.//w:sz',n)] for c in cells];assert actual==[[s] for s in case['single_line_styles']],actual;assert all(c.find('w:tcPr/w:vAlign',n).attrib[W+'val']=='center' for c in cells)
          for cell,source_size in zip(cells,case['single_line_styles']):
           spacing=cell.find('w:p/w:pPr/w:spacing',n);assert spacing is not None;assert spacing.attrib.get(W+'lineRule')=='exact',spacing.attrib;assert int(spacing.attrib[W+'line'])==source_size*10,spacing.attrib
          checks.append(dict(source_single_line_sizes=actual,source_line_boxes_twips=[s*10 for s in case['single_line_styles']],legacy_vertical_alignment='center'))
         if case.get('overfull_cell'):
          cell=doc.find('.//w:tbl/w:tr/w:tc',n);paragraphs=cell.findall('w:p',n);actual=[''.join(t.text or '' for t in p.findall('.//w:t',n)) for p in paragraphs];assert actual==['Alpha Beta'],actual;checks.append('overfull source line boxes retain legacy editable cell text')
         if 'row_heights' in case:
          actual=[int(h.attrib[W+'val']) for h in doc.findall('.//w:tbl/w:tr/w:trPr/w:trHeight',n)];assert actual==case['row_heights'],dict(expected=case['row_heights'],actual=actual);checks.append(dict(source_row_heights=case['row_heights'],actual=actual))
         if 'pixels' in case:
          media=[f.filename for f in z.infolist() if f.filename.startswith('word/media/') and not f.is_dir()];assert len(media)==1;actual=owned_rgb_pixels(z.read(media[0]));assert actual==case['pixels'],actual;checks.append('source RGB pixels preserved')
        margin=doc.find('.//w:pgMar',n).attrib;size=doc.find('.//w:pgSz',n).attrib;left=int(margin[W+'left']);width=int(size[W+'w']);right=int(margin[W+'right']);assert (width,int(size[W+'h']))==case.get('page',(12240,15840))
        paragraphs={''.join(t.text or '' for t in p.findall('.//w:t',n)):p for p in doc.findall('.//w:p',n)}
        if 'retained_text' in case:assert case['retained_text'] in paragraphs,paragraphs;checks.append('existing editable text retained')
        if 'font' in case:
         fonts=paragraphs['A'].findall('.//w:rFonts',n);assert fonts;assert all(f.attrib.get(W+'ascii')==case['font'] for f in fonts),[f.attrib for f in fonts];checks.append('verified program display name')
        for text,expected in case.get('body',{}).items():
         p=paragraphs[text];ind=p.find('w:pPr/w:ind',n);indent=0 if ind is None else int(ind.attrib.get(W+'left','0'));actual=(left+indent)/20;checks.append(dict(text=text,expected=expected,actual=actual));assert abs(actual-expected)<=.05001,checks[-1]
        if 'pitch' in case:
         previous=None
         for index in range(4):
          spacing=paragraphs[f'Line {index+1}'].find('w:pPr/w:spacing',n);assert spacing is not None;assert spacing.attrib.get(W+'lineRule')=='exact';height=int(spacing.attrib[W+'line'])/20;assert height>0
          if previous is not None:
           actual=previous+int(spacing.attrib.get(W+'before','0'))/20;checks.append(dict(pitch_expected=case['pitch'],actual=actual));assert abs(actual-case['pitch'])<=.05001,checks[-1]
          previous=height
        if 'table' in case:
         table=doc.find('.//w:tbl',n);assert table is not None;ind=table.find('w:tblPr/w:tblInd',n);indent=0 if ind is None else int(ind.attrib.get(W+'w','0'));actual=(left+indent)/20;checks.append(dict(table_expected=case['table'],actual=actual));assert abs(actual-case['table'])<=.05001,checks[-1]
        if 'centre' in case:
         p=paragraphs['x'*85];assert p.find('w:pPr/w:jc',n).attrib[W+'val']=='center';ind=p.find('w:pPr/w:ind',n);a=0 if ind is None else int(ind.attrib.get(W+'left','0'));b=0 if ind is None else int(ind.attrib.get(W+'right','0'));actual=(left+a+width-right-b)/40;checks.append(dict(centre_expected=case['centre'],actual=actual));assert abs(actual-case['centre'])<=.0251,checks[-1]
      except (AssertionError,KeyError,ValueError,zipfile.BadZipFile,E.ParseError) as e:failure=str(e) or type(e).__name__
      results.append(dict(id=case['id'],mode=output['mode'],passed=failure is None,failure=failure,checks=checks))
    report=dict(engine=raw['engine'],browser=raw['browser'],cases=len(results),passed=sum(r['passed'] for r in results),scope='Actual production byte-PDF conversion default/strict and inspection exports; owned geometry/default page bounds, resized strict rejection and real copy-permitted encrypted source rejected by pure/WASM API. No rendered-PNG API, Word rendering or font-fidelity proof.',input_sha256={c['id']:hashlib.sha256(c['data']).hexdigest() for c in cases},WASM_sha256=hashlib.sha256((args.probe/'jade_pdf2docx_wasm_bg.wasm').read_bytes()).hexdigest(),rows=results,production_ready=False);(out/'geometry-report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if k not in ('rows','input_sha256')}));print('\n'.join(str(r) for r in results if not r['passed']));return 0 if all(r['passed'] for r in results) else 1

if __name__ == "__main__":
    sys.exit(main())
