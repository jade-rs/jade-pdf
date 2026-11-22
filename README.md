# jade-pdf

A Rust workspace for comparing PDFs and converting PDF documents to DOCX.

The project is under active development. Conversion coverage and layout fidelity vary by document; see the guides below for supported features and limitations.

## Features

- **PDF comparison:** align pages and text blocks, inspect extracted document structure, and render pages with Pdfium.
- **PDF to DOCX:** convert text, tables, and images, with quality diagnostics and an optional strict mode.
- **Optional OCR:** recognize scanned content with `ndlocr-lite-rs` and ONNX Runtime. Models and character sets are supplied separately.
- **Browser API:** use the `jade-pdf2docx-wasm` crate for conversion through WebAssembly. Native and browser extraction paths have different coverage.

The conversion command is `jade-pdf2docx`. Workspace components are listed in the [development guide](docs/development.md).

## Quick start

Run these commands from the repository root with Rust stable installed.

### Convert PDF to DOCX

```sh
cargo run -p jade-pdf2docx -- input.pdf output.docx
cargo run -p jade-pdf2docx -- input.pdf output.docx --strict --timeout-ms 60000
cargo run -p jade-pdf2docx -- --help
```

Pages larger than Word's 22-inch limit are scaled proportionally on output, including text, tables, images, OCR frames, headers, and footers. File conversion reports `page_resized_for_word`; `--strict` rejects this physical size change before OCR or publication. Rust callers can inspect `docx_quality_report`; browser inspection exposes the same diagnostic. The bytes API returns the fitted document, and rendered PNG page conversion fits the page and image together without resampling its payload.

Native file conversion also accepts standard encrypted PDFs that open without a password and permit text copying. It keeps the original input unchanged and bounds the internal decrypted representation with the input/allocation limits. Password-protected or copy-restricted inputs remain errors; the pure Rust bytes/WASM parser still rejects encryption.

Whole-page OCR preserves supplied line rectangles as page-anchored text frames and keeps the recognizer’s reading order. OCR does not recover the original fonts or page artwork.

Strict mode rejects detected quality loss instead of publishing the output. OCR is opt-in and requires the `ocr-onnx` feature, external model files, and `pdftoppm`; see the [OCR setup guide](docs/ocr_runtime.md).

### Compare PDFs

```sh
cargo run -p jade-pdf-diff -- old.pdf new.pdf
cargo run -p jade-pdf-diff -- --ir input.pdf
```

The comparison command writes JSON to stdout and exits with `0` when there are no changes, `1` when changes are found, `2` on errors, or `3` when source or comparison coverage requires review. See the [PDF comparison guide](docs/pdf_diff.md) for rendering and text-layer commands.

PDF comparison uses the pinned Pdfium runtime described in the [comparison guide](docs/pdf_diff.md). For PDF→DOCX rendering with a prepared, verified runtime without downloads, follow the [offline Pdfium guide](docs/pdfium-offline.md).

Native file parsing also exposes `ParsedTextElement::baseline_y` for a finite origin reported by the SDK for the first horizontal glyph. DOCX row grouping uses this separately from `bounds`, which remains unchanged. Rust callers constructing text elements can set `baseline_y: None` to retain bounds-based grouping; pure bytes/WASM parsing leaves it unset.

Inferred centred text lines within the body area retain their PDF centre through section-relative paragraph indents, keeping the previous writable width. Substituted font metrics can still change the visible text width.

PDF comparison qualification uses 81 collected public PDFs (69 regular documents, 10 robustness inputs, one specification reference, and one unresolved semantic gold case). See the [corpus record](corpus/README.md) for validated profiles and remaining accuracy limits.

## Development

```sh
cargo fmt --all -- --check
RUST_TEST_THREADS=1 sh scripts/check_rust_features.sh default
python3 -m unittest discover -s tests -p 'test_*.py'
```

The feature-check script runs Clippy and tests for the conversion workspace. PDF comparison tests run separately on macOS. Public-PDF E2E tests exercise the conversion CLI against independently extracted expected text; other tests generate their inputs. See the [development guide](docs/development.md) for feature checks and WASM builds.

## Documentation

Detailed guides are currently written in Japanese.

- [OCR setup](docs/ocr_runtime.md): external models and scanned-document conversion
- [Offline Pdfium](docs/pdfium-offline.md): preparing a verified native runtime
- [Linux deployment](docs/systemd_memory.md): memory limits and service management
- [PDF comparison](docs/pdf_diff.md): comparison, text extraction, and rendering
- [Development](docs/development.md): workspace structure, builds, and tests
- [Test fixtures](fixtures/README.md): public PDF sources, attribution, and update policy

PDF comparison returns an ordered page-text view in `PageDiff.text_diff`, alongside the original line text, segments, and geometry. This separates line wrapping from text edits while retaining form and visual changes. Check the comparison-completeness diagnostics; old JSON without this field represents an uncomputed text view. See the [comparison guide](docs/pdf_diff.md) for source ranges, normalization, and alignment limits.

## License

Project code is licensed under [PolyForm Noncommercial 1.0.0](LICENSE). Commercial use requires a separate license from the author; see the full terms in `LICENSE`.

The public PDF fixtures and their extracted expected text are licensed separately under [CC BY-SA 4.0](fixtures/LICENSE-CC-BY-SA-4.0.txt), with attribution recorded in the [fixture guide](fixtures/README.md). Third-party dependencies retain their respective licenses.

Verified embedded Microsoft Symbol fonts can recover unambiguous legacy private-use mathematical characters. A separate MT Extra profile recovers only the verified ellipsis and script small l; assembly fragments remain unresolved. Font names, glyph IDs, source mappings and character cells must agree; unresolved cases retain review diagnostics. See the [extraction guide](docs/pdf_diff.md). `pdf-diff --licenses` includes the Adobe Symbol mapping and pinned ttf-parser notices, also exported as `ADOBE_SYMBOL_ENCODING_LICENSE` and `TTF_PARSER_LICENSE`. Unicode direction and Japanese tracking data notices are included and exported as `UNICODE_DATA_LICENSE`.

Japanese character tracking is distinguished from inferred word gaps using verified full-em advances and assigned Han/Kana character classes. Source-coded and ActualText spaces remain intact; public glyph cells and review diagnostics are retained. See the [extraction guide](docs/pdf_diff.md) for the bounded spacing rule.
