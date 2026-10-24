import fs from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const {chromium}=await import(process.env.JADE_PDF_PLAYWRIGHT_PATH?pathToFileURL(process.env.JADE_PDF_PLAYWRIGHT_PATH).href:'playwright');
const manifestBytes=await fs.readFile(path.join(root,'corpus/manifest.json')), manifest=JSON.parse(manifestBytes);
const selected=process.argv.slice(2), entries=[...manifest.documents,...manifest.robustness].filter(d=>!selected.length||selected.includes(d.id));
if(!entries.length || selected.some(id=>!entries.some(d=>d.id===id))) throw new Error('Unknown corpus selection');
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
for(const d of entries) if(hash(await fs.readFile(path.join(root,'corpus/cache',d.id+'.pdf')))!==d.sha256) throw new Error('PDF checksum mismatch: '+d.id);
const reportPath=process.env.JADE_PDF_WORKER_REPORT || path.join(root,'corpus/results-browser-workers.json');
await fs.rm(reportPath,{force:true});
const server=http.createServer(async(request,response)=>{
  try {
    const url=new URL(request.url,'http://localhost');
    response.setHeader('Content-Security-Policy',"default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; worker-src 'self'");
    if(url.pathname==='/') {response.end('<!doctype html><title>PDF Worker checks</title>'); return;}
    const target=path.resolve(root,'.'+decodeURIComponent(url.pathname));
    if(!target.startsWith(root+path.sep)) {response.writeHead(403).end();return;}
    response.setHeader('Content-Type',({'.js':'application/javascript','.mjs':'application/javascript','.wasm':'application/wasm','.json':'application/json'})[path.extname(target)]||'application/octet-stream');
    response.end(await fs.readFile(target));
  } catch {response.writeHead(404).end();}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
let browser;
const results=[],documents=[];
try {
  browser=await chromium.launch({headless:true,...(process.env.JADE_PDF_BROWSER?{executablePath:process.env.JADE_PDF_BROWSER}:{})});
  const page=await browser.newPage();
  page.on('console',m=>{if(m.type()==='error')process.stderr.write(m.text()+'\n');});
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.evaluate(async()=>{
    const {PdfWorkerClient}=await import('/crates/jade-pdf-diff/wasm/worker_client.mjs');
    const probe=await import('/corpus/wasm-runtime/probe/jade_pdf_wasm_smoke.js');
    const local=await probe.default();
    const {default:create}=await import('/corpus/wasm-runtime/release/node/pdfium.esm.js');
    if(!probe.initialize_pdfium_render(await create({locateFile:n=>'/corpus/wasm-runtime/release/node/'+n}),local,false)) throw new Error('Main SDK initialization failed');
    window.probe=probe; window.PdfWorkerClient=PdfWorkerClient;
    window.client=new PdfWorkerClient('/tools/wasm-smoke/worker_adapter.mjs');
    const NativeWorker=window.Worker;
    window.workerStats={created:0,terminated:0};
    window.Worker=class extends NativeWorker {
      constructor(...args) {super(...args);window.workerStats.created++;}
      terminate() {window.workerStats.terminated++;return super.terminate();}
    };
    window.checkPair=async function(operation,inputs,params={}) {
      let direct,expectedError;
      try {
        if(operation==='renderWidth'||operation==='renderDpi') {
          const result=probe.worker_render(inputs[0],JSON.stringify(params),operation==='renderDpi');
          direct={png:result.png,metadata:JSON.parse(result.metadata)};
        } else direct=JSON.parse(probe.worker_json(operation,inputs[0],inputs[1]||new Uint8Array(),JSON.stringify(params)));
      } catch(error) {if(error instanceof WebAssembly.RuntimeError)throw error;expectedError=String(error);}
      let actual,error;
      try {actual=await window.client.run(operation,inputs,params);}
      catch(e) {error={code:e.code,message:e.message};}
      let passed=expectedError!==undefined ? error?.code==='PDF_ERROR' && error.message===expectedError : !error;
      if(passed && expectedError===undefined) {
        if(operation==='renderWidth'||operation==='renderDpi') {
          passed=JSON.stringify(direct.metadata)===JSON.stringify(actual.metadata)
            && direct.png.length===actual.png.length && direct.png.every((b,i)=>b===actual.png[i]);
        } else passed=JSON.stringify(direct)===JSON.stringify(actual);
      }
      return {operation,params,passed,rejected:expectedError!==undefined,error,expected_error:expectedError,
        worker_idle:window.client.activeWorkers===0};
    };
  });
  for(const entry of process.env.JADE_PDF_WORKER_FAULTS_ONLY ? [] : entries) {
    const row=await page.evaluate(async entry=>{
      const bytes=new Uint8Array(await(await fetch('/corpus/cache/'+entry.id+'.pdf')).arrayBuffer());
      const rewritten=new Uint8Array(bytes.length+12);rewritten.set(bytes);rewritten.set(new TextEncoder().encode('\n% rewrite\n'),bytes.length);
      const checks=[];
      const renderPage=entry.invalid_page || 1;
      checks.push(await checkPair('inspect',[bytes]));
      checks.push(await checkPair('compare',[bytes,rewritten],{old_name:'old',new_name:'new'}));
      checks.push(await checkPair('renderWidth',[bytes],{page:renderPage,width:400}));
      for(const dpi of [72,144]) checks.push(await checkPair('renderDpi',[bytes],{page:renderPage,dpi}));
      if(entry.invalid_page) checks.push(await checkPair('textLayer',[bytes],{page:entry.invalid_page}));
      else for(let p=1;p<=(entry.pages||1);p++) checks.push(await checkPair('textLayer',[bytes],{page:p}));
      return {id:entry.id,passed:checks.every(x=>x.passed&&x.worker_idle),checks};
    },entry);
    documents.push(row);console.log((row.passed?'PASS ':'FAIL ')+entry.id+' '+row.checks.length+' operations');
  }
  const fixtures=process.env.JADE_PDF_WORKER_FAULTS_ONLY ? [] : await page.evaluate(async()=>{
    const checks=[];
    for(const name of JSON.parse(probe.appearance_cases())) {
      const params={old_name:'old',new_name:'new'};
      if(name==='budget') {params.options=JSON.parse(probe.worker_defaults()).options;params.options.visual.max_total_pixels=720000;}
      const check=await checkPair('compare',[probe.worker_fixture(name,false),probe.worker_fixture(name,true)],params);
      checks.push({id:'fixture-'+name,...check});
    }
    return checks;
  });results.push(...fixtures);
  const faults=await page.evaluate(async()=>{
    const checks=[],bytes=new Uint8Array(await(await fetch('/corpus/cache/hello.pdf')).arrayBuffer());
    const expect=async(id,fn,code)=>{
      try {await fn();checks.push({id,passed:false,error:'Expected '+code});}
      catch(error) {checks.push({id,passed:error.code===code,code:error.code,message:error.message});}
    };
    const before=workerStats.created;
    const abort=new AbortController();abort.abort();
    await expect('pre-aborted',()=>client.run('inspect',[bytes],{},{signal:abort.signal}),'ABORTED');
    await expect('invalid-metadata',()=>client.run('inspect',[bytes],{options:{a:Infinity}}),'INVALID_INPUT');
    await expect('metadata-budget',()=>client.run('inspect',[bytes],{x:['x'.repeat(600000),'x'.repeat(600000)]}),'LIMIT');
    const cycle={};cycle.self=cycle;
    await expect('cyclic-metadata',()=>client.run('inspect',[bytes],cycle),'INVALID_INPUT');
    const accessor={};let invoked=false;
    Object.defineProperty(accessor,'toJSON',{get(){invoked=true;return()=>({});}});
    await expect('metadata-accessor',()=>client.run('inspect',[bytes],accessor),'INVALID_INPUT');
    checks.push({id:'metadata-accessor-not-invoked',passed:!invoked});
    const tiny=new PdfWorkerClient('/tools/wasm-smoke/worker_adapter.mjs',{maxInputBytes:1});
    await expect('input-budget',()=>tiny.run('inspect',[bytes]),'LIMIT');tiny.dispose();
    await expect('unknown-operation',()=>client.run('shell',[bytes]),'INVALID_INPUT');
    checks.push({id:'preflight-spawns-no-worker',passed:before===workerStats.created});
    const copy=bytes.slice(), length=copy.byteLength;
    await client.run('inspect',[copy]);
    checks.push({id:'default-preserves-input',passed:copy.byteLength===length && copy.every((b,i)=>b===bytes[i])});
    const owned=bytes.slice();await client.run('inspect',[owned],{},{transfer:true});
    checks.push({id:'ownership-transfer',passed:owned.byteLength===0});
    await expect('detached-buffer-rejected',()=>client.run('inspect',[owned.buffer]),'INVALID_INPUT');
    const both=bytes.slice();await client.run('compare',[both,both],{},{transfer:true});
    checks.push({id:'shared-input-transfer',passed:both.byteLength===0});
    await expect('partial-transfer-rejected',()=>client.run('inspect',[bytes.subarray(1)],{},{transfer:true}),'INVALID_INPUT');
    const limited=new PdfWorkerClient('/tools/wasm-smoke/worker_adapter.mjs',{maxOutputBytes:128});
    await expect('sdk-output-budget',()=>limited.run('inspect',[bytes]),'LIMIT');limited.dispose();
    for(const [mode,code,op] of [['throw-init','WORKER_ERROR'],['trap','WORKER_ERROR'],['large-json','LIMIT'],['raw-version','PROTOCOL'],['raw-large','LIMIT'],['raw-utf8','PROTOCOL'],['raw-depth','LIMIT'],['raw-png','PROTOCOL','renderWidth']]) {
      const worker=new PdfWorkerClient('/tools/wasm-smoke/worker_fault.mjs?mode='+mode,{maxOutputBytes:1024});
      await expect(mode,()=>worker.run(op||'inspect',[bytes]),code);
      checks.push({id:mode+'-released',passed:worker.activeWorkers===0});worker.dispose();
    }
    const options=JSON.parse(probe.worker_defaults()).options;options.position_tolerance=-1;
    await expect('invalid-core-option',()=>client.run('compare',[bytes,bytes],{options}),'PDF_ERROR');
    await expect('invalid-core-limit',()=>client.run('inspect',[bytes],{limits:{max_input_mb:1,max_pages:0,max_blocks_per_page:1}}),'PDF_ERROR');
    await client.run('inspect',[bytes]);checks.push({id:'recovery-after-errors',passed:client.activeWorkers===0});
    const missing=new PdfWorkerClient('/tools/wasm-smoke/no-such-worker.mjs');
    await expect('worker-entry-failure',()=>missing.run('inspect',[bytes]),'WORKER_ERROR');
    checks.push({id:'worker-entry-failure-released',passed:missing.activeWorkers===0});missing.dispose();
    return checks;
  });results.push(...faults);
  const cdp=await browser.newBrowserCDPSession();
  const destroyed=new Map();
  cdp.on('Target.targetDestroyed',event=>destroyed.set(event.targetId,Date.now()));
  await cdp.send('Target.setDiscoverTargets',{discover:true});
  for(const mode of ['timeout','abort','wait-init','dispose']) {
    const job='jade-worker-'+mode+'-'+Date.now();
    await page.evaluate(()=>{window.faultStartedJob=null;});
    const pending=page.evaluate(async({mode,job})=>{
      const bytes=new Uint8Array(await(await fetch('/corpus/cache/hello.pdf')).arrayBuffer());
      const controller=new AbortController();
      const url='/tools/wasm-smoke/worker_fault.mjs?mode='+(mode==='wait-init'?'wait-init':'spin')+'&job='+job;
      const worker=new PdfWorkerClient(url,{timeoutMs:2500});
      window.faultStarted=false;let ticks=0;
      const heartbeat=setInterval(()=>ticks++,10),channel=new BroadcastChannel(job);
      channel.onmessage=()=>{window.faultStarted=true;window.faultStartedJob=job;};
      window.finishFault=()=>mode==='dispose'?worker.dispose():controller.abort();
      const promise=worker.run('inspect',[bytes],{block:true},{signal:controller.signal});
      let busy=false;
      try {await worker.run('inspect',[bytes]);} catch(e) {busy=e.code==='BUSY';}
      let code;try {await promise;}catch(e){code=e.code;}
      const rejectedAtEpochMs=Date.now();
      const expected=mode==='abort'?'ABORTED':mode==='dispose'?'DISPOSED':'TIMEOUT';
      const quarantine=worker.activeWorkers===1;
      let blockedDuringRetirement=false;
      try{await worker.run('inspect',[bytes]);}catch(e){blockedDuringRetirement=e.code===(mode==='dispose'?'DISPOSED':'BUSY');}
      const rejectedAt=performance.now();
      await worker.whenIdle();
      const retirementWaitMs=performance.now()-rejectedAt;
      const idle=worker.activeWorkers===0;
      let recovered=true;
      if(mode!=='wait-init'&&mode!=='dispose') {try {recovered=(await worker.run('inspect',[bytes])).pages.length===1;}catch{recovered=false;}}
      worker.dispose();clearInterval(heartbeat);channel.close();
      return {id:'lifecycle-'+mode,passed:code===expected&&idle&&busy&&recovered&&ticks>2&&quarantine&&blockedDuringRetirement,
        code,idle,busy,recovered,ticks,quarantine,blockedDuringRetirement,retirementWaitMs,rejectedAtEpochMs,started:window.faultStarted};
    },{mode,job});
    await page.waitForFunction(job=>window.faultStartedJob===job,job,{timeout:2000});
    const live=(await cdp.send('Target.getTargets')).targetInfos.filter(t=>t.type==='worker'&&t.url.includes(job));
    if(mode==='abort'||mode==='dispose') {await page.waitForTimeout(60);await page.evaluate(()=>window.finishFault());}
    const result=await pending;
    let remaining;
    for(let attempt=0;attempt<250;attempt++) {
      remaining=(await cdp.send('Target.getTargets')).targetInfos.filter(t=>t.type==='worker'&&t.url.includes(job));
      if(!remaining.length)break;
      await page.waitForTimeout(20);
    }
    result.observed_live_targets=live.length;result.remaining_targets=remaining.length;
    result.live_targets=live;result.remaining_target_details=remaining;
    result.target_destroy_event=live.length===1 && destroyed.has(live[0].targetId);
    result.observed_termination_delay_ms=result.target_destroy_event ? destroyed.get(live[0].targetId)-result.rejectedAtEpochMs : null;
    result.passed&&=live.length===1 && remaining.length===0 && result.target_destroy_event;
    results.push(result);console.log((result.passed?'PASS ':'FAIL ')+JSON.stringify(result));
  }
  const cleanup=await page.evaluate(()=>{
    client.dispose();return {id:'all-worker-termination-requests',passed:workerStats.created===workerStats.terminated&&client.activeWorkers===0,...workerStats};
  });results.push(cleanup);
  let remainingTargets=[];
  for(let attempt=0;attempt<250;attempt++) {
    remainingTargets=(await cdp.send('Target.getTargets')).targetInfos.filter(t=>t.type==='worker');
    if(!remainingTargets.length)break;
    await page.waitForTimeout(20);
  }
  results.push({id:'no-remaining-worker-targets',passed:remainingTargets.length===0,remainingTargets});
  const sourceFiles=['crates/jade-pdf-diff/wasm/worker_client.mjs','crates/jade-pdf-diff/wasm/worker_server.mjs','crates/jade-pdf-diff/wasm/worker_protocol.mjs','tools/wasm-smoke/worker_adapter.mjs','tools/wasm-smoke/worker_operations.mjs','tools/wasm-smoke/worker_fault.mjs','tools/wasm-smoke/worker_smoke.mjs'];
  const sourceHashes=Object.fromEntries(await Promise.all(sourceFiles.map(async f=>[f,hash(await fs.readFile(path.join(root,f)))])));
  const operations=documents.flatMap(d=>d.checks);
  const report={version:1,manifest_sha256:hash(manifestBytes),browser:browser.version(),wasm_sha256:hash(await fs.readFile(path.join(root,'corpus/wasm-runtime/probe/jade_pdf_wasm_smoke_bg.wasm'))),source_sha256:sourceHashes,
    methodology:'Disposable real module Workers under self-only script/worker CSP; exact direct-WASM differential results, including all selected text pages and binary PNG/viewport outputs. Parent enforces input/output transport budgets, admission slots, deadline/abort/disposal and requests Worker termination on every outcome. Interrupted slots remain reserved for three seconds: Chrome can defer forcibly stopping running code by two seconds. Non-returning WASM and startup stalls are terminated; Chrome DevTools independently observes target destruction. Not an immediate CPU/memory ceiling, filesystem/network sandbox, aggregate host bound, or cross-browser qualification.',
    documents:documents.length,passed_documents:documents.filter(d=>d.passed).length,operation_checks:operations.length,matching_operations:operations.filter(x=>x.passed).length,matching_successful_operations:operations.filter(x=>x.passed&&!x.rejected).length,matching_rejections:operations.filter(x=>x.passed&&x.rejected).length,
    cases:results.length,passed:results.filter(x=>x.passed).length,results,document_results:documents};
  await fs.writeFile(reportPath,JSON.stringify(report,null,2)+'\n');
  console.log(`${report.passed_documents}/${report.documents} documents, ${report.matching_operations}/${report.operation_checks} operations, ${report.passed}/${report.cases} additional cases`);
  if(report.passed!==report.cases||report.passed_documents!==report.documents)process.exitCode=1;
} finally {if(browser)await browser.close();await new Promise(resolve=>server.close(resolve));}
