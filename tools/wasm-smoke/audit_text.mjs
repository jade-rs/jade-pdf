// Independent text inventory. Agreement is evidence, not Unicode ground truth.
// Only the manifest's size/SHA-pinned public PDFs are accepted by the coordinator.
import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const self = fileURLToPath(import.meta.url);
const root = path.resolve(path.dirname(self), '../..');
const { values } = parseArgs({ options: {
  binary: {type:'string',default:path.join(root,'target/release/pdf-diff')},
  report: {type:'string',default:path.join(root,'corpus/results-independent-text.json')},
  only: {type:'string',multiple:true},
  extract: {type:'string'},
}});
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const pdfjsRoot = path.join(root,'tools/wasm-smoke/node_modules/pdfjs-dist');

if (values.extract) {
  const pdfjs = await import('./node_modules/pdfjs-dist/legacy/build/pdf.mjs');
  const task = pdfjs.getDocument({
    data:new Uint8Array(await fs.readFile(values.extract)),
    standardFontDataUrl:pdfjsRoot+'/standard_fonts/',
    cMapUrl:pdfjsRoot+'/cmaps/', cMapPacked:true, wasmUrl:pdfjsRoot+'/wasm/',
    useSystemFonts:false, disableFontFace:true, isEvalSupported:false,
    enableXfa:false, verbosity:0,
  });
  try {
    const document = await task.promise;
    const pages = [];
    for (let index=1;index<=document.numPages;index++) {
      const page = await document.getPage(index);
      const content = await page.getTextContent({disableNormalization:false});
      pages.push({index,text:content.items.map(item=>item.str??'').join('')});
      page.cleanup();
    }
    process.stdout.write(JSON.stringify({version:pdfjs.version,pages}));
  } finally { await task.destroy(); }
} else {
  const manifestBytes = await fs.readFile(path.join(root,'corpus/manifest.json'));
  const manifest = JSON.parse(manifestBytes);
  if (values.only?.some(id=>!manifest.documents.some(e=>e.id===id))) throw new Error('Unknown corpus ID');
  const documents = manifest.documents.filter(e=>!values.only||values.only.includes(e.id));
  const directory = await fs.mkdtemp(path.join(os.tmpdir(),'jade-independent-text-'));
  const binary = path.join(directory,'pdf-diff');
  await fs.copyFile(values.binary,binary);
  await fs.chmod(binary,0o700);
  const binaryHash = hash(await fs.readFile(binary));
  const results = [];
  const ligatures={'ﬀ':'ff','ﬁ':'fi','ﬂ':'fl','ﬃ':'ffi','ﬄ':'ffl','ﬅ':'st','ﬆ':'st'};
  const normalized = text => text.replace(/[ﬀﬁﬂﬃﬄﬅﬆ]/gu,c=>ligatures[c])
    .replace(/ſ/gu,'s').replace(/\u00ad/gu,'').normalize('NFC').replace(/[\p{White_Space}\p{Cc}]/gu,'');
  const inventory = text => {
    const result=new Map();
    for (const character of normalized(text)) result.set(character,(result.get(character)??0)+1);
    return result;
  };
  const delta=(a,b)=>[...a].flatMap(([character,count])=>{
    const difference=count-(b.get(character)??0);
    return difference>0?[[character,difference]]:[];
  });
  const sum = entries => entries.reduce((total,[,n])=>total+n,0);
  const run = (command,args) => {
    const result=spawnSync(command,args,{encoding:'utf8',timeout:60000,maxBuffer:32*1024*1024});
    if (result.status!==0) {
      const error=new Error((result.stderr||String(result.error)).slice(0,2500));
      error.resourceLimited=result.error?.code==='ETIMEDOUT'||/heap out of memory|heap limit/i.test(result.stderr??'');
      throw error;
    }
    return JSON.parse(result.stdout);
  };
  try {
    for (const entry of documents) {
      const result={id:entry.id,status:'error',pages:0};
      try {
        const source=path.join(root,'corpus/cache',entry.id+'.pdf');
        const bytes=await fs.readFile(source);
        if(bytes.length!==entry.bytes||hash(bytes)!==entry.sha256) throw new Error('PDF size/SHA mismatch');
        const native=run(binary,['--ir',source]);
        const independent=run(process.execPath,['--max-old-space-size=512',self,'--extract',source]);
        if(native.pages.length!==independent.pages.length||native.pages.length!==entry.pages) throw new Error('Page count differs');
        let nativeCount=0,independentCount=0,missing=0,extra=0;
        const affected=[];
        for(let i=0;i<native.pages.length;i++) {
          const text=native.pages[i].blocks.map(block=>block.text).join('');
          const a=inventory(text),b=inventory(independent.pages[i].text);
          const lost=delta(b,a),added=delta(a,b);
          nativeCount+=sum([...a]);independentCount+=sum([...b]);
          missing+=sum(lost);extra+=sum(added);
          if(lost.length||added.length) affected.push({page:i+1,
            missing_from_native:lost,extra_in_native:added,
            native_text_sample:text.slice(0,300),independent_text_sample:independent.pages[i].text.slice(0,300)});
        }
        Object.assign(result,{status:affected.length?'disagreement':'agreement',pages:native.pages.length,
          independent_version:independent.version,native_characters:nativeCount,
          independent_characters:independentCount,missing_from_native:missing,extra_in_native:extra,affected_pages:affected});
      } catch(error) {
        result.status=error.resourceLimited?'resource_limited':'error';
        result.error=String(error);
      }
      results.push(result);
      console.log(result.status.toUpperCase(),result.id,result.missing_from_native??'',result.extra_in_native??'');
    }
    const report={version:1,manifest_sha256:hash(manifestBytes),binary_sha256:binaryHash,
      package_lock_sha256:hash(await fs.readFile(path.join(root,'tools/wasm-smoke/package-lock.json'))),
      tool_sha256:hash(await fs.readFile(self)),
      methodology:'PDF.js with Unicode normalization enabled vs native per-page scalar inventories after NFC, explicit Latin ligature/long-s expansion and whitespace/control/soft-hyphen removal. Reading order and painting are not checked. Disagreements require adjudication; PDF.js is not ground truth. Each independent worker uses a 512MiB V8 old-space setting and 60s timeout; this is not a total RSS limit.',
      documents:results.length,pages:results.reduce((n,r)=>n+r.pages,0),
      requested_pages:documents.reduce((n,d)=>n+d.pages,0),
      completed_pages:results.reduce((n,r)=>n+r.pages,0),
      agreeing_documents:results.filter(r=>r.status==='agreement').length,
      disagreeing_documents:results.filter(r=>r.status==='disagreement').length,
      resource_limited_documents:results.filter(r=>r.status==='resource_limited').length,
      errors:results.filter(r=>r.status==='error').length,results};
    await fs.writeFile(values.report,JSON.stringify(report,null,2)+'\n');
    console.log(`${report.agreeing_documents} agree, ${report.disagreeing_documents} disagree, ${report.resource_limited_documents} resource limited, ${report.errors} errors`);
    // A completed audit can expose differences. It must not call them passing tests.
    if(report.errors) process.exitCode=1;
    else if(report.disagreeing_documents||report.resource_limited_documents) process.exitCode=3;
  } finally { await fs.rm(directory,{recursive:true,force:true}); }
}
