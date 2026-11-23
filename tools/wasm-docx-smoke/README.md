# Production DOCX WASM smoke

Build and invoke the public `jade-pdf2docx-wasm` exports in a real browser:

```sh
python3 tools/pdf_corpus.py
python3 tools/wasm-smoke/prepare_runtime.py
npm ci --prefix tools/wasm-smoke
npx --prefix tools/wasm-smoke playwright install chromium
python3 tools/wasm-docx-smoke/prepare_runtime.py
python3 tools/wasm-docx-smoke/check_browser.py \
  --probe corpus/wasm-runtime/docx-probe \
  --output corpus/wasm-runtime/docx-browser-chromium --engine chromium
```

Use `--engine firefox` or `--engine webkit` after installing the corresponding
pinned Playwright browser. The build helper reuses the verified wasm-bindgen
archive and executable from the existing runtime preparation. It copies the
production WASM artifact when Cargo reports it, before temporary build artifacts
can be collected.

The checks compare the generated DOCX paragraph and table coordinates with
authored PDF coordinates. Owned Helvetica inputs are created in a temporary
directory. Both default and strict conversion exports, inspection, Word's maximum
page size and rejection of the pinned encrypted NTA input are exercised. Each
output's OOXML is checked independently with Python's standard library. Four
authored text lines also check that the explicit line box and paragraph gap
reconstruct their 20-point source pitch. The suite requires every fixture and
both default and strict outputs to be present.

Two additional Simple/Type0 inputs use owned metadata-only sfnt structures and
non-UTF-8 PDF names. Their DOCX font family must come from the matching program's
PostScript name. These four default/strict checks cover display-name recovery;
the structures do not establish drawing-font validity or rendered font fidelity.

An owned two-pixel image with a singleton DeviceRGB color-space array checks the
actual decoded RGB samples in the default DOCX. Its PNG is independently checked
with Python's standard library, including chunk CRCs and bounded decompression.
Strict conversion still rejects the unverified image diagnostic. Pixel recovery
does not establish full image placement or table-layout fidelity.

Two owned three-row tables independently check the original top-to-bottom cell
order in both default and strict exports. Descending values and reverse paint
order must preserve the authored row and column associations without introducing
quality diagnostics. These checks do not establish complex table partitioning or
within-cell rendering fidelity.

This suite verifies byte-PDF conversion and inspection. It does not establish
rendered-PNG API behavior, font or artwork fidelity, Microsoft Word rendering,
or browser host memory and cancellation limits. The strict checks enforce
rejection of detected quality loss; absence of a diagnostic does not prove full
visual fidelity. PDF inputs and generated DOCX/WASM files are not stored in Git.
