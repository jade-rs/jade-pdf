// Byte transport budgets bound requests/responses, not the SDK's internal heap.
export const VERSION = 1;
export const DEFAULTS = Object.freeze({timeoutMs:120000,terminationGraceMs:3000,maxInputBytes:64*1024**2,
  maxTotalInputBytes:128*1024**2,maxOutputBytes:64*1024**2,maxWorkers:1});
export const PARAM_BYTES = 1024**2;
export const ERROR_BYTES = 8192;
export const OPERATIONS = Object.freeze({inspect:1,compare:2,textLayer:1,renderWidth:1,renderDpi:1});
export class PdfWorkerError extends Error {
  constructor(code,message) { super(message); this.name='PdfWorkerError'; this.code=code; }
}
export const fail = (code,message) => { throw new PdfWorkerError(code,message); };
export function positive(value,name,maximum=Number.MAX_SAFE_INTEGER) {
  if (!Number.isSafeInteger(value) || value<1 || value>maximum) fail('INVALID_INPUT',`Invalid ${name}`);
  return value;
}
export function utf8Length(text,maximum) {
  let bytes=0;
  for(let i=0;i<text.length;i++) {
    const c=text.charCodeAt(i);
    if(c<128) bytes++;
    else if(c<2048) bytes+=2;
    else if(c>=0xd800 && c<=0xdbff && i+1<text.length
      && text.charCodeAt(i+1)>=0xdc00 && text.charCodeAt(i+1)<=0xdfff) { bytes+=4; i++; }
    else bytes+=3;
    if(bytes>maximum) fail('LIMIT','Encoded output exceeds the byte budget');
  }
  return bytes;
}
// Check nesting before native JSON parsing. Escaped brackets inside strings
// are data, not structure; the parser still checks complete JSON syntax.
export function checkJsonDepth(text,maximum) {
  let depth=0,string=false,escaped=false;
  for(const c of text) {
    if(string) {
      if(escaped) escaped=false;
      else if(c==='\\') escaped=true;
      else if(c==='"') string=false;
    } else if(c==='"') string=true;
    else if(c==='{' || c==='[') { if(++depth>maximum) fail('LIMIT','JSON nesting exceeds the protocol limit'); }
    else if(c==='}' || c===']') depth--;
  }
}
export function parameters(value={}) {
  if(value===null || typeof value!=='object' || Array.isArray(value)) fail('INVALID_INPUT','Parameters must be an object');
  const ancestors=new Set(), stack=[{value,depth:0,exit:false}];
  let nodes=0,characters=0;
  while(stack.length) {
    const item=stack.pop(), v=item.value;
    if(item.exit) { ancestors.delete(v); continue; }
    if(++nodes>20000 || item.depth>64) fail('LIMIT','Parameters exceed the protocol work limit');
    if(v===null || typeof v==='boolean') continue;
    if(typeof v==='string') { characters+=v.length; if(characters>PARAM_BYTES) fail('LIMIT','Parameter strings exceed the protocol limit'); continue; }
    if(typeof v==='number' && Number.isFinite(v)) continue;
    if(typeof v!=='object') fail('INVALID_INPUT','Parameters must contain plain JSON values');
    const array=Array.isArray(v), prototype=Object.getPrototypeOf(v);
    if(prototype!==(array?Array.prototype:Object.prototype) && prototype!==null) fail('INVALID_INPUT','Parameters must contain plain objects');
    if(ancestors.has(v)) fail('INVALID_INPUT','Cyclic parameters');
    if(Object.getOwnPropertyDescriptor(v,'toJSON')) fail('INVALID_INPUT','Custom JSON conversion is not supported');
    ancestors.add(v); stack.push({value:v,exit:true});
    const keys=Object.keys(v);
    if(keys.length>20000 || (array && v.length>20000)) fail('LIMIT','Parameter collection is too large');
    for(const key of keys) {
      characters+=key.length;
      if(characters>PARAM_BYTES) fail('LIMIT','Parameter keys exceed the protocol limit');
      const property=Object.getOwnPropertyDescriptor(v,key);
      if(!property || !('value' in property)) fail('INVALID_INPUT','Parameter accessors are not supported');
      stack.push({value:property.value,depth:item.depth+1,exit:false});
    }
  }
  const json=JSON.stringify(value);
  utf8Length(json,PARAM_BYTES); checkJsonDepth(json,64);
  return json;
}
export function inputBuffers(operation,inputs,limits) {
  if(!Object.hasOwn(OPERATIONS,operation)) fail('INVALID_INPUT','Unknown PDF operation');
  if(!Array.isArray(inputs) || inputs.length!==OPERATIONS[operation]) fail('INVALID_INPUT','Invalid PDF input count');
  let total=0;
  const views=inputs.map(input=>{
    let view=input;
    if(input instanceof ArrayBuffer) {
      try {view=new Uint8Array(input);} catch {fail('INVALID_INPUT','A detached PDF buffer cannot be reused');}
    }
    if(!(view instanceof Uint8Array) || !(view.buffer instanceof ArrayBuffer) || view.byteLength===0)
      fail('INVALID_INPUT','Inputs must be nonempty, unshared PDF byte buffers');
    total+=view.byteLength;
    if(view.byteLength>limits.maxInputBytes || total>limits.maxTotalInputBytes) fail('LIMIT','PDF input exceeds the byte budget');
    return view;
  });
  return views;
}
export function jsonBytes(text,maximum) {
  if(typeof text!=='string') fail('PROTOCOL','The PDF adapter must return serialized JSON');
  utf8Length(text,maximum); checkJsonDepth(text,256);
  return new TextEncoder().encode(text);
}
export function decodeJson(buffer) {
  const text=new TextDecoder('utf-8',{fatal:true}).decode(buffer);
  checkJsonDepth(text,256);
  return JSON.parse(text);
}
export function errorMessage(error) {
  let message=String(error?.message ?? error).slice(0,ERROR_BYTES);
  while(message.length && new TextEncoder().encode(message).byteLength>ERROR_BYTES) message=message.slice(0,Math.floor(message.length/2));
  return message;
}
