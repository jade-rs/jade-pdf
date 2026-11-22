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
output's OOXML is checked independently with Python's standard library.

This suite verifies byte-PDF conversion and inspection. It does not establish
rendered-PNG API behavior, font or artwork fidelity, Microsoft Word rendering,
or browser host memory and cancellation limits. The strict checks enforce
rejection of detected quality loss; absence of a diagnostic does not prove full
visual fidelity. PDF inputs and generated DOCX/WASM files are not stored in Git.
