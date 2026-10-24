import {servePdfWorker} from '/crates/jade-pdf-diff/wasm/worker_server.mjs';
import {loadPdfOperations} from './worker_operations.mjs';
const query=new URL(import.meta.url).searchParams, mode=query.get('mode');
const notify=()=>{const c=new BroadcastChannel(query.get('job')); c.postMessage('running'); c.close();};
if(mode?.startsWith('raw-')) {
  addEventListener('message',event=>{
    const request=event.data;
    let data=new TextEncoder().encode('{}'), version=1, format='json',metadata;
    if(mode==='raw-version') version=999;
    if(mode==='raw-large') data=new Uint8Array(request.maxOutputBytes+1);
    if(mode==='raw-utf8') data=new Uint8Array([255]);
    if(mode==='raw-depth') data=new TextEncoder().encode('['.repeat(257)+'0'+']'.repeat(257));
    if(mode==='raw-png') {format='png';data=new Uint8Array(24);metadata=new TextEncoder().encode('{}').buffer;}
    postMessage({version,kind:'success',format,data:data.buffer,metadata},metadata?[data.buffer,metadata]:[data.buffer]);
  });
} else {
  servePdfWorker(async()=>{
    if(mode==='wait-init') {notify(); await new Promise(()=>{});}
    if(mode==='throw-init') throw new Error('Injected SDK initialization failure');
    return {inspect:async(inputs,params)=>{
      if(mode==='spin' && params.block) {
        // A synchronous, non-returning WASM function, not an async JS delay.
        const bytes=new Uint8Array([0,97,115,109,1,0,0,0,1,4,1,96,0,0,3,2,1,0,7,8,1,4,115,112,105,110,0,0,10,9,1,7,0,3,64,12,0,11,11]);
        const instance=new WebAssembly.Instance(new WebAssembly.Module(bytes));
        notify(); instance.exports.spin();
      }
      if(mode==='trap') throw new WebAssembly.RuntimeError('Injected WASM trap');
      if(mode==='large-json') return JSON.stringify('x'.repeat(4096));
      const operations=await loadPdfOperations();
      return operations.inspect(inputs,{});
    }};
  });
}
