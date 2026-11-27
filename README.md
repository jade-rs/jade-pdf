# jade-pdf

A Rust workspace for comparing PDFs and converting PDF documents to DOCX.

The project is under active development. Conversion coverage and layout fidelity vary by document; see the guides below for supported features and limitations.

Native conversion now recovers unreadable TrueType display names from verified embedded font metadata and retains subsets with nonprinting control glyphs. The reviewed Japanese form renders its text again, but pagination remains incorrect; see the [font and rendering evidence](corpus/validation-native-fonts.json).

Single-entry Device color-space arrays also retain decoded image colors. The [pixel validation](corpus/validation-singleton-colorspace.json) checks the original RGB samples independently; table layout and image placement remain separate limitations.

DeviceCMYK JPEG images retain PDF sample polarity, including Adobe CMYK/YCCK data, and apply source `Decode` and supported soft-mask `Matte` before RGB conversion. The existing unmanaged CMYK approximation can lose shadow detail; this does not establish color-managed reproduction. See the [CMYK image evidence and remaining limits](corpus/validation-device-cmyk-jpeg.json).

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

Repeated boundary text keeps its per-page header/footer style and source anchor across section margins. A first-page title with a unique style stays in the editable body, even when later running headers use the same text. See [page chrome behavior and limits](docs/page_chrome.md).

Strict mode rejects detected quality loss instead of publishing the output. OCR is opt-in and requires the `ocr-onnx` feature, external model files, and `pdftoppm`; see the [OCR setup guide](docs/ocr_runtime.md).

Source-positioned table cells reuse the existing MS-PGothic fallback width correction while preserving embedded fonts, explicit glyph placement and body/header/footer routes. The [fallback width follow-up](docs/positioned_cell_fallback_width.md) passes 2,729 Rust tests and matches all 700 declared statuses; 695 payloads remain byte exact. All 1,622 Kanpo glyphs now render on their source pages, improving its unchanged audit from38 to40 verified passes out of52. Source font metrics, diagonals, curves, exact anchors and whole-document appearance remain unaccepted.

Verified single rectangular clipping preserves opaque table backgrounds, complete ruled columns remain authoritative over wrapped-text anchors, and one/two-page bare digits stay editable without inventing footer roles. The [Kanpo guide follow-up](docs/kanpo_public_guide_holdout.md) passes 2,722 Rust tests and preserves all 700 declared runtime statuses. Its native original52 audit improves from32 to38 verified passes: all1,622 characters are editable, three tables retain three columns, and nine header cells retain gray. Rendering still clips23 glyphs; exact footer anchors, diagonals, curves and browser encryption remain incomplete. Whole-document readiness is unaccepted.

Pure bytes/WASM extraction moves verified upright baselines with the effective page-box origin and clears their upright confidence after page rotation. Word retains an edge digit bracketed by verified neighbouring body glyphs in a rotated source column. See [viewport baselines, body digits and remaining rotation limits](docs/page_viewport_text_baselines.md). The fixed follow-up passes 2,705 Rust tests and matches all 694 declared runtime statuses: 646 prior payloads stay byte exact and 48 owned outcomes retain all authored glyphs. Twelve owned outcomes recover a previously dropped digit. Whole-document rotation, font geometry and appearance remain unaccepted; the owned checks do not establish an unseen-document accuracy rate.

Normal minimum file/bytes conversion excludes confirmed text and images wholly outside a known display-page frame while preserving public parser extraction, partial intersections and graphic rules. Empty visible content cannot fall back to hidden preview text, and confirmed tall right-aligned runs retain enough horizontal fallback width. See [visible page content and remaining rotation limits](docs/default_page_visible_content.md). The fixed follow-up passes 2,691 Rust tests. All 598 prior runtime payloads remain byte exact; 48 additional owned outcomes match their declared statuses, giving 646 comparison outcomes. These owned checks do not establish an unseen-document accuracy rate. Labour remains36/57 and CPI26/107, with whole-document fidelity unaccepted.

Graphic table inference rejects an internal column line that crosses multiple actual row rules and ends freely, while preserving real junctions, collinear continuations and the existing open-header fallback. See [free-ended graph bars and table boundaries](docs/graphic_table_free_end_boundaries.md). The fixed follow-up passes 2,681 Rust tests and matches all 598 expected runtime statuses; only three known labour default payloads change and the other 595 remain byte exact. Its spurious 9×96 table is removed, improving the unchanged 57-criterion audit from 35 to 36 passes, with 21 unmet. The two source pages still become three, and vector graph appearance remains incomplete.

Visibility and OCR use the rotated display-page dimensions for normalized text/image bounds. Visible images retain the OCR review requirement under strict mode. Region crops and positioned OCR lines share the native text's display coordinates, while whole-page OCR keeps its raster positions and caller order. See [rotated-page visibility and OCR coordinates](docs/rotated_page_visibility.md). The fixed follow-up passes 2,672 Rust tests, matches all 598 expected runtime statuses, and preserves all 598 prior payloads byte for byte. Owned API regressions verify strict review, region crops and native/OCR reconciliation; existing labour35/57 and CPI26/107 appearance criteria remain unchanged.

Painted curved paths in page content produce a per-page `unsupported_curved_path` diagnostic. The line-based layout model does not reproduce these shapes, which can include outlined lettering. Strict conversion rejects this detected loss; the diagnostic does not recognize the missing text or trigger OCR. Curves are also excluded from table-rule inference. Absence of this diagnostic does not establish support for arbitrary vector artwork; see the [curved-path evidence](corpus/validation-curved-path-loss.json).

The pure Rust parser accepts direct `LZWDecode` content filters with `EarlyChange` 0 or 1 and no prediction (`Predictor` 1). It also decodes the exact `ASCII85Decode` → `FlateDecode` chain in page and Form content, with absent/null `DecodeParms` or `[null null]`, charging both decoded stages to the shared limits. Malformed ASCII85/Flate content and unsupported ASCII85 filter orders or parameters fail explicitly. Other filter chains, indirect filter/parameter values, and LZW predictors retain their existing limitations. Formal page `Contents` may reference an indirect array; stream order, repeated references, and state across streams are preserved. Wrong-generation or invalid content references are errors. Malformed LZW and unsupported LZW parameters fail conversion instead of producing a blank document. Scanned images still require OCR for editable text. See the [LZW and Contents validation](corpus/validation-lzw-content.json) for the reviewed source, conformance checks and remaining limits.

Unfiltered 8-bit images with a directly declared DeviceRGB or DeviceGray color space preserve their source samples and Decode mapping. The raw image path checks exact sample length and shared memory/decoded-byte limits; OCR review remains required. See [unfiltered image scope and limits](docs/unfiltered_device_images.md).

Images drawn inside called Form XObjects retain their local resource names, composed transforms, supported inherited paint, and convex clipping. Clipped PNGs preserve at least the source pixel density and 96dpi display resolution. Positioned images on upright pages use page-relative top-origin anchors. This recovers bitmap components without implementing complete Form artwork or transparency-group compositing: Form and OCR quality diagnostics remain, and strict conversion still rejects them. See [Form image behavior and limits](docs/form_images.md). Upright images with known bounds preserve physical extents directly from PDF points without integer display-pixel rounding; see [image dimensions and renderer limits](docs/image_physical_extents.md).

Supported normal print defaults (`BG2`/`UCR2 /Default`, boolean `SA`, and finite `SM` in 0..=1) preserve image samples instead of causing white replacements. Unknown functions, device color-space replacements and malformed states remain unconfirmed. See [image print defaults and remaining placement limits](docs/source_image_print_defaults.md). The fixed follow-up passes 2,657 Rust tests and preserves 586 of 598 prior runtime payloads byte for byte. Six defaults recover image pixels and six strict diagnostics remove only verified image-replacement warnings; all expected statuses remain unchanged. Labour34/57 and CPI26/107 criteria remain unchanged, with placement and whole-document readiness unaccepted. Recovering image pixels does not establish correct graph layout or whole-document fidelity.

When verified page rotation exactly cancels an image's cardinal CTM rotation, display dimensions and angle agree with the normalized bounds while PNG samples remain unchanged. Confirmed upright floating images use the rotated page's display height for page-relative placement. Reflection, skew, partial clipping and unknown frames retain prior handling. See [cancelled image rotation and remaining page-flow limits](docs/source_cancelled_image_rotation.md). The fixed follow-up passes 2,663 Rust tests and preserves 595 of 598 prior runtime payloads byte for byte; only the known labour defaults change. Its original57 follow-up improves34→35 pass, with22 unmet; only the correctly placed source logo gains a pass. Graph and whole-document readiness remain unaccepted.

### Compare PDFs

```sh
cargo run -p jade-pdf-diff -- old.pdf new.pdf
cargo run -p jade-pdf-diff -- --ir input.pdf
```

The comparison command writes JSON to stdout and exits with `0` when there are no changes, `1` when changes are found, `2` on errors, or `3` when source or comparison coverage requires review. See the [PDF comparison guide](docs/pdf_diff.md) for rendering and text-layer commands.

PDF comparison uses the pinned Pdfium runtime described in the [comparison guide](docs/pdf_diff.md). For PDF→DOCX rendering with a prepared, verified runtime without downloads, follow the [offline Pdfium guide](docs/pdfium-offline.md).

`ParsedTextElement::baseline_y` exposes a finite origin reported by the SDK for the first horizontal glyph, or verified from an upright horizontal text drawing matrix in the bytes/WASM parser. This now covers supported simple fonts and horizontal CID fonts as well as generated Type1 glyphs. Positive finite font size, verified horizontal writing mode and positive axis-aligned page-space transforms are required. Unknown/vertical writing modes, skew and reflection retain `None`; merged spans keep compatible baseline confidence. Rust callers constructing text elements can still set `baseline_y: None` to retain bounds-based grouping. See the [text baseline guide](docs/text_baselines.md).

`ParsedTextElement::font_weight` preserves an explicitly verified integer FontDescriptor weight in the bytes/WASM parser. Rust struct literals need `font_weight: None` when unconfirmed. DOCX uses verified embedded face attributes first, then the declared weight (700 or greater becomes bold). The native backend also preserves declared weights for a restricted set of embedded subset Type1/CFF resources after confirming their identity and actual SDK embedding; other SDK-only spans retain `None`. See the [source FontDescriptor weight guide](docs/source_font_descriptor_weight.md) and [native subset font weight scope](docs/native_subset_font_weight.md).

`ParsedTextElement::fill_stroke_width` separately records a verified same-color, opaque fill-and-stroke text width. Rust struct literals need `fill_stroke_width: None` when unconfirmed. A supported thin stroke can produce Word emphasis while the original FontDescriptor weight remains 400. Unknown paint effects, colors and font types retain the existing behavior. See [painted text emphasis and its limits](docs/source_painted_text_emphasis.md). The fixed follow-up passes 2,639 Rust tests and preserves 592 of 598 prior runtime payloads byte for byte; only the known labour/CPI default emphasis changes. Labour remains 32/57 and CPI 26/107 source criteria, with whole-document production readiness unaccepted.

`ParsedTextElement::source_background` separately records a verified opaque source rectangle behind text. Rust struct literals need `source_background: None` when unconfirmed. Source-framed table cells use the color only when the proven rectangle covers the whole cell; partial fills, unknown effects, fonts and page groups retain existing styling. See [source table backgrounds and their limits](docs/source_table_cell_backgrounds.md). The fixed follow-up passes 2,651 Rust tests and preserves 595 of 598 prior runtime payloads byte for byte; only the known labour defaults gain source cell backgrounds. The unchanged original57 follow-up is34 pass/23 unmet, with whole-document production readiness unaccepted.

Inferred centred text lines within the body area retain their PDF centre through section-relative paragraph indents, keeping the previous writable width. Substituted font metrics can still change the visible text width.

Text and table offsets share the page margin origin so overhanging headings and graphics do not shift body content.

Tables inferred from PDF rules keep their source-coordinate row order and each grid row’s height, including descending values and content streams that paint the bottom row first. Cells whose modeled text agrees with the source retain individual run styles. Multiline cells use separate source-positioned paragraphs and explicit edge spaces when their paragraph boxes fit the source row height; single-line cells retain their existing text and alignment. Nested and unmatched cells use the existing layout. Overfull cells keep the existing one-paragraph layout and retain verified source run styles when the legacy text matches exactly. Complex cell boundaries, font substitution, and rendered placement still require review; see the [cell-line validation](corpus/validation-native-cell-lines.json).

Repeated, independently outlined cells can form one editable table while retaining physical whitespace bands and empty cells. Only complete source edges create borders; ambiguous glyph ownership retains the existing fallback. This does not establish fill, glyph placement, or page fidelity; see [separated box tables](docs/separated_box_tables.md).

Partial-rule table merges retain rectangular ownership: vertical continuations match the preceding column range, source text belongs to the restart cell, and native styles come from every covered source row. Per-cell source validation detects duplicate loss and values moved to another cell; see [graphic table ownership](docs/graphic_table_ownership.md).

Text paragraphs use an explicit line box based on the source font size, with the PDF's blank space applied separately. This prevents font-dependent automatic leading from accumulating between source lines. Font substitution can still shift the first baseline or change glyph widths.

For newly inferred borderless tables in documents containing ruled tables, source runs retain their verified font, size, color and bold style, with source column gaps, row gaps, physical row order and separate page origins. Canonical standard Helvetica bold fonts without explicit widths use verified ASCII advances. See [scope and remaining layout limits](docs/mixed_table_source_styles.md).

Verified synchronized body lines beneath merged table headers become separate editable Word rows while retaining source edges, unit prefixes and numeric anchors. Ambiguous multiline cells retain their existing fallback. See [logical table rows and remaining limits](docs/source_table_logical_rows.md).

Verified dense identifier/description regions can form editable borderless tables with source word separators, column anchors and row spacing. Independent prose and spaced uppercase labels remain outside this route. See [identifier tables, Census source checks and remaining limits](docs/identifier_description_tables.md).

Verified bare consecutive page numbers can be restored as literal source footers when every page matches its upright baseline, style and edge anchor. Dense prose columns and ambiguous sequences retain their existing fallback. See [bare page footer scope and remaining failures](docs/verified_bare_page_footers.md).

Dense prose with exactly two verified source columns uses Word columns without invented tables when the source anchors and column frame agree. Three-column and identifier-table fallbacks remain unchanged. See [source prose columns and remaining failures](docs/source_prose_columns.md).

Explicit WinAnsi in verified nonsymbolic TrueType fonts retains section signs and Latin characters without overriding authored ToUnicode. See [TrueType text decoding and remaining form-layout failures](docs/truetype_winansi.md).

Verified horizontal image rows retain source page-relative coordinates and point dimensions while unresolved mixed-flow images keep their existing route. See [source image rows and remaining pagination failures](docs/source_image_rows.md).

Table body filtering reuses the original coordinate-assignment decision to retain outside captions and prose. See [source text ownership and the Japanese CPI evaluation](docs/source_table_text_ownership.md).

Measured numeric operands separated by a wide `TJ` gap retain distinct source positions and editable table cells. Numeric word gaps and right anchors are preserved while newly recovered prose gutters stay within their verified body range. See [numeric source positions and remaining fidelity limits](docs/source_numeric_tj_positions.md).

Verified white fill no longer becomes a black rule connecting independent tables. Adjacent source-framed tables retain their shared top and separate editable cells, with one group height reserved in the text flow. See [white fill, adjacent tables and remaining source fidelity limits](docs/source_white_fill_and_adjacent_tables.md).

Detected two-column text retains page-specific Word sections when another page contains an image or inferred table. A final table keeps its page boundary outside the table; floating-image-only and running-header-only pages retain known source dimensions. Sustained central gutters can recover narrow two-column text. Explicit column widths keep right-column prefixes out of the preceding column frame; paragraph offsets use those same widths. Verified column bodies flow from left to right once per source page, including unmatched rows. Column candidates with unsplit body rows crossing into the next column are rejected. Pages containing images or tables, OCR content, full-height tables, and complex column layouts retain limitations; see the [text-column guide](docs/text_columns.md).

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

Supported static Type1 drawing programs can now be converted to embedded OpenType/CFF faces by the pure Rust parser, including the bytes/WASM conversion path. Original code widths and verified Unicode mappings select each face; source rights and notices are retained. Native reconciliation preserves those selections on matching pages. DOCX uses intrinsic bold/italic attributes and respects the no-subsetting flag. This is partial Type1 support, and does not establish page-layout or Microsoft Word fidelity. See the [Type1 support and qualification guide](docs/type1_fonts.md).

Generated CFF real numbers preserve all significant digits using an integer significand and adjusted exponent. This restores previously invisible CMTT code and option text in the tested LibreOffice rendering of GNU Hello without changing glyph programs, Unicode mappings, metrics, or font permissions. See the [CFF rendering qualification](corpus/validation-cff-real-rendering.json) for its measured scope and remaining failures.

Upright generated Type1 text retains one element per source glyph, including separate elements for ordinary letters that share a font. Verified upright elements expose their baseline; logical TJ word gaps can have independent bounds and no source font. For fully measured lines, DOCX uses glyph advances and character spacing, with an owned nonprinting font for logical spaces. Font-size and renderer rounding still limit positioning accuracy. Converter distributions must also include the pinned [ttf-parser MIT notice](crates/jade-pdf2docx-docx/licenses/ttf-parser-0.25.1.txt).

Within a `TJ` array, a forward gap of at least 0.2 em between word-spaced characters restores one missing word separator without changing glyph positions. Explicit spaces, small kerning adjustments, CJK tracking, and separate show operators retain their existing behavior. This bounded spacing heuristic does not establish font substitution or pagination fidelity.

Native conversion preserves physically printed line-end hyphens. Pdfium's control-code representation is restored only for characters that its hyphen API verifies; literal control codes keep their identity. Public PDF tests compare original word boundaries and punctuation without deleting source hyphens.

Native extraction checks rectangular text segments against the SDK's complete page text. When overlapping rectangles repeat adjacent glyphs, it can recover text by SDK character index, retaining every indexed character once. Coherent source spans retain their geometry only when their complete text, style and object bounds also match an original rectangle. Pages with painted control codes keep the existing path; verified printed hyphens and SDK-generated line breaks remain distinct. This corrects duplicated text and the tested GNU page overflow without guessing Unicode. See the [native text guide](docs/native_text.md) and [qualification ledger](corpus/validation-native-segment-identity.json).

When source and native Unicode agree in order on a page, native reconciliation can retain SDK word boundaries inside a source span while preserving source-coded whitespace, geometry, color and weight. CJK tracking and generated Type1 glyph routes retain their existing rules. Source variation selectors omitted by the SDK are retained only when every base scalar agrees; leading, repeated or detached selectors do not establish that correspondence. See the [native source-boundary guide](docs/native_source_word_boundaries.md) for validation status and limitations.

The bytes parser preserves empty `bfchar` destinations and empty elements in `bfrange` destination arrays. These entries produce empty text and keep later character mappings aligned. Incomplete UTF-16 destinations are rejected as whole mappings.

When the built-in program is unavailable, the existing ToUnicode map stays shared and unmapped codes remain explicitly unverified in the quality report; normal best-effort output may still contain fallback text. Strict mode rejects any painted unverified code.

Type0 Font objects with `Identity-H`, a single CIDFont descendant, no `ToUnicode`, and verified Adobe/Japan1 registration can use the pinned Adobe-Japan1-UCS2 table. Explicit ToUnicode remains authoritative. Invalid byte pairs, notdef and CIDs outside the declared Supplement stop parsing with `ParseError::InvalidRegisteredCid`. This currently covers indirect Font objects and Supplements 0–7; it does not establish vertical-layout, inline-font or page-layout fidelity. See the [registered CID guide](docs/registered_cid_decoding.md).

The production DOCX WASM build helper bundles and verifies the Adobe Glyph List, Adobe-Japan1-UCS2, fontTools, and [ReportLab notices](crates/jade-pdf2docx-parser/licenses/reportlab.txt). ReportLab contributes static PDF encoding glyph names and is not a runtime dependency. Distribute the complete generated directory; see the [WASM build guide](tools/wasm-docx-smoke/README.md). Native converter distributions must also include all four parser notices.

Verified small-glyph baseline differences are retained as signed DOCX run positions without changing the source font size. Bounded row recovery uses each verified source column independently and keeps ambiguous or overlapping cases unchanged. This improves tested scientific exponents; it does not establish equation, table, pagination or Microsoft Word fidelity. See the [inline baseline guide](docs/inline_baselines.md).

長方形の罫線を扱う条件と、元の表の所属・色・改ページに残る課題は、[長方形からの罫線抽出](docs/stroked_rectangles.md)を参照してください。
PDF由来の太字・通常体と表の先頭行の改ページについては、[書体と表の先頭行](docs/source_typography_and_table_opening.md)を参照してください。

部分clip付きの短い文字objectが多いnative解析では、一意な所属の共有索引で既定のmetadata予算内の処理を改善しています。曖昧な所属の扱いと検証範囲は、[native clipの文字索引](docs/native_clip_workspace.md)を参照してください。

原本で一行の表セルは、確認できた水平原点へ既定の左右余白を重ねずに配置します。適用条件と残る表示上の問題は、[一行セルの水平位置](docs/native_single_line_cell_origin.md)を参照してください。

表セルの行配置が収まらない場合の文字書式・添字・下線については、[セル書式の保持](docs/native_overfull_cell_styles.md)を参照してください。

小数の行高を持つ複数行セルでは、累積した行の端点をtwipへ丸め、行数に比例した丸め誤差を防ぎます。条件と残る表の問題は、[セル行高の累積丸め](docs/native_cell_twip_rounding.md)を参照してください。

ヘッダーで途切れる表の外辺、複数行の数値列の右揃え、罫線と文字下線の区別については、[部分的な表の外辺と数値配置](docs/native_partial_table_edges.md)を参照してください。

単一のTJ命令で離して描かれた数値のセル対応、本文の語間、図表と段組みの区別については、[原本数値の描画位置の保持](docs/source_numeric_tj_positions.md)を参照してください。

白い塗りによる偽の罫線と横並び表の配置については、[白い塗りと横並びの原本表](docs/source_white_fill_and_adjacent_tables.md)を参照してください。

連続する原本ページの装飾付き番号を、文字と書式を保持してfooterへ移す条件と制約は、[原本ページ番号の保持](docs/source_numbered_footers.md)を参照してください。

ToUnicodeのmappingはtoken単位で読み、一行内の複数entryや複数行の配列を同じ意味として扱います。コメントや文字列を命令と混同しない条件と検証範囲は、[ToUnicodeのrecord処理](docs/tounicode_token_records.md)を参照してください。

ToUnicodeもFontDescriptorもない標準Latin書体の明示WinAnsi文字は、既存のglyph名表で復元します。適用範囲・処理量とメモリの扱い・nativeとの差は、[標準書体のWinAnsi文字対応](docs/named_winansi_text.md)を参照してください。
