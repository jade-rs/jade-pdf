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

The generated directory includes `LICENSE-adobe-glyph-list.txt`,
`LICENSE-fonttools.txt`, and `LICENSE-reportlab.txt` for the conversion parser's
glyph and standard encoding tables. ReportLab contributes static glyph-name
facts and is not a runtime dependency. Distribute these notices with the WASM and
generated bindings. The build helper verifies their complete contents and records
their hashes in `build.json`.
Recheck a copied distribution with
`python3 tools/wasm-docx-smoke/check_distribution.py --probe <directory>`.

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

Four owned curve controls retain existing editable text and inspect the exact
per-page `unsupported_curved_path` diagnostic. Painted curves must reject strict
export; cancelled paths remain accepted. A curve followed by a straight segment
must not invent a table rule, while a separately drawn rule after a cancelled
path remains present. These checks do not establish curve rendering or OCR
recognition. All fourteen earlier input hashes are retained.

Five owned LZW controls exercise the production browser exports: direct content,
explicit EarlyChange 0, an indirect Contents array with repeated streams and
cross-stream text state, malformed data, and an unsupported predictor. Valid
controls check editable text and source x coordinates in both default and strict
exports; inspection and both conversion modes must reject the invalid controls.
These controls do not establish filter-chain, indirect parameter, or predictor
support. Literal controls reset the dictionary per byte; native codec tests
separately cover 9–12-bit transitions and full-table resets for both EarlyChange
values.

Six additional Type1 metadata controls check built-in glyph assignments,
Differences overlays, unknown used glyphs, partial/complete ToUnicode priority,
and StandardEncoding quotes/ligatures. They verify exact DOCX Unicode and the
`unverified_encoding` inspection reason; strict export rejects painted unknown
codes. These controls have no CharStrings and do not establish drawing-font or
Word fidelity. Complete real Type1 drawing-font controls are separately generated
with `tools/generate_type1_drawing_controls.py`. All earlier 27 input hashes are
retained.
