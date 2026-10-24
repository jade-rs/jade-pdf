// Run against a compiled probe and a hash-pinned Emscripten Pdfium under corpus/wasm-runtime.
import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const { chromium } = await import(process.env.JADE_PDF_PLAYWRIGHT_PATH
  ? pathToFileURL(process.env.JADE_PDF_PLAYWRIGHT_PATH).href : 'playwright');
const manifestBytes = await fs.readFile(path.join(root,'corpus/manifest.json'));
const manifest = JSON.parse(manifestBytes);
const runtime = JSON.parse(await fs.readFile(path.join(root,'tools/wasm-smoke/runtime.json')));
const adapterBytes=await fs.readFile(path.join(root,'crates/jade-pdf-diff/wasm/pdfium_adapter.mjs'));
const adapterHash=createHash('sha256').update(adapterBytes).digest('hex');
if (!adapterBytes.equals(await fs.readFile(path.join(root,'corpus/wasm-runtime/release/node/jade_pdfium_adapter.mjs'))))
  throw new Error('Prepared Pdfium character-box adapter differs from product source');
const loaderHash=createHash('sha256').update(await fs.readFile(path.join(root,'corpus/wasm-runtime/release/node/pdfium.esm.js'))).digest('hex');
const unpreparedLoaderHash=createHash('sha256').update(await fs.readFile(path.join(root,'corpus/wasm-runtime/release/node/pdfium-unprepared.esm.js'))).digest('hex');
let contractProbe;
const selected = process.argv.slice(2);
if (selected.some(id => !manifest.documents.some(e => e.id === id))) throw new Error('Unknown corpus ID');
const documents = manifest.documents.filter(e => !selected.length || selected.includes(e.id));
// Use one native executable for the complete run, even if Cargo rebuilds it.
const binaryDirectory = await fs.mkdtemp(path.join(os.tmpdir(),'jade-pdf-wasm-native-'));
const binary = path.join(binaryDirectory,'pdf-diff');
await fs.copyFile(process.env.JADE_PDF_BINARY || path.join(root,'target/debug/pdf-diff'),binary);
await fs.chmod(binary,0o700);
const binaryHash=createHash('sha256').update(await fs.readFile(binary)).digest('hex');
const reportPath=process.env.JADE_PDF_BROWSER_REPORT || path.join(root,'corpus/results-wasm-runtime.json');
await fs.rm(reportPath,{force:true});
const server = http.createServer(async (request,response) => {
  try {
    const url = new URL(request.url,'http://localhost');
    const name = decodeURIComponent(url.pathname);
    if (name === '/favicon.ico') { response.writeHead(204).end(); return; }
    if (name === '/') { response.end('<!doctype html><title>jade-pdf WASM runtime test</title>'); return; }
    const target = path.resolve(root,'.'+name);
    if (!target.startsWith(root+path.sep)) { response.writeHead(403).end(); return; }
    const types = {'.wasm':'application/wasm','.js':'application/javascript','.mjs':'application/javascript','.json':'application/json'};
    response.setHeader('Content-Type',types[path.extname(target)] || 'application/octet-stream');
    response.end(await fs.readFile(target));
  } catch { response.writeHead(404).end(); }
});
await new Promise(resolve => server.listen(0,'127.0.0.1',resolve));
let browser;
const results = [];
try {
  browser = await chromium.launch({headless:true,
    ...(process.env.JADE_PDF_BROWSER ? {executablePath:process.env.JADE_PDF_BROWSER} : {})});
  const page = await browser.newPage();
  page.on('console', message => { if (message.type() === 'error') process.stderr.write(message.text()+'\n'); });
  page.on('pageerror', error => process.stderr.write(error.stack+'\n'));
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  const initialization = await page.evaluate(async () => {
    const probe = await import('/corpus/wasm-runtime/probe/jade_pdf_wasm_smoke.js');
    const local = await probe.default();
    const contractProbe = probe.contract_probe();
    const probeLength = contractProbe.length;
    if (probeLength > 2048) throw new Error("Runtime ABI probe exceeds its fixed-size test budget");
    const {default:createUnprepared} = await import('/corpus/wasm-runtime/release/node/pdfium-unprepared.esm.js');
    const {preparePdfiumForJade} = await import('/crates/jade-pdf-diff/wasm/pdfium_adapter.mjs');
    const locateFile = name=>'/corpus/wasm-runtime/release/node/'+name;
    const bytes = new Uint8Array(await (await fetch('/corpus/cache/hello.pdf')).arrayBuffer());
    const operations = {
      inspect:()=>JSON.parse(probe.inspect(bytes)).pages.length===1,
      text_layer:()=>JSON.parse(probe.layer(bytes,1)).length>0,
      width_png:()=>probe.render(bytes,1,400).length>8,
      dpi_png:()=>probe.render_at_dpi(bytes,1,72).length>8,
      comparison:()=>Object.values(JSON.parse(probe.compare(bytes,bytes)).summary).every(x=>x===0),
    };
    const checks=[];
    const rejectUnprepared = (id,module) => {
      const original=module.ccall;
      const loads=[];
      let closedDocuments=0,closedTextPages=0,bitmapAllocations=0;
      module.ccall=function(name,returnType,types,args,...rest) {
        if(name==='FPDF_LoadMemDocument64') loads.push(args[1]);
        if(name==='FPDF_CloseDocument') closedDocuments++;
        if(name==='FPDFText_ClosePage') closedTextPages++;
        if(name.startsWith('FPDFBitmap_Create')) bitmapAllocations++;
        return Reflect.apply(original,module,[name,returnType,types,args,...rest]);
      };
      const initialized=probe.initialize_pdfium_render(module,local,false);
      const rejected=[],unexpected=[];
      for (const [operation,run] of Object.entries(operations)) {
        try { run(); unexpected.push({operation,result:'returned a result'}); }
        catch (error) {
          if (error instanceof WebAssembly.RuntimeError) throw error;
          if (String(error).includes('PDFIUM_WASM_ABI_MISMATCH')) rejected.push(operation);
          else unexpected.push({operation,error:String(error)});
        }
      }
      const bounded=loads.length===5 && loads.every(n=>n===probeLength)
        && closedDocuments===5 && closedTextPages===5 && bitmapAllocations===0;
      checks.push({id,passed:initialized && rejected.length===5 && bounded,initialized,rejected,unexpected,
        loaded_document_bytes:[...loads],closedDocuments,closedTextPages,bitmapAllocations});
    };
    const unprepared=await createUnprepared({locateFile});
    rejectUnprepared('unprepared-wasm-rejected',unprepared);
    preparePdfiumForJade(unprepared);
    const recoveredInitialization=probe.initialize_pdfium_render(unprepared,local,false);
    const recovered=[];
    for (const [operation,run] of Object.entries(operations)) {
      try { if(run()) recovered.push(operation); }
      catch (error) { if(error instanceof WebAssembly.RuntimeError) throw error; }
    }
    checks.push({id:'prepared-wasm-recovery',passed:recoveredInitialization && recovered.length===5,recovered});
    // A previously successful call must not cache acceptance of a new SDK.
    const replacement=await createUnprepared({locateFile});
    rejectUnprepared('unprepared-wasm-rebind-rejected',replacement);
    const {default:createPdfium} = await import('/corpus/wasm-runtime/release/node/pdfium.esm.js');
    const pdfium = await createPdfium({locateFile: name=>'/corpus/wasm-runtime/release/node/'+name});
    // Product callers may import their own copy in addition to the SDK loader.
    // Preparation must remain idempotent across distinct module instances.
    preparePdfiumForJade(pdfium);
    preparePdfiumForJade(pdfium);
    window.jadeProbe = probe;
    const ok = probe.initialize_pdfium_render(pdfium,local,false);
    return {ok,checks,contractProbe:Array.from(contractProbe),keys:Object.keys(pdfium).filter(k=>/wasm|table|malloc|ccall|free/i.test(k))};
  });
  contractProbe = Buffer.from(initialization.contractProbe);
  if (!initialization.ok) throw new Error('Pdfium WASM initialization failed: '+JSON.stringify(initialization));
  results.push(...initialization.checks);
  for (const result of initialization.checks) console.log((result.passed?'PASS ':'FAIL ')+JSON.stringify(result));
  const glyphBoxGold = await page.evaluate(() => {
    // Independent Type3 glyph: 400x700 font units at 20pt and (60,100).
    // Its painted box is [60,100,68,114]; all axes deliberately differ.
    const stream = s => `<< /Length ${s.length} >>\nstream\n${s}\nendstream`;
    const objects = ['<< /Type /Catalog /Pages 2 0 R >>',
      '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
      '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',
      '<< /Type /Font /Subtype /Type3 /FontBBox [0 0 400 700] /FontMatrix [.001 0 0 .001 0 0] /CharProcs << /A 6 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [500] /Resources << >> >>',
      stream('BT /F1 20 Tf 60 100 Td (A) Tj ET'),
      stream('500 0 0 0 400 700 d1 0 0 400 700 re f')];
    let source='%PDF-1.7\n'; const offsets=[];
    for (const [i,obj] of objects.entries()) { offsets.push(source.length); source+=`${i+1} 0 obj\n${obj}\nendobj\n`; }
    const xref=source.length;
    source+=`xref\n0 ${objects.length+1}\n0000000000 65535 f \n`;
    source+=offsets.map(x=>String(x).padStart(10,'0')+' 00000 n \n').join('');
    source+=`trailer\n<< /Size ${objects.length+1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
    const runs=JSON.parse(window.jadeProbe.layer(new TextEncoder().encode(source),1));
    const expected={x0:60,y0:100,x1:68,y1:114}, run=runs[0];
    const passed=runs.length===1 && run.text==='A' && run.chars.length===1
      && Object.entries(expected).every(([k,v])=>Math.abs(run.bounds[k]-v)<.001)
      && Math.abs(run.chars[0].x0-60)<.001 && Math.abs(run.chars[0].x1-68)<.001;
    return {id:'glyph-box-output-slots',passed,expected,runs};
  });
  results.push(glyphBoxGold);
  for (const entry of documents) {
    const started = Date.now();
    try {
      const file = path.join(root,'corpus/cache',entry.id+'.pdf');
      const data = await fs.readFile(file);
      if (createHash('sha256').update(data).digest('hex') !== entry.sha256) throw new Error('PDF checksum mismatch');
      const native = spawnSync(binary,['--ir',file],
        {encoding:'utf8',timeout:60000,maxBuffer:32*1024*1024});
      if (native.status !== 0) throw new Error(native.stderr || String(native.error));
      const expected = JSON.parse(native.stdout);
      const actual = await page.evaluate(async ({id,rtlSpaces}) => {
        const bytes = new Uint8Array(await (await fetch('/corpus/cache/'+id+'.pdf')).arrayBuffer());
        const ir = JSON.parse(window.jadeProbe.inspect(bytes));
        if (rtlSpaces) {
          for (const run of JSON.parse(window.jadeProbe.layer(bytes,1))) {
            for (const [i,c] of Array.from(run.text).entries()) {
              if (i>0 && /\s/u.test(c)) {
                const span=run.chars[i], previous=run.chars[i-1];
                if (span.x0!==span.x1 || Math.abs(span.x0-previous.x0)>.001)
                  throw new Error('RTL blank caret does not anchor to the preceding glyph: '+JSON.stringify({i,span,previous,text:run.text}));
              }
            }
          }
        }
        const rendered = JSON.parse(window.jadeProbe.render_with_info(bytes,1,400));
        const png = new Uint8Array(rendered.png);
        const width = new DataView(png.buffer,png.byteOffset,png.byteLength).getUint32(16);
        if (png[0] !== 137 || png[1] !== 80 || width !== 400) throw new Error('Invalid rendered PNG');
        const v=rendered.viewport, p=ir.pages[0];
        if (v.width!==rendered.width || v.height!==rendered.height || v.rotation!==p.rotation
            || v.user_unit!==p.user_unit
            || ['x0','y0','x1','y1'].some(k=>Math.abs(v.view_box[k]-p.view_box[k])>.01))
          throw new Error('Rendered viewport does not match the extracted page');
        if(id==='pdfjs-user-unit') {
          if(p.user_unit!==72 || p.width!==8.5 || p.height!==11) throw new Error('Public physical page-unit gold mismatch');
          const dpiPng=new Uint8Array(window.jadeProbe.render_at_dpi(bytes,1,72));
          const header=new DataView(dpiPng.buffer,dpiPng.byteOffset,dpiPng.byteLength);
          if(header.getUint32(16)!==612 || header.getUint32(20)!==792) throw new Error('Physical 72DPI rendering mismatch');
        }
        return {ir,png_bytes:png.length,character_layer:rtlSpaces ? JSON.parse(window.jadeProbe.layer(bytes,1)) : null};
      },{id:entry.id,rtlSpaces:entry.right_to_left_spaces});
      if (actual.character_layer) {
        const layerProcess=spawnSync(binary,['--text-layer',file,'1'],
          {encoding:'utf8',timeout:60000,maxBuffer:32*1024*1024});
        if (layerProcess.status!==0) throw new Error(layerProcess.stderr || String(layerProcess.error));
        const nativeLayer=JSON.parse(layerProcess.stdout), wasmLayer=actual.character_layer;
        if (nativeLayer.length!==wasmLayer.length) throw new Error('Character layer run count mismatch');
        for (const [i,run] of nativeLayer.entries()) {
          const other=wasmLayer[i];
          if (run.text!==other.text || run.chars.length!==other.chars.length
            || ['x0','y0','x1','y1'].some(k=>Math.abs(run.bounds[k]-other.bounds[k])>.001)
            || run.chars.some((span,j)=>['x0','x1'].some(k=>Math.abs(span[k]-other.chars[j][k])>.001)))
            throw new Error('Native/WASM public character-box geometry mismatch');
        }
      }
      if (entry.full_page_text !== undefined) {
        const fullText = ir => ir.pages.map(p=>p.blocks.map(b=>b.text).join('\n')).join('\n');
        if (fullText(expected)!==entry.full_page_text || fullText(actual.ir)!==entry.full_page_text)
          throw new Error('Declared full-page Unicode gold mismatch');
      }
      for (const gold of entry.reading_order ?? []) {
        const normalized = text => text.normalize('NFC').replace(/\s/gu,'');
        for (const [engine,ir] of [['native',expected],['WASM',actual.ir]]) {
          const text = normalized(ir.pages[gold.page-1].blocks.map(b=>b.text).join('\n'));
          let offset = 0;
          for (const anchor of gold.anchors) {
            const value = normalized(anchor), at = text.indexOf(value,offset);
            if (at<0) throw new Error(`${engine} declared reading-order gold mismatch: ${anchor}`);
            offset=at+value.length;
          }
        }
      }
      for(const gold of [entry.actual_text_gold,...(entry.actual_text_golds||[])].filter(Boolean)) {
        for(const [engine,ir] of [['native',expected],['WASM',actual.ir]]) {
          const text=ir.pages[gold.page-1].blocks.map(b=>b.text).join('\n');
          if([...text].filter(c=>c==='.').length!==gold.expected_periods) throw new Error(`${engine} ActualText periods mismatch`);
        }
      }
      if(entry.word_group_gold) {
        const gold=entry.word_group_gold;
        for(const [engine,ir] of [['native',expected],['WASM',actual.ir]]) {
          let rows=ir.pages[gold.page-1].blocks.map(b=>b.text);
          if(gold.numeric_labels) rows=rows.filter(t=>/^[0-9 ]+$/.test(t));
          const expectedRows=gold.numeric_labels||gold.expected_blocks;
          if(expectedRows && JSON.stringify([...rows].sort())!==JSON.stringify([...expectedRows].sort()))
            throw new Error(engine+' source word-group multiset mismatch');
          if(gold.selected_labels?.some(t=>!rows.includes(t)))
            throw new Error(engine+' selected source labels missing');
        }
      }
      if (actual.ir.pages.length !== expected.pages.length) throw new Error('Native/WASM page count mismatch');
      const compact = text => text.replace(/\s/gu,'');
      for (let i=0;i<expected.pages.length;i++) {
        const a=expected.pages[i], b=actual.ir.pages[i];
        if (a.rotation !== b.rotation || a.user_unit!==b.user_unit || Math.abs(a.width-b.width)>.01 || Math.abs(a.height-b.height)>.01)
          throw new Error('Native/WASM geometry mismatch on page '+(i+1));
        if ((a.reading_order_requires_review??false)!==(b.reading_order_requires_review??false))
          throw new Error('Native/WASM reading-order diagnostics mismatch on page '+(i+1));
        if (a.sdk_unicode_mapping_errors !== b.sdk_unicode_mapping_errors)
          throw new Error('Native/WASM Unicode mapping diagnostics mismatch on page '+(i+1));
        if (['x0','y0','x1','y1'].some(k=>Math.abs(a.view_box[k]-b.view_box[k])>.01))
          throw new Error('Native/WASM view box mismatch on page '+(i+1));
        const text = p => compact(p.blocks.map(b=>b.text).join(''));
        if (text(a) !== text(b)) {
          const old=text(a), next=text(b);
          let at=0;while(at<old.length && old[at]===next[at])at++;
          const sorted=s=>Array.from(s).sort().join('');
          throw new Error('Native/WASM text mismatch on page '+(i+1)+' '+JSON.stringify({
            native_length:old.length,wasm_length:next.length,same_characters:sorted(old)===sorted(next),
            at,native:old.slice(Math.max(0,at-30),at+60),wasm:next.slice(Math.max(0,at-30),at+60)}));
        }
      }
      if(['w3c-complex-table','w3c-rowspan-table'].includes(entry.id)) {
        const suffix='\n% Empty-shell browser representation-only rewrite\n';
        const rewritten=path.join(binaryDirectory,entry.id+'-rewrite.pdf');
        await fs.writeFile(rewritten,Buffer.concat([data,Buffer.from(suffix)]));
        const process=spawnSync(binary,[file,rewritten],{encoding:'utf8',timeout:60000,maxBuffer:32*1024*1024});
        const reference=JSON.parse(process.stdout);
        const result=await page.evaluate(async ({id,suffix})=>{
          const bytes=new Uint8Array(await(await fetch('/corpus/cache/'+id+'.pdf')).arrayBuffer());
          const tail=new TextEncoder().encode(suffix), next=new Uint8Array(bytes.length+tail.length);
          next.set(bytes);next.set(tail,bytes.length);
          return JSON.parse(window.jadeProbe.compare(bytes,next));
        },{id:entry.id,suffix});
        const expectedExit=3;
        const same=key=>JSON.stringify(result[key])===JSON.stringify(reference[key]);
        if(process.status!==expectedExit || !same('summary') || !same('diagnostics') || !same('comparison')
          || result.diagnostics.old_pages_with_unobserved_text_objects!==1
          || result.diagnostics.new_pages_with_unobserved_text_objects!==1
          || result.comparison.verified_empty_acroform_shells!==2
          || result.comparison.interactive_pages_requiring_review!==0
          || result.pages.some(p=>p.visual.changed_pixels!==0))
          throw new Error('Native/WASM public empty-shell completion coverage mismatch');
        const gold={id:'empty-shell-'+entry.id,passed:true,expected_native_exit:expectedExit,
          verified_empty_acroform_shells:2,interactive_review_pages:0,
          reading_order_review_pages:result.diagnostics.old_pages_with_uncertain_reading_order??0};
        results.push(gold);console.log('PASS '+JSON.stringify(gold));
      }
      results.push({id:entry.id,passed:true,pages:expected.pages.length,png_bytes:actual.png_bytes,ms:Date.now()-started});
    } catch(error) { results.push({id:entry.id,passed:false,error:String(error),ms:Date.now()-started}); }
    console.log((results.at(-1).passed?'PASS ':'FAIL ')+entry.id+' '+JSON.stringify(results.at(-1)));
  }
  const geometryResults = await page.evaluate(async () => {
    const results=[];
    // Independent PDF streams with exact colored rectangles. DOMMatrix applies
    // the public affine coefficients, while Canvas reads the actual PNG pixels.
    const cases=[
      ['-50 -80 350 420','30 40 230 340',0,1],
      ['-450 -480 -50 20','-370 -360 -170 -60',-400,1],
      ['-50 -80 350 420','30 40 500 500',0,1],
      ['-50.25 -80.5 350.5 420.25','30.1 40.2 230.35 340.6',0,1],
      ['-50 -80 350 420','30 40 230 340',0,3],
      ['99950 99920 100350 100420','100030 100040 100230 100340',100000,1],
    ];
    function fixture(media,crop,rotation,shift,unit) {
      const stream=`q 1 0 0 1 ${shift} ${shift} cm 0 g 60 80 40 20 re f 0 0 1 rg 180 290 20 30 re f Q`;
      const objects=['<< /Type /Catalog /Pages 2 0 R >>',
        `<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [${media}] /CropBox [${crop}] /Rotate ${rotation} >>`,
        `<< /Type /Page /Parent 2 0 R /UserUnit ${unit} /Resources << >> /Contents 4 0 R >>`,
        `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`];
      let source='%PDF-1.7\n'; const offsets=[];
      for (const [i,o] of objects.entries()) {offsets.push(source.length);source+=`${i+1} 0 obj\n${o}\nendobj\n`;}
      const start=source.length;
      source+='xref\n0 5\n0000000000 65535 f \n';
      for (const o of offsets) source+=String(o).padStart(10,'0')+' 00000 n \n';
      source+=`trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n${start}\n%%EOF\n`;
      return new TextEncoder().encode(source);
    }
    for (const [index,[media,crop,shift,unit]] of cases.entries()) for (const rotation of [0,90,180,270]) {
      const id=`painted-viewport-${index}-${rotation}`;
      try {
        const bytes=fixture(media,crop,rotation,shift,unit);
        const info=JSON.parse(window.jadeProbe.render_with_info(bytes,1,400));
        const bitmap=await createImageBitmap(new Blob([new Uint8Array(info.png)],{type:'image/png'}));
        const canvas=new OffscreenCanvas(bitmap.width,bitmap.height), context=canvas.getContext('2d');
        context.drawImage(bitmap,0,0);bitmap.close();
        const pixels=context.getImageData(0,0,canvas.width,canvas.height).data;
        const m=info.viewport.pdf_to_pixel;
        const matrix=new DOMMatrix([m.a,m.b,m.c,m.d,m.e,m.f]);
        for (const [blue,box] of [[false,[60,80,100,100]],[true,[180,290,200,320]]]) {
          const actual=[Infinity,Infinity,-Infinity,-Infinity];
          for (let y=0;y<canvas.height;y++) for (let x=0;x<canvas.width;x++) {
            const offset=(y*canvas.width+x)*4, [r,g,b]=pixels.slice(offset,offset+3);
            if(r<30&&g<30&&(blue?b>225:b<30)) {
              actual[0]=Math.min(actual[0],x);actual[1]=Math.min(actual[1],y);
              actual[2]=Math.max(actual[2],x+1);actual[3]=Math.max(actual[3],y+1);
            }
          }
          const points=[[box[0],box[1]],[box[2],box[1]],[box[0],box[3]],[box[2],box[3]]]
            .map(([x,y])=>matrix.transformPoint({x:x+shift,y:y+shift}));
          const expected=[Math.min(...points.map(p=>p.x)),Math.min(...points.map(p=>p.y)),
            Math.max(...points.map(p=>p.x)),Math.max(...points.map(p=>p.y))];
          if(actual.some((v,i)=>!Number.isFinite(v)||Math.abs(v-expected[i])>1))
            throw new Error(JSON.stringify({blue,actual,expected}));
        }
        results.push({id,passed:true});
      } catch(error) { results.push({id,passed:false,error:String(error)}); }
    }
    // Width-only rendering must reject unreasonable aspect ratios and area too.
    let rejected=0;
    for (const media of ['0 0 1 100000','0 0 100000 1','0 0 2000 2001']) {
      try { window.jadeProbe.render(fixture(media,media,0,0,1),1,8192); }
      catch(error) { if(error instanceof WebAssembly.RuntimeError) throw error; rejected++; }
    }
    results.push({id:'render-resource-limits',passed:rejected===3,rejected});
    return results;
  });
  results.push(...geometryResults);
  const appearances=await page.evaluate(()=>{
    const cases=JSON.parse(window.jadeProbe.appearance_cases());
    return cases.map(name=>{
      try {
        const result=JSON.parse(window.jadeProbe.appearance_case(name));
        if(name.startsWith('oversized_widget_')) return {
          id:'appearance-'+name,passed:result.rejected&&result.error.includes('2000000 UTF-16')&&result.same_complete&&result.same_source,
          rejected:result.rejected,error:result.error};
        const diff=result.diff, summary=diff.summary;
        let passed=result.same_complete&&result.same_source;
        if(name.startsWith('agl_')) {
          const mode=name.slice(4);
          const supported=['uacute','ugrave'].includes(mode);
          const known=['union','universal','uacute','ugrave','nonbmp','multicomponent'].includes(mode);
          passed&&=result.complete===supported&&result.text_complete===supported&&summary.pages_visual_changed===0
            &&(supported?summary.blocks_changed===1&&summary.pages_added===0&&summary.pages_removed===0:
              summary.pages_added===1&&summary.pages_removed===1&&Object.entries(summary).filter(([k])=>!['pages_added','pages_removed'].includes(k)).every(([,v])=>v===0))
            &&(diff.diagnostics.new_pages_with_unavailable_source_unicode??0)===Number(!known)
            &&(diff.diagnostics.new_pages_with_source_unicode_mismatch??0)===Number(known&&!supported)
            &&(diff.diagnostics.new_pages_with_unobserved_text_objects??0)===Number(!supported)
            &&(diff.diagnostics.new_pages_with_missing_source_characters??0)===Number(!supported);
        } else if(name==='actual_space_separator') {
          passed&&=!result.complete&&!result.text_complete&&summary.pages_visual_changed===0
            &&summary.blocks_changed===1&&summary.pages_added===0&&summary.pages_removed===0;
        } else if(name.startsWith('marked_scope_')) {
          const mode=name.slice('marked_scope_'.length);
          passed&&=!result.complete&&!result.text_complete&&summary.pages_visual_changed===0
            &&(mode==='no_object'?Object.values(summary).every(v=>v===0):summary.blocks_changed>0&&summary.pages_added===0&&summary.pages_removed===0)
            &&(mode!=='nested_actual'||summary.blocks_changed===2);
        } else if(name.startsWith('actual_structure_')) {
          const mode=name.slice('actual_structure_'.length);
          const ambiguous=['collision','reused_form','bad_parent','bad_backpointer','crosspage','nested_actual'].includes(mode);
          const empty=mode==='empty';
          passed&&=result.complete===!ambiguous&&result.text_complete===(!ambiguous&&!empty)
            &&summary.pages_visual_changed===0
            &&(ambiguous?Object.values(summary).every(v=>v===0):empty?diff.pages.some(p=>p.blocks.some(b=>b.old_text==='A'&&b.new_text===null)):summary.blocks_changed===1);
        } else if(name.startsWith('source_unicode_')) {
          const actual=['source_unicode_actual','source_unicode_actual_named','source_unicode_actual_large'].includes(name);
          const invalid=name==='source_unicode_actual_invalid';
          const empty=['source_unicode_actual_empty','source_unicode_actual_blank'].includes(name);
          passed&&=result.complete===(actual||empty)&&result.text_complete===actual
            &&summary.pages_visual_changed===0
            &&(diff.diagnostics.new_pages_with_source_unicode_mismatch??0)===Number(!actual&&!empty&&!invalid)
            &&(diff.diagnostics.old_pages_with_source_unicode_mismatch??0)===0
            &&(actual||empty||invalid||Object.values(summary).every(v=>v===0))
            &&(!invalid||(diff.diagnostics.new_pages_with_unavailable_source_unicode??0)===1);
        } else if(name.startsWith('source_')) {
          const unavailable=name==='source_trailing_invalid';
          passed&&=!result.complete&&!result.text_complete
            &&(diff.diagnostics.old_pages_with_missing_source_characters??0)===0
            &&(diff.diagnostics.new_pages_with_missing_source_characters??0)===Number(!unavailable)
            &&(diff.diagnostics.new_pages_with_unavailable_source_accounting??0)===Number(unavailable)
            &&summary.pages_visual_changed===Number(name==='source_duplicate_visible')
            &&Object.entries(summary).filter(([key])=>!['pages_visual_changed','pages_modified'].includes(key)).every(([,value])=>value===0);
        } else if(name.startsWith('oblique_')) {
          const edit=name==='oblique_edit';
          passed&&=result.complete&&result.text_complete
            &&summary.pages_geometry_changed===Number(!edit)
            &&summary.pages_visual_changed===1
            &&(edit?summary.blocks_changed+summary.blocks_added+summary.blocks_removed>0:
              summary.blocks_changed+summary.blocks_added+summary.blocks_removed===0)
            &&(edit||summary.blocks_moved+summary.blocks_resized+summary.blocks_format_changed===0);
        } else if(name.startsWith('angled_label_')) {
          const edit=name==='angled_label_edit', rotated=name.endsWith('rotation');
          passed&&=result.complete&&result.text_complete
            &&summary.pages_geometry_changed===Number(!edit)
            &&summary.pages_visual_changed===Number(edit||rotated)
            &&(edit?summary.blocks_changed+summary.blocks_added+summary.blocks_removed>0:
              summary.blocks_changed+summary.blocks_added+summary.blocks_removed===0)
            &&(edit||summary.blocks_moved+summary.blocks_resized+summary.blocks_format_changed===0);
        } else if(name.startsWith('unobserved_')) {
          passed&&=!result.complete&&!result.text_complete
            &&diff.diagnostics.old_pages_with_unobserved_text_objects===1
            &&diff.diagnostics.new_pages_with_unobserved_text_objects===1
            &&summary.blocks_changed===0&&summary.pages_visual_changed===0
            &&summary.pages_geometry_changed===Number(name==='unobserved_units');
        } else if(name==='surrogate_unpaired') {
          passed&&=!result.complete&&!result.text_complete&&Object.values(summary).every(v=>v===0);
        } else if(name.startsWith('supplementary_unicode')) {
          const edit=name==='supplementary_unicode_edit';
          passed&&=result.complete&&result.text_complete&&summary.blocks_changed===Number(edit)
            &&summary.pages_visual_changed===0&&summary.blocks_added===0&&summary.blocks_removed===0
            &&summary.pages_geometry_changed===Number(!edit);
        } else if(name.startsWith('form_orphan_')) {
          const cycle=name==='form_orphan_cycle';
          passed&&=!result.complete&&result.text_complete
            &&summary.canonical_form_fields_changed===(cycle?0:1)
            &&summary.pages_visual_changed===0&&summary.form_fields_changed===0
            &&diff.comparison.canonical_fields_examined===(cycle?0:name==='form_orphan_partial'?4:2)
            &&diff.comparison.canonical_form_review_reasons.length>0;
        } else if(name==='form_catalog_invalid') {
          passed&&=!result.complete&&result.text_complete
            &&summary.canonical_form_fields_changed===0&&summary.pages_visual_changed===0
            &&diff.comparison.canonical_form_review_reasons.length===2;
        } else if(name==='form_catalog_null') {
          passed&&=result.complete&&result.text_complete&&Object.values(summary).every(v=>v===0)
            &&diff.comparison.canonical_form_review_reasons.length===0;
        } else if(name==='form_null_entries') {
          passed&&=!result.complete&&result.text_complete&&Object.values(summary).every(v=>v===0)
            &&diff.comparison.canonical_fields_examined===2
            &&diff.comparison.canonical_form_review_reasons.length===0;
        } else if(name==='form_repeated_widget') {
          passed&&=!result.complete&&summary.canonical_form_fields_changed===1
            &&summary.form_fields_changed===0&&summary.pages_visual_changed===0
            &&diff.comparison.canonical_fields_examined===4
            &&diff.comparison.canonical_form_review_reasons.length===0;
        } else if(name.startsWith('empty_form_')) {
          const accepted=['empty_form_shell','empty_form_shell_edit'].includes(name);
          const changed=Number(name==='empty_form_shell_edit');
          passed&&=summary.blocks_changed===changed&&summary.pages_visual_changed===changed
            &&result.complete===accepted&&result.text_complete
            &&(diff.comparison.verified_empty_acroform_shells??0)===(accepted?2:0)
            &&diff.comparison.interactive_pages_requiring_review===(accepted?0:1)
            &&diff.comparison.canonical_fields_examined===(name==='empty_form_orphan_widget'?2:0);
        } else if(name==='crop_table_identity') {
          passed&&=summary.pages_added===0&&summary.pages_removed===0&&summary.pages_geometry_changed===1
            &&summary.blocks_changed===0&&summary.pages_visual_changed===1&&!result.complete
            &&diff.diagnostics.old_pages_with_unobserved_text_objects===1
            &&diff.diagnostics.new_pages_with_unobserved_text_objects===1;
        } else if(name==='tagged_table_diagonal') {
          passed&&=summary.pages_geometry_changed===1&&summary.blocks_changed===0
            &&summary.blocks_added===0&&summary.blocks_removed===0&&summary.pages_visual_changed===0
            &&!result.complete&&!result.text_complete
            &&diff.diagnostics.old_pages_with_uncertain_reading_order===1
            &&diff.diagnostics.new_pages_with_uncertain_reading_order===1;
        } else if(name.startsWith('tagged_table_stale_')) {
          const changed=Number(name==='tagged_table_stale_edit');
          passed&&=summary.pages_geometry_changed===(changed?0:1)&&summary.blocks_changed===changed
            &&summary.blocks_added===0&&summary.blocks_removed===0&&summary.pages_visual_changed===changed
            &&!result.complete&&!result.text_complete
            &&diff.diagnostics.old_pages_with_uncertain_reading_order===1
            &&diff.diagnostics.new_pages_with_uncertain_reading_order===1
            &&diff.comparison.interactive_pages_requiring_review===0;
        } else if(name==='tagged_table_spans_edit') {
          passed&&=summary.pages_geometry_changed===0&&summary.blocks_changed===1
            &&summary.blocks_added===0&&summary.blocks_removed===0&&summary.pages_visual_changed===1&&result.complete;
        } else if(['layout_units','layout_units_edit','space_units','construction_units','construction_shared_units','tagged_table_units','tagged_table_edit','tagged_table_spans_units','tagged_table_spans_rtl','tagged_table_shear_units','tagged_table_shear_edit'].includes(name)) {
          const changed=Number(name==='layout_units_edit'||name==='tagged_table_edit'||name==='tagged_table_shear_edit');
          const whitespaceObject=['tagged_table_units','tagged_table_edit','tagged_table_shear_units','tagged_table_shear_edit'].includes(name);
          passed&&=result.complete===!whitespaceObject
            &&(diff.diagnostics.old_pages_with_unobserved_text_objects??0)===Number(whitespaceObject)
            &&(diff.diagnostics.new_pages_with_unobserved_text_objects??0)===Number(whitespaceObject)
            &&summary.pages_geometry_changed===1&&summary.blocks_changed===changed
            &&summary.blocks_added===0&&summary.blocks_removed===0&&summary.blocks_moved===0
            &&summary.blocks_format_changed===0&&summary.pages_visual_changed===changed
            &&(changed||summary.blocks_resized===0)
            &&diff.pages[0].blocks[0].old_font_size===12&&diff.pages[0].blocks[0].new_font_size===12/64;
        } else if(['widget_units','widget_units_edit'].includes(name)) {
          const changed=Number(name==='widget_units_edit');
          passed&&=summary.form_fields_changed===2*changed&&summary.canonical_form_fields_changed===changed
            &&diff.comparison.widget_fields_examined===4&&!result.complete
            &&diff.pages[0].form_changes.every(f=>f.old&&f.new&&Math.abs(f.old.bounds.x0-72*f.new.bounds.x0)<.001);
        } else if(name==='user_unit_scale')
          passed&&=summary.pages_geometry_changed===1&&summary.pages_visual_changed===1&&result.complete
            &&diff.pages[0].old_size.user_unit===1&&diff.pages[0].new_size.user_unit===3
            &&diff.pages[0].visual.old_viewport.width===600&&diff.pages[0].visual.new_viewport.width===1800;
        else if(['user_unit_invalid','user_unit_limit'].includes(name))
          passed&&=!result.complete&&diff.comparison.skipped_page_pairs===1&&diff.comparison.pixel_budget_used===0
            &&(name!=='user_unit_invalid'||diff.diagnostics.old_pages_with_unknown_units===1);
        else if(['partial_scan','image_only','path','text_color','text_face','line_hyphen'].includes(name))
          passed&&=summary.pages_visual_changed===1&&result.complete&&diff.pages[0].visual.changed_pixels>0;
        else if(['form_value','form_flags','form_hidden','form_unicode','checkbox','checkbox_export','checkbox_unchecked_export','list','combo_custom','combo_no_options','choice_export_label'].includes(name))
          passed&&=summary.form_fields_changed===1&&summary.pages_visual_changed===0&&!result.complete;
        else if(name==='image_reencoded')
          passed&&=Object.values(summary).every(v=>v===0)&&result.complete;
        else if(name==='tounicode_overrides')
          passed&&=Object.values(summary).every(v=>v===0)&&result.complete&&result.text_complete;
        else if(name==='tounicode_override_edit')
          passed&&=summary.blocks_changed===1&&summary.pages_visual_changed===0&&result.complete&&result.text_complete
            &&diff.pages[0].blocks[0].old_text==='ó€éê'&&diff.pages[0].blocks[0].new_text==='ò€éê';
        else if(name==='form_deep_property')
          passed&&=summary.canonical_form_fields_changed===1&&summary.form_fields_changed===0
            &&summary.pages_visual_changed===0&&!result.complete
            &&diff.comparison.canonical_form_review_reasons.length===0;
        else if(['form_unplaced','form_multi_export'].includes(name)) {
          passed&&=summary.canonical_form_fields_changed===1&&summary.form_fields_changed===0
            &&summary.pages_visual_changed===0&&!result.complete
            &&diff.comparison.canonical_fields_examined===2&&diff.comparison.canonical_form_review_reasons.length===0;
          const change=diff.canonical_form_changes[0];
          if(name==='form_unplaced') passed&&=change.old.properties.V==='original'&&change.new.properties.V==='changed';
          else passed&&=JSON.stringify(change.old.properties.V)==='["one","two"]'&&JSON.stringify(change.new.properties.V)==='["one","three"]';
        }
        else if(['form_hybrid_xfa','form_partial_property','form_stream_reencoded','form_corrupt_stream'].includes(name)) {
          const c=diff.comparison;
          passed&&=summary.form_fields_changed===0&&summary.pages_visual_changed===0&&c.canonical_fields_examined===2&&!result.complete;
          if(name==='form_stream_reencoded') passed&&=summary.canonical_form_fields_changed===0&&c.canonical_properties_unavailable===0&&c.canonical_form_review_reasons.length===0;
          else {
            const change=diff.canonical_form_changes[0];
            passed&&=summary.canonical_form_fields_changed===1;
            if(name==='form_hybrid_xfa') passed&&=change.new.properties.V==='changed'&&c.canonical_form_review_reasons.some(r=>r.includes('XFA semantics'));
            else passed&&=change.old.properties.Ff===0&&change.new.properties.Ff===1&&change.new.unavailable_properties.V&&c.canonical_properties_unavailable>0;
          }
        }
        else if(name==='radio_reordered')
          passed&&=Object.values(summary).every(v=>v===0)&&diff.comparison.widget_fields_examined===4&&!result.complete;
        else if(name.startsWith('mapping_error_'))
          passed&&=Object.values(summary).every(v=>v===0)&&!result.complete&&!result.text_complete
            &&diff.diagnostics.old_pages_with_unmapped_text===1&&diff.diagnostics.new_pages_with_unmapped_text===1;
        else if(['budget','oversized','oversized_image','nested_oversized_image'].includes(name))
          passed&&=!result.complete&&diff.comparison.skipped_page_pairs===1;
        else throw new Error('Unrecognized appearance case');
        if(name==='form_flags'){const f=diff.pages[0].form_changes[0];
          passed&&=f.old.flags===0&&f.new.flags===3&&f.old.value===f.new.value;}
        if(name==='line_hyphen') passed&&=diff.pages[0].blocks.some(b=>b.old_text==='well-'&&b.new_text==='well=');
        if(name==='checkbox_export'){const f=diff.pages[0].form_changes[0];
          passed&&=f.old.checked===true&&f.new.checked===true;}
        if(name==='checkbox_unchecked_export'){const f=diff.pages[0].form_changes[0];
          passed&&=f.old.checked===false&&f.new.checked===false&&f.old.value===f.new.value&&f.old.export_value==='Yes'&&f.new.export_value==='Yep';}
        if(name==='choice_export_label'){const f=diff.pages[0].form_changes[0];
          passed&&=f.old.value==='foo'&&f.new.value==='bar'&&f.old.display_value==='Foo'&&f.new.display_value==='Bar';}
        return {id:'appearance-'+name,passed,summary,complete:result.complete,
          coverage:diff.comparison,changed_pixels:diff.pages.map(p=>p.visual?.changed_pixels)};
      }catch(error){if(error instanceof WebAssembly.RuntimeError)throw error;
        return {id:'appearance-'+name,passed:false,error:String(error)};}
    });
  });
  results.push(...appearances);
  const overflow = await page.evaluate(async () => {
    const count=65537;
    const objects=['<< /Type /Catalog /Pages 2 0 R >>',
      `<< /Type /Pages /Count ${count} /MediaBox [0 0 200 200] /Resources << >> /Kids [${Array.from({length:count},(_,i)=>`${i+3} 0 R`).join(' ')}] >>`,
      ...Array.from({length:count},()=> '<< /Type /Page /Parent 2 0 R >>')];
    let source='%PDF-1.7\n';const offsets=[];
    for(const [i,o] of objects.entries()){offsets.push(source.length);source+=`${i+1} 0 obj\n${o}\nendobj\n`;}
    const start=source.length;
    source+=`xref\n0 ${objects.length+1}\n0000000000 65535 f \n`;
    source+=offsets.map(o=>String(o).padStart(10,'0')+' 00000 n \n').join('');
    source+=`trailer\n<< /Size ${objects.length+1} /Root 1 0 R >>\nstartxref\n${start}\n%%EOF\n`;
    const bytes=new TextEncoder().encode(source);
    let rejected=0;
    for(const action of [()=>window.jadeProbe.inspect(bytes),()=>window.jadeProbe.render(bytes,1,400),
      ()=>window.jadeProbe.render_at_dpi(bytes,1,72),()=>window.jadeProbe.layer(bytes,1)]) {
      try{action();}catch(error){if(error instanceof WebAssembly.RuntimeError)throw error;
        if(!String(error).includes('65537'))throw error;rejected++;}
    }
    const valid=new Uint8Array(await(await fetch('/corpus/cache/hello.pdf')).arrayBuffer());
    return {rejected,recovered:JSON.parse(window.jadeProbe.inspect(valid)).pages.length===1};
  });
  results.push({id:'full-page-count-before-u16-truncation',passed:overflow.rejected===4&&overflow.recovered,...overflow});
  for (const entry of manifest.robustness || []) {
    const data=await fs.readFile(path.join(root,'corpus/cache',entry.id+'.pdf'));
    if(createHash('sha256').update(data).digest('hex')!==entry.sha256) throw new Error('Robustness PDF checksum mismatch');
    const result=await page.evaluate(async entry=>{
      const bytes=new Uint8Array(await(await fetch('/corpus/cache/'+entry.id+'.pdf')).arrayBuffer());
      try {
        if(entry.outcome==='geometry_rejected'||entry.outcome==='password_required') {
          let rejected=0;
          for(const action of [()=>window.jadeProbe.inspect(bytes),()=>window.jadeProbe.render(bytes,entry.invalid_page,400),
            ()=>window.jadeProbe.render_at_dpi(bytes,entry.invalid_page,72),()=>window.jadeProbe.layer(bytes,entry.invalid_page)]) {
            try {action();} catch(error) {if(error instanceof WebAssembly.RuntimeError)throw error;
              if(entry.outcome==='password_required'&&!String(error).includes('password'))throw error;rejected++;}
          }
          return {passed:rejected===4,rejected};
        }
        const ir=JSON.parse(window.jadeProbe.inspect(bytes));
        for(let p=1;p<=ir.pages.length;p++)window.jadeProbe.render(bytes,p,400);
        return {passed:ir.pages.length===entry.pages};
      } catch(error) {return {passed:false,error:String(error)};}
    },entry);
    results.push({id:entry.id,...result});
  }
  // Exercise both byte inputs and the diff JSON contract with a generated fixture pair.
  const comparison = await page.evaluate(async () => {
    const a=window.jadeProbe.generated_report(false), b=window.jadeProbe.generated_report(true);
    const same=JSON.parse(window.jadeProbe.compare(a,a));
    const changed=JSON.parse(window.jadeProbe.compare(a,b));
    return {same:same.summary,changed:changed.summary};
  });
  const comparisonPassed = Object.values(comparison.same).every(n=>n===0)
    && comparison.changed.pages_added===1 && comparison.changed.blocks_changed>=1;
  results.push({id:'byte-diff-api',passed:comparisonPassed,...comparison});
  const budget = await page.evaluate(async () => {
    const a=window.jadeProbe.generated_report(false), b=window.jadeProbe.generated_report(true);
    const limited=JSON.parse(window.jadeProbe.compare_with_budget(a,b,0));
    const hello=new Uint8Array(await(await fetch('/corpus/cache/hello.pdf')).arrayBuffer());
    const same=JSON.parse(window.jadeProbe.compare_with_budget(hello,hello,0));
    return {limited:limited.diff.diagnostics,complete:limited.complete,same_complete:same.complete};
  });
  results.push({id:'alignment-budget-diagnostic',passed:!budget.complete&&budget.same_complete
    &&(budget.limited.page_alignment_limited||budget.limited.block_alignment_limited_pages>0),...budget});
  const invalidInputs = await page.evaluate(async () => {
    const bytes=new Uint8Array(await (await fetch('/corpus/cache/hello.pdf')).arrayBuffer());
    const invalid=new TextEncoder().encode('not a PDF');
    let rejected=0;
    for (const action of [()=>window.jadeProbe.inspect(invalid),
      ()=>window.jadeProbe.compare(bytes,invalid),
      ()=>window.jadeProbe.render(bytes,0,400),
      ()=>window.jadeProbe.render(bytes,1,0)]) {
      try { action(); } catch(error) {
        if (error instanceof WebAssembly.RuntimeError) throw error;
        rejected++;
      }
    }
    const recovered=JSON.parse(window.jadeProbe.inspect(bytes)).pages.length===1;
    return {rejected,recovered};
  });
  results.push({id:'invalid-inputs-and-recovery',passed:invalidInputs.rejected===4 && invalidInputs.recovered,...invalidInputs});
  const wasm = await fs.readFile(path.join(root,'corpus/wasm-runtime/probe/jade_pdf_wasm_smoke_bg.wasm'));
  const report = {version:3,browser:browser.version(),pdfium:runtime.pdfium.version,
    pdfium_archive_sha256:runtime.pdfium.sha256,pdfium_wasm_sha256:runtime.pdfium.members['lib/pdfium.wasm'].sha256,
    wasm_sha256:createHash('sha256').update(wasm).digest('hex'),
    runtime_adapter_sha256:adapterHash,pdfium_loader_sha256:loaderHash,
    unprepared_loader_sha256:unpreparedLoaderHash,contract_probe_sha256:createHash('sha256').update(contractProbe).digest('hex'),
    manifest_sha256:createHash('sha256').update(manifestBytes).digest('hex'),native_binary_sha256:binaryHash,
    selected_documents:documents.map(e=>e.id),
    cases:results.length,passed:results.filter(r=>r.passed).length,results};
  await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');
  console.log(`${report.passed}/${report.cases} passed`);
  if (report.passed!==report.cases) process.exitCode=1;
} finally {
  if (browser) await browser.close();
  await new Promise(resolve=>server.close(resolve));
  await fs.rm(binaryDirectory,{recursive:true,force:true});
}
