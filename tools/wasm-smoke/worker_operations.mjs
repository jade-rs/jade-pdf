export async function loadPdfOperations() {
  const probe=await import('/corpus/wasm-runtime/probe/jade_pdf_wasm_smoke.js');
  const local=await probe.default();
  const {default:createPdfium}=await import('/corpus/wasm-runtime/release/node/pdfium.esm.js');
  const pdfium=await createPdfium({locateFile:n=>'/corpus/wasm-runtime/release/node/'+n});
  if(!probe.initialize_pdfium_render(pdfium,local,false)) throw new Error('PDF Worker SDK initialization failed');
  return {
    inspect:(inputs,params)=>probe.worker_json('inspect',inputs[0],new Uint8Array(),JSON.stringify(params)),
    compare:(inputs,params)=>probe.worker_json('compare',inputs[0],inputs[1],JSON.stringify(params)),
    textLayer:(inputs,params)=>probe.worker_json('textLayer',inputs[0],new Uint8Array(),JSON.stringify(params)),
    renderWidth:(inputs,params)=>probe.worker_render(inputs[0],JSON.stringify(params),false),
    renderDpi:(inputs,params)=>probe.worker_render(inputs[0],JSON.stringify(params),true),
  };
}
