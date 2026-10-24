import {VERSION,DEFAULTS,PARAM_BYTES,PdfWorkerError,fail,positive,inputBuffers,jsonBytes,decodeJson,checkJsonDepth,utf8Length,errorMessage} from './worker_protocol.mjs';

/** Install synchronously in an application-owned module Worker. initialize()
 * lazily loads the SDK and returns the five adapter functions. It is covered
 * by the parent deadline; a Worker accepts exactly one PDF request. */
export function servePdfWorker(initialize,options={}) {
  if(typeof initialize!=='function') fail('INVALID_INPUT','A PDF Worker initializer is required');
  const limits={...DEFAULTS,...options};
  if(Object.keys(options).some(k=>!['maxInputBytes','maxTotalInputBytes','maxOutputBytes'].includes(k))) fail('INVALID_INPUT','Unknown PDF Worker server option');
  for(const key of ['maxInputBytes','maxTotalInputBytes','maxOutputBytes']) positive(limits[key],key);
  let used=false;
  globalThis.addEventListener('message',async event=>{
    let executing=false;
    try {
      if(used) fail('PROTOCOL','A PDF Worker accepts one operation');
      used=true;
      const request=event.data;
      if(!request || request.version!==VERSION || request.kind!=='run') fail('PROTOCOL','Invalid PDF Worker request');
      const inputs=inputBuffers(request.operation,request.inputs,limits);
      if(typeof request.params!=='string') fail('PROTOCOL','Invalid PDF Worker parameters');
      utf8Length(request.params,PARAM_BYTES);
      checkJsonDepth(request.params,64);
      const params=decodeJson(new TextEncoder().encode(request.params));
      if(params===null || typeof params!=='object' || Array.isArray(params)) fail('PROTOCOL','PDF Worker parameters must be an object');
      const maximum=Math.min(limits.maxOutputBytes,positive(request.maxOutputBytes,'maxOutputBytes'));
      const operations=await initialize();
      const execute=operations?.[request.operation];
      if(typeof execute!=='function') fail('PROTOCOL','PDF Worker operation is unavailable');
      executing=true;
      const value=await execute(inputs,params);
      const png=request.operation==='renderWidth' || request.operation==='renderDpi';
      if(png) {
        if(!(value?.png instanceof Uint8Array) || !(value.png.buffer instanceof ArrayBuffer)) fail('PROTOCOL','Invalid PDF adapter PNG');
        if(value.png.byteLength>maximum) fail('LIMIT','PNG exceeds the output byte budget');
        const metadata=jsonBytes(value.metadata,maximum-value.png.byteLength);
        const bytes=value.png.byteOffset===0 && value.png.byteLength===value.png.buffer.byteLength
          ? value.png : Uint8Array.prototype.slice.call(value.png);
        globalThis.postMessage({version:VERSION,kind:'success',format:'png',data:bytes.buffer,metadata:metadata.buffer},[bytes.buffer,metadata.buffer]);
      } else {
        const bytes=jsonBytes(value,maximum);
        globalThis.postMessage({version:VERSION,kind:'success',format:'json',data:bytes.buffer},[bytes.buffer]);
      }
    } catch(error) {
      const code=error instanceof PdfWorkerError ? error.code : !executing || error instanceof WebAssembly.RuntimeError ? 'WORKER_ERROR' : 'PDF_ERROR';
      globalThis.postMessage({version:VERSION,kind:'error',code,message:errorMessage(error)});
    }
  });
}
