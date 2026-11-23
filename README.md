# jade-pdf

A Rust workspace for comparing PDFs and converting PDF documents to DOCX.

The project is under active development. Conversion coverage and layout fidelity vary by document; see the guides below for supported features and limitations.

Native conversion now recovers unreadable TrueType display names from verified embedded font metadata and retains subsets with nonprinting control glyphs. The reviewed Japanese form renders its text again, but pagination remains incorrect; see the [font and rendering evidence](corpus/validation-native-fonts.json).

Single-entry Device color-space arrays also retain decoded image colors. The [pixel validation](corpus/validation-singleton-colorspace.json) checks the original RGB samples independently; table layout and image placement remain separate limitations.

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

Painted curved paths in page content produce a per-page `unsupported_curved_path` diagnostic. The line-based layout model does not reproduce these shapes, which can include outlined lettering. Strict conversion rejects this detected loss; the diagnostic does not recognize the missing text or trigger OCR. Curves are also excluded from table-rule inference. Absence of this diagnostic does not establish support for arbitrary vector artwork; see the [curved-path evidence](corpus/validation-curved-path-loss.json).

The pure Rust parser accepts direct `LZWDecode` content filters with `EarlyChange` 0 or 1 and no prediction (`Predictor` 1). Formal page `Contents` may reference an indirect array; stream order, repeated references, and state across streams are preserved. Wrong-generation or invalid content references are errors. Malformed LZW and unsupported LZW parameters fail conversion instead of producing a blank document. Filter chains, indirect filter/parameter values, and LZW predictors remain unsupported; other content codecs retain their existing limitations. Scanned images still require OCR for editable text. See the [LZW and Contents validation](corpus/validation-lzw-content.json) for the reviewed source, conformance checks and remaining limits.

### Compare PDFs

```sh
cargo run -p jade-pdf-diff -- old.pdf new.pdf
cargo run -p jade-pdf-diff -- --ir input.pdf
```

The comparison command writes JSON to stdout and exits with `0` when there are no changes, `1` when changes are found, `2` on errors, or `3` when source or comparison coverage requires review. See the [PDF comparison guide](docs/pdf_diff.md) for rendering and text-layer commands.

PDF comparison uses the pinned Pdfium runtime described in the [comparison guide](docs/pdf_diff.md). For PDF→DOCX rendering with a prepared, verified runtime without downloads, follow the [offline Pdfium guide](docs/pdfium-offline.md).

Native file parsing also exposes `ParsedTextElement::baseline_y` for a finite origin reported by the SDK for the first horizontal glyph. DOCX row grouping uses this separately from `bounds`, which remains unchanged. Rust callers constructing text elements can set `baseline_y: None` to retain bounds-based grouping; pure bytes/WASM parsing leaves it unset.

Inferred centred text lines within the body area retain their PDF centre through section-relative paragraph indents, keeping the previous writable width. Substituted font metrics can still change the visible text width.

Text and table offsets share the page margin origin so overhanging headings and graphics do not shift body content.

Tables inferred from PDF rules keep their source-coordinate row order and each grid row’s height, including descending values and content streams that paint the bottom row first. Cells whose modeled text agrees with the source retain individual run styles. Multiline cells use separate source-positioned paragraphs and explicit edge spaces when their paragraph boxes fit the source row height; single-line cells retain their existing text and alignment. Nested, unmatched, or overfull cells use the existing layout. Complex cell boundaries, font substitution, and rendered placement still require review; see the [cell-line validation](corpus/validation-native-cell-lines.json).

Text paragraphs use an explicit line box based on the source font size, with the PDF's blank space applied separately. This prevents font-dependent automatic leading from accumulating between source lines. Font substitution can still shift the first baseline or change glyph widths.

PDF comparison qualification uses 90 registered public PDFs (76 regular documents with 717 pages, 10 robustness inputs, one specification reference, and three unresolved semantic gold cases). Three additional reviewed W3C files represent two independent visible contents and remain outside the registered corpus. See the [corpus record](corpus/README.md) for validated profiles and remaining accuracy limits.

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

PDF comparison keeps nonpainting source text while reporting its SDK record count and unavailable render modes separately from Unicode mapping errors. Text comparisons with these diagnostics require review; byte-identical source proof remains valid for appearance identity. These checks do not verify OCR accuracy or arbitrary color, opacity, and clipping. See the [comparison guide](docs/pdf_diff.md).

PDF comparison returns an ordered page-text view in `PageDiff.text_diff`, alongside the original line text, segments, and geometry. This separates line wrapping from text edits while retaining form and visual changes. Check the comparison-completeness diagnostics; old JSON without this field represents an uncomputed text view. See the [comparison guide](docs/pdf_diff.md) for source ranges, normalization, and alignment limits.

## License

Project code is licensed under [PolyForm Noncommercial 1.0.0](LICENSE). Commercial use requires a separate license from the author; see the full terms in `LICENSE`.

The public PDF fixtures and their extracted expected text are licensed separately under [CC BY-SA 4.0](fixtures/LICENSE-CC-BY-SA-4.0.txt), with attribution recorded in the [fixture guide](fixtures/README.md). Third-party dependencies retain their respective licenses.

Verified embedded Microsoft Symbol fonts can recover unambiguous legacy private-use mathematical characters. A separate MT Extra profile recovers only the verified ellipsis and script small l; assembly fragments remain unresolved. Font names, glyph IDs, source mappings and character cells must agree; unresolved cases retain review diagnostics. See the [extraction guide](docs/pdf_diff.md). `pdf-diff --licenses` includes the Adobe Symbol mapping and pinned ttf-parser notices, also exported as `ADOBE_SYMBOL_ENCODING_LICENSE` and `TTF_PARSER_LICENSE`. Unicode direction and Japanese tracking data notices are included and exported as `UNICODE_DATA_LICENSE`.

Japanese character tracking is distinguished from inferred word gaps using verified full-em advances and assigned Han/Kana character classes. Source-coded and ActualText spaces remain intact; public glyph cells and review diagnostics are retained. See the [extraction guide](docs/pdf_diff.md) for the bounded spacing rule.

The conversion parser includes the Adobe Glyph List and Zapf glyph-name mappings under BSD-3-Clause. Their [copyright and license notice](crates/jade-pdf2docx-parser/licenses/adobe-glyph-list.txt) must accompany redistributed converter binaries as well as source distributions. The [mapping provenance](crates/jade-pdf2docx-parser/resources/adobe-glyph-list.json) records the pinned data and reproducible generator; no font programs are bundled.

For embedded Type1 FontFile fonts without an explicit PDF BaseEncoding, the bytes parser reads supported static built-in Encoding assignments and applies PDF Differences before Unicode extraction. ToUnicode takes precedence for each mapped code. Painted codes with unverified glyph names produce `UnsupportedFontReason::Encoding` (`unverified_encoding`); strict mode rejects them. Unused unknown slots do not reject known text. Dynamic PostScript and the ISOLatin1 base vector remain unverified. This does not provide OCR recovery for opaque glyph names. The StandardEncoding table carries the [fontTools MIT notice](crates/jade-pdf2docx-parser/licenses/fonttools.txt), which must accompany redistributed binaries.

If a nonstandard Type1 font has no usable built-in font program and no per-code Unicode mapping, its painted codes are unverified; strict conversion rejects them. Optional null or unresolved Encoding values use the built-in default, as required by PDF null-object semantics. The existing non-embedded Standard 14 font path is preserved.

When the built-in program is unavailable, the existing ToUnicode map stays shared and unmapped codes remain explicitly unverified in the quality report; normal best-effort output may still contain fallback text. Strict mode rejects any painted unverified code.

The production DOCX WASM build helper bundles and verifies the Adobe Glyph List and fontTools notices. Distribute the complete generated directory; see the [WASM build guide](tools/wasm-docx-smoke/README.md). Native converter distributions must also include both parser notices.
