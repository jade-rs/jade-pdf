import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import {pathToFileURL,fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const [manifestFile,probeDir,outputDir,engine='chromium']=process.argv.slice(2);
const manifest=JSON.parse(await fs.readFile(manifestFile,'utf8'));
const { [engine]: browserType }=await import(pathToFileURL(path.join(root,'tools/wasm-smoke/node_modules/playwright/index.mjs')).href);
const routes=new Map([['/',{bytes:Buffer.from('<!doctype html><title>Owned DOCX geometry controls</title>'),type:'text/html'}]]);
for(const name of ['jade_pdf2docx_wasm.js','jade_pdf2docx_wasm_bg.wasm']) routes.set('/'+name,{bytes:await fs.readFile(path.join(probeDir,name)),type:name.endsWith('.wasm')?'application/wasm':'application/javascript'});
for(const item of manifest.cases) routes.set('/cases/'+item.id,{bytes:await fs.readFile(item.input),type:'application/pdf'});
const server=http.createServer((request,response)=>{
 const entry=routes.get(new URL(request.url,'http://localhost').pathname);
 if(!entry){response.writeHead(404);response.end();return;}
 response.writeHead(200,{'Content-Type':entry.type,'Content-Security-Policy':"default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'"});response.end(entry.bytes);
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
let browser;const rows=[];
try {
 browser=await browserType.launch({headless:true});const page=await browser.newPage();
 await page.goto('http://127.0.0.1:'+server.address().port+'/');
 await page.evaluate(async()=>{const api=await import('/jade_pdf2docx_wasm.js');await api.default();globalThis.docxApi=api;});
 for(const item of manifest.cases){
  const result=await page.evaluate(async id=>{
   const bytes=new Uint8Array(await(await fetch('/cases/'+id)).arrayBuffer()),api=globalThis.docxApi;
   let inspection;try{inspection={value:JSON.parse(api.inspectPdfJson(bytes))};}catch(e){inspection={error:String(e)};}
   const outputs=[];
   for(const [mode,fn] of [['default',api.convertPdfToDocx],['strict',api.convertPdfToDocxStrict]]){
    try{outputs.push({mode,bytes:Array.from(fn(bytes))});}catch(e){outputs.push({mode,error:String(e)});}
   }
   return {inspection,outputs};
  },item.id);
  for(const row of result.outputs)if(row.bytes){row.file=path.join(outputDir,item.id+'-'+row.mode+'.docx');await fs.writeFile(row.file,Buffer.from(row.bytes));delete row.bytes;}
  rows.push({id:item.id,...result});
 }
 await fs.writeFile(path.join(outputDir,'browser-raw.json'),JSON.stringify({engine,browser:browser.version(),rows},null,2)+'\n');
}finally{await browser?.close();await new Promise(resolve=>server.close(resolve));}
