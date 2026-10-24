import assert from 'node:assert/strict';
import {preparePdfiumForJade} from '../../crates/jade-pdf-diff/wasm/pdfium_adapter.mjs';
const key=128,buffer=512,out=6000;
let required=0,grow=false,calls=[];
const module={HEAPU8:new Uint8Array(8192),ccall(name,ret,types,args){
  calls.push({name,args});
  if(name==='FPDFPageObjMark_GetParamBlobValue') {
    assert.equal(args[1],key+9);
    if(grow) {const next=new Uint8Array(16384);next.set(this.HEAPU8);this.HEAPU8=next;}
    new DataView(this.HEAPU8.buffer).setUint32(out,required,true);
  }
  return 1;
}};
module.HEAPU8.set(new TextEncoder().encode('JadeBlob:ActualText\0'),key);
const prepared=preparePdfiumForJade(module);assert.equal(prepared,module);
assert.equal(preparePdfiumForJade(module),module);
const blob=()=>module.ccall('FPDFPageObjMark_GetParamKey','number',Array(5).fill('number'),[10,key,buffer,4096,out]);
assert.equal(blob(),1);required=4;assert.equal(blob(),1);
required=4096;assert.equal(blob(),1);
required=4097;assert.equal(blob(),0);
required=4;grow=true;assert.equal(blob(),1);
module.ccall('FPDFPageObjMark_GetParamKey','number',Array(5).fill('number'),[10,0,buffer,4096,out]);
assert.equal(calls.at(-1).name,'FPDFPageObjMark_GetParamKey');
module.ccall('FPDFText_GetCharBox','number',Array(6).fill('number'),[1,2,3,4,5,6]);
assert.deepEqual(calls.at(-1).args,[1,2,3,5,6,4]);
assert.throws(()=>module.ccall('FPDFPageObjMark_GetParamKey','number',Array(5).fill('number'),[10,key,buffer,4097,out]),/Invalid bounded/);
assert.throws(()=>module.ccall('FPDFText_GetCharBox','number',[],[]),/Invalid FPDF/);
module.ccall=()=>1;assert.throws(()=>preparePdfiumForJade(module),/was replaced/);
const legacy={ccall(){}};Object.defineProperty(legacy,Symbol.for('jade-pdf.pdfium-render.0.8.37.ccall'),{value:legacy.ccall});
assert.throws(()=>preparePdfiumForJade(legacy),/Legacy Pdfium adapter/);
console.log('Adapter: bounded empty/full/oversize/growing-memory blob reads, ordinary key enumeration, geometry, validation and rebinding passed');
