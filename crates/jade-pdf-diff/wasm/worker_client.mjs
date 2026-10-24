import {VERSION,DEFAULTS,ERROR_BYTES,PdfWorkerError,fail,positive,parameters,inputBuffers,decodeJson,utf8Length} from './worker_protocol.mjs';
export {PdfWorkerError} from './worker_protocol.mjs';

/** One disposable module Worker per operation. PDF buffers are copied by
 * default; transfer:true transfers ownership of whole ArrayBuffers instead. */
export class PdfWorkerClient {
  #url; #limits; #active=new Set(); #idle=new Set(); #disposed=false;
  constructor(workerUrl,options={}) {
    this.#url=new URL(workerUrl,globalThis.location?.href ?? import.meta.url);
    this.#limits={...DEFAULTS,...options};
    if(Object.keys(options).some(k=>!Object.hasOwn(DEFAULTS,k))) fail('INVALID_INPUT','Unknown PDF Worker option');
    for(const key of ['maxInputBytes','maxTotalInputBytes','maxOutputBytes']) positive(this.#limits[key],key);
    positive(this.#limits.timeoutMs,'timeoutMs',2147483647);
    positive(this.#limits.terminationGraceMs,'terminationGraceMs',2147483647);
    positive(this.#limits.maxWorkers,'maxWorkers',64);
  }
  get activeWorkers() { return this.#active.size; }
  whenIdle() {
    return this.#active.size===0 ? Promise.resolve() : new Promise(resolve=>this.#idle.add(resolve));
  }
  dispose() {
    this.#disposed=true;
    for(const cancel of [...this.#active]) cancel(new PdfWorkerError('DISPOSED','PDF Worker client disposed'));
  }
  async run(operation,inputs,params={},options={}) {
    if(this.#disposed) fail('DISPOSED','PDF Worker client disposed');
    if(this.#active.size>=this.#limits.maxWorkers) fail('BUSY','PDF Worker concurrency budget exhausted');
    const {signal,transfer=false}=options;
    if(Object.keys(options).some(k=>!['signal','transfer','timeoutMs'].includes(k))) fail('INVALID_INPUT','Unknown PDF operation option');
    if(signal!=null) {
      try { Object.getOwnPropertyDescriptor(AbortSignal.prototype,'aborted').get.call(signal); }
      catch { fail('INVALID_INPUT','Invalid abort signal'); }
    }
    if(signal?.aborted) fail('ABORTED','PDF operation aborted');
    if(typeof transfer!=='boolean') fail('INVALID_INPUT','Invalid transfer option');
    const timeout=positive(options.timeoutMs ?? this.#limits.timeoutMs,'timeoutMs',2147483647);
    const start=performance.now(), deadline=start+timeout;
    const metadata=parameters(params), views=inputBuffers(operation,inputs,this.#limits);
    if(transfer && views.some(v=>v.byteOffset!==0 || v.byteLength!==v.buffer.byteLength))
      fail('INVALID_INPUT','Ownership transfer requires whole PDF buffers');
    return new Promise((resolve,reject)=>{
      let worker=null,timer,settled=false;
      const stop=()=>{
        if(worker) {
          worker.removeEventListener('message',message);
          worker.removeEventListener('error',workerError);
          worker.removeEventListener('messageerror',messageError);
          worker.terminate(); worker=null;
        }
      };
      const finish=(error,value)=>{
        if(settled) return;
        const interrupted=worker!==null && ['TIMEOUT','ABORTED','DISPOSED'].includes(error?.code);
        settled=true; stop(); clearTimeout(timer);
        if(signal) EventTarget.prototype.removeEventListener.call(signal,'abort',abort);
        const release=()=>{
          this.#active.delete(cancel);
          if(this.#active.size===0) {for(const resolve of this.#idle)resolve();this.#idle.clear();}
        };
        // Worker.terminate() has no completion event. Chromium can defer
        // force-stopping a running script for two seconds. Hold the admission
        // slot after an interruption; this is not a portable memory/CPU bound.
        if(interrupted) setTimeout(release,this.#limits.terminationGraceMs); else release();
        if(error) reject(error); else resolve(value);
      };
      const cancel=error=>finish(error);
      const abort=()=>finish(new PdfWorkerError('ABORTED','PDF operation aborted'));
      const expired=()=>performance.now()>=deadline;
      const timedOut=()=>finish(new PdfWorkerError('TIMEOUT','PDF Worker deadline exceeded'));
      const workerError=event=>{ event.preventDefault(); finish(new PdfWorkerError('WORKER_ERROR',event.message || 'PDF Worker failed')); };
      const messageError=()=>finish(new PdfWorkerError('PROTOCOL','Unreadable PDF Worker response'));
      const message=event=>{
        if(expired()) { timedOut(); return; }
        try {
          const response=event.data;
          if(!response || response.version!==VERSION) fail('PROTOCOL','Invalid PDF Worker response version');
          if(response.kind==='error') {
            if(!['INVALID_INPUT','LIMIT','PROTOCOL','PDF_ERROR','WORKER_ERROR'].includes(response.code)
                || typeof response.message!=='string') fail('PROTOCOL','Invalid PDF Worker error');
            utf8Length(response.message,ERROR_BYTES);
            finish(new PdfWorkerError(response.code,response.message)); return;
          }
          if(response.kind!=='success' || !(response.data instanceof ArrayBuffer)) fail('PROTOCOL','Invalid PDF Worker result');
          const png=operation==='renderWidth' || operation==='renderDpi';
          if(response.format!==(png?'png':'json')) fail('PROTOCOL','Unexpected PDF Worker output format');
          const extra=png ? response.metadata : new ArrayBuffer(0);
          if(!(extra instanceof ArrayBuffer)) fail('PROTOCOL','Invalid PNG metadata');
          if(response.data.byteLength+extra.byteLength>this.#limits.maxOutputBytes) fail('LIMIT','PDF Worker output exceeds the byte budget');
          // Release the SDK heap before parent-side bounded JSON decoding.
          stop();
          const value=png ? {png:new Uint8Array(response.data),metadata:decodeJson(extra)} : decodeJson(response.data);
          if(png) {
            const m=value.metadata, bytes=value.png;
            if(bytes.length<24 || [137,80,78,71,13,10,26,10].some((v,i)=>bytes[i]!==v)
              || !m || !Number.isInteger(m.width) || !Number.isInteger(m.height)
              || m.width<1 || m.height<1 || m.width>8192 || m.height>8192 || m.width*m.height>16000000
              || m.viewport?.width!==m.width || m.viewport?.height!==m.height)
              fail('PROTOCOL','Invalid PDF Worker PNG/viewport');
            const header=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
            if(header.getUint32(16)!==m.width || header.getUint32(20)!==m.height) fail('PROTOCOL','PNG dimensions do not match the viewport');
          }
          if(expired()) { timedOut(); return; }
          finish(null,value);
        } catch(error) {
          finish(error instanceof PdfWorkerError ? error : new PdfWorkerError('PROTOCOL','Malformed PDF Worker result: '+String(error)));
        }
      };
      this.#active.add(cancel);
      if(signal) EventTarget.prototype.addEventListener.call(signal,'abort',abort,{once:true});
      timer=setTimeout(timedOut,Math.max(1,deadline-performance.now()));
      try {
        if(signal?.aborted) { abort(); return; }
        if(expired()) { timedOut(); return; }
        worker=new Worker(this.#url,{type:'module',name:'jade-pdf-'+operation});
        worker.addEventListener('message',message);
        worker.addEventListener('error',workerError);
        worker.addEventListener('messageerror',messageError);
        const buffers=views.map(v=>transfer ? v.buffer : Uint8Array.prototype.slice.call(v).buffer);
        if(expired()) { timedOut(); return; }
        worker.postMessage({version:VERSION,kind:'run',operation,inputs:buffers,params:metadata,
          maxOutputBytes:this.#limits.maxOutputBytes},[...new Set(buffers)]);
      } catch(error) {
        finish(new PdfWorkerError('WORKER_ERROR','Cannot start PDF Worker: '+String(error)));
      }
    });
  }
}
