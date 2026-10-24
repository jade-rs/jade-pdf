// Companion runtime adapter for the exactly pinned pdfium-render 0.8.37.
// Its WASM GetCharBox bridge passes output slots as left, top, right, bottom;
// Pdfium's public fpdf_text.h requires left, right, bottom, top. Correct the
// call before the wrapper constructs and normalizes a PdfRect. Its BlobValue
// method also calls GetParamKey and copies the reported length without checking
// the capacity. Jade uses a reserved key to request a bounded, raw-byte read.
const blobKey = new TextEncoder().encode('JadeBlob:ActualText\0');
const prepared = Symbol.for('jade-pdf.pdfium-render.0.8.37.ccall');
const contract = Symbol.for('jade-pdf.actual-text-blob.v1');

/** Call before initialize_pdfium_render() with the loaded Emscripten module. */
export function preparePdfiumForJade(module) {
  if (module === null || typeof module !== 'object' || typeof module.ccall !== 'function')
    throw new TypeError('A loaded Emscripten Pdfium module with ccall is required');
  const installed = module[prepared];
  if (installed) {
    if (module.ccall !== installed) throw new Error('Prepared Pdfium ccall was replaced');
    if (installed[contract] !== true) throw new Error('Legacy Pdfium adapter; load a fresh SDK module with the current adapter');
    return module;
  }
  const original = module.ccall;
  const adapter = function(name, returnType, argumentTypes, args, ...rest) {
    if (name === 'FPDFText_GetCharBox') {
      if (!Array.isArray(args) || args.length !== 6)
        throw new TypeError('Invalid FPDFText_GetCharBox arguments');
      args = [args[0], args[1], args[2], args[4], args[5], args[3]];
    }
    // This is an explicit private operation between Jade and its companion
    // adapter, not a guess about ordinary GetParamKey indices. Read only public
    // Emscripten memory; legitimate SDK key enumeration retains its behavior.
    if (name === 'FPDFPageObjMark_GetParamKey' && Array.isArray(args) && args.length === 5) {
      const heap = module.HEAPU8, key = args[1];
      if (heap instanceof Uint8Array && Number.isSafeInteger(key) && key >= 0
          && key + blobKey.length <= heap.length
          && blobKey.every((byte, i) => heap[key + i] === byte)) {
        const [mark, , buffer, capacity, out] = args;
        if (![mark, buffer, capacity, out].every(Number.isSafeInteger)
            || mark <= 0 || buffer <= 0 || capacity !== 4096
            || buffer + capacity > heap.length || out <= 0 || out + 4 > heap.length)
          throw new TypeError('Invalid bounded ActualText blob request');
        const result = Reflect.apply(original, module, ['FPDFPageObjMark_GetParamBlobValue',
          returnType, argumentTypes, [mark, key + 9, buffer, capacity, out], ...rest]);
        // SDK success can report a required size larger than the supplied
        // buffer. Do not let the pinned wrapper copy that many bytes.
        const current = module.HEAPU8;
        if (!(current instanceof Uint8Array) || out + 4 > current.length)
          throw new TypeError('Invalid ActualText blob result memory');
        const required = new DataView(current.buffer, current.byteOffset + out, 4).getUint32(0, true);
        return result && required <= capacity ? result : 0;
      }
    }
    return Reflect.apply(original, module, [name, returnType, argumentTypes, args, ...rest]);
  };
  Object.defineProperty(adapter, contract, {value: true});
  if (!Reflect.set(module, 'ccall', adapter) || module.ccall !== adapter)
    throw new Error('Cannot prepare Pdfium ccall');
  if (!Reflect.defineProperty(module, prepared, {value: adapter})) {
    Reflect.set(module, 'ccall', original);
    throw new Error('Cannot record prepared Pdfium ccall');
  }
  return module;
}
