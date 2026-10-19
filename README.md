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

Strict mode rejects detected quality loss instead of publishing the output. OCR is opt-in and requires the `ocr-onnx` feature, external model files, and `pdftoppm`; see the [OCR setup guide](docs/ocr_runtime.md).

### Compare PDFs

```sh
cargo run -p jade-pdf-diff -- old.pdf new.pdf
cargo run -p jade-pdf-diff -- --ir input.pdf
```

The comparison command writes JSON to stdout and exits with `0` when there are no changes or `1` when changes are found. See the [PDF comparison guide](docs/pdf_diff.md) for rendering and text-layer commands.

Native extraction and rendering use Pdfium where required. For a prepared, verified runtime without downloads, follow the [offline Pdfium guide](docs/pdfium-offline.md).

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

## License

Project code is licensed under [PolyForm Noncommercial 1.0.0](LICENSE). Commercial use requires a separate license from the author; see the full terms in `LICENSE`.

The public PDF fixtures and their extracted expected text are licensed separately under [CC BY-SA 4.0](fixtures/LICENSE-CC-BY-SA-4.0.txt), with attribution recorded in the [fixture guide](fixtures/README.md). Third-party dependencies retain their respective licenses.
