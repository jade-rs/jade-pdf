#[cfg(test)]
use std::fs;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Display, Formatter},
    path::PathBuf,
};

use docx_rs::{
    AlignmentType, BorderType, BreakType, Docx, Footer, Header, HeightRule, LineSpacing,
    PageMargin, PageOrientationType, PageSize, Paragraph, ParagraphChild, Pic, Run, RunFonts,
    Section, SectionProperty, SectionType, Shading, ShdType, Tab, TabValueType, Table,
    TableAlignmentType, TableCell, TableCellBorder, TableCellBorderPosition, TableCellBorders,
    TableCellContent, TableCellMargins, TableLayoutType, TableRow, VAlignType, VMergeType,
    WidthType,
};
use jade_pdf2docx_parser::{
    ParseLimits, ParsedBounds, ParsedDocument, ParsedEmbeddedFont, ParsedGraphicElement,
    ParsedImageElement, ParsedPage, ParsedTextElement, QualityReport, parse_pdf_bytes_with_options,
    parse_pdf_with_options,
};

pub use jade_pdf2docx_ocr::{
    OcrCellSpan, OcrPageContent, OcrTable, OcrTextLimits, PositionedOcrLine, VerifiedBlankPage,
};
pub use jade_pdf2docx_parser::{ParseCancellation, ParseError, ParseOptions};

#[cfg(test)]
mod conversion_cancellation_tests;
mod conversion_control;

mod html_table;
mod native_baseline;
mod native_line_pitch;
mod native_spacing;
mod ocr_reconcile;
mod output;
mod package;
mod paragraph_anchor;
#[cfg(not(target_arch = "wasm32"))]
pub use output::validate_distinct_paths;
use package::pack_docx_to_bytes;
mod page_ocr;
mod page_ocr_budget;
mod positioned_ocr;
mod region_ocr;
mod whole_page_ocr;
mod word_page_fit;
pub use page_ocr::{
    convert_minimum_pdf_to_docx_with_bounded_page_ocr,
    convert_minimum_pdf_to_docx_with_bounded_page_ocr_content,
    convert_minimum_pdf_to_docx_with_bounded_region_ocr_content,
    convert_minimum_pdf_to_docx_with_page_ocr,
};
pub use page_ocr_budget::{PageOcrLimits, PageOcrRaster};
pub use word_page_fit::docx_quality_report;
#[cfg(not(target_arch = "wasm32"))]
mod output_path;
#[cfg(not(target_arch = "wasm32"))]
pub use output_path::resolve_output_parent;
mod column_bands;
mod raw_cid_glyphs;
mod rule_glyphs;
mod ruleless_table;
mod stacked_rules;
#[cfg(not(target_arch = "wasm32"))]
mod staged_output;
mod table_cell_text;
mod table_grid;
mod text_line_rows;
mod vertical_forms;
#[cfg(not(target_arch = "wasm32"))]
pub use staged_output::StagedDocxOutput;

pub use html_table::{HtmlTableParseError, ocr_table_from_html};

/// 自動 OCR を起動する本文文字数の既定下限。
pub const DEFAULT_AUTO_OCR_MIN_TEXT_CHARS_PER_PAGE: usize = 8;

/// PDF 表罫線の交点に生じる微小な隙間を同一境界として扱う許容幅。
const TABLE_RULE_JOIN_TOLERANCE_PT: i32 = 3;

/// 1 列でも本文幅の罫線ボックスとして表扱いする最小幅。
const WIDE_SINGLE_COLUMN_TABLE_MIN_WIDTH_PT: f32 = 180.0;

/// 位置付き PDF 表で左右のセル余白に用いる既定値（twips）。
const POSITIONED_TABLE_CELL_HORIZONTAL_MARGIN_TWIPS: usize = 108;

/// MS PGothic を代替する位置付き PDF 表の左右セル余白（twips）。
const MS_PGOTHIC_TABLE_CELL_HORIZONTAL_MARGIN_TWIPS: usize = 104;

/// 明朝系fontの通常表でbaselineを補う上セル余白（twips）。
const MINCHO_TABLE_CELL_TOP_MARGIN_TWIPS: usize = 60;

/// 高い1セル表で中央配置した明朝系fontのascentを補う上セル余白（twips）。
const CENTERED_MINCHO_CONTAINER_TOP_MARGIN_TWIPS: usize = 270;

/// DOCX の既定font size（half-points）。
const DEFAULT_DOCX_FONT_SIZE_HALF_POINTS: usize = 22;

/// docx-rs が均等段組へ使う既定の段間隔（twips）。
const DEFAULT_DOCX_COLUMN_SPACE_TWIPS: f32 = 425.0;

/// 同じ text row と段組前後を判定する PDF y 座標許容差（pt）。
const TEXT_ROW_Y_TOLERANCE_PT: f32 = 4.0;

/// PDF 自動 OCR 判定の方針を表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoOcrPolicy {
    /// この文字数未満のページを「テキストレイヤ不足」とみなす。
    pub min_text_chars_per_page: usize,
    /// `true` のとき、画像があるページだけを OCR 対象にする。
    pub require_page_image: bool,
}

impl Default for AutoOcrPolicy {
    fn default() -> Self {
        Self {
            min_text_chars_per_page: DEFAULT_AUTO_OCR_MIN_TEXT_CHARS_PER_PAGE,
            require_page_image: true,
        }
    }
}

/// 既定方針で PDF 解析結果から OCR 対象ページ番号を返す。
///
/// ページ番号は 0 始まり。テキスト情報を十分に持つ PDF ページ内の画像は
/// OCR 対象にしない。
pub fn automatic_ocr_pdf_page_indices(parsed: &ParsedDocument) -> Vec<usize> {
    automatic_ocr_pdf_page_indices_with_policy(parsed, AutoOcrPolicy::default())
}

/// 指定方針で PDF 解析結果から OCR 対象ページ番号を返す。
///
/// ページ番号は 0 始まり。`require_page_image` が `true` の場合、画像があり、
/// 座標付きテキストが閾値未満のページだけをスキャンページとみなす。
pub fn automatic_ocr_pdf_page_indices_with_policy(
    parsed: &ParsedDocument,
    policy: AutoOcrPolicy,
) -> Vec<usize> {
    if parsed.page_count == 0 {
        return Vec::new();
    }
    if policy.require_page_image && parsed.image_elements.is_empty() {
        return Vec::new();
    }
    let mut text_chars_by_page = vec![0usize; parsed.page_count];
    for element in &parsed.text_elements {
        if element.page < parsed.page_count {
            text_chars_by_page[element.page] += meaningful_char_count(&element.text);
        }
    }
    let mut has_image_by_page = vec![false; parsed.page_count];
    for image in &parsed.image_elements {
        if image.page < parsed.page_count {
            has_image_by_page[image.page] = true;
        }
    }
    has_image_by_page
        .into_iter()
        .zip(text_chars_by_page)
        .enumerate()
        .filter_map(|(page, (has_image, text_chars))| {
            let image_ok = !policy.require_page_image || has_image;
            (image_ok && text_chars < policy.min_text_chars_per_page).then_some(page)
        })
        .collect()
}

/// 空白を除いた意味のある文字数を数える。
fn meaningful_char_count(text: &str) -> usize {
    text.chars().filter(|ch| !ch.is_whitespace()).count()
}

/// 最小変換に必要な入力パラメータを表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinimumPdfToDocxRequest {
    /// 変換元 PDF ファイルパス。
    pub input_pdf_path: PathBuf,
    /// 変換先 DOCX ファイルパス。
    pub output_docx_path: PathBuf,
    /// OCR で得た行テキスト（任意）。
    ///
    /// 空行・空白行は無視する。CJK OCR が文中で分割した行は段落化前に連結する。
    /// 値がある場合は parser の `text_preview` より優先して使用する。
    pub ocr_text_lines: Option<Vec<String>>,
    /// OCR で得た表（セル格子）を DOCX の表として出力する場合に指定する。
    ///
    /// `validate()` に通る矩形表のみ受け付ける。`None` のときは表を出力しない。
    pub ocr_table: Option<OcrTable>,
    /// `true` のとき、デバッグ用メタ段落を省略し、抽出本文・表・画像を DOCX に書く。
    /// 本文が空のときもメタ情報は追加しない。
    pub clean_output: bool,
}

/// 最小変換結果のメタ情報を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinimumConversionReport {
    /// 元画像の白置換など、変換で検出した品質欠落。
    pub quality: QualityReport,
    /// 変換元 PDF の絶対/相対パス。
    pub source_pdf_path: PathBuf,
    /// 変換元 PDF のバイト長。
    pub source_pdf_byte_len: usize,
    /// 変換元 PDF のヘッダ文字列（先頭 8 文字）。
    pub source_pdf_header: String,
    /// 変換元 PDF のバージョン文字列（例: `1.7`）。
    pub source_pdf_version: String,
    /// 変換元 PDF から抽出した最小テキストプレビュー。
    pub source_text_preview: String,
    /// 生成した DOCX の出力先パス。
    pub output_docx_path: PathBuf,
}

/// DOCX に貼り込むレンダリング済み PDF ページ画像を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPageImage {
    /// PNG 形式の画像バイト列。
    pub png_bytes: Vec<u8>,
    /// 画像幅（px）。
    pub width_px: u32,
    /// 画像高さ（px）。
    pub height_px: u32,
}

impl RenderedPageImage {
    /// PNG バイト列と寸法からページ画像を構築する。
    pub fn new(png_bytes: Vec<u8>, width_px: u32, height_px: u32) -> Self {
        Self {
            png_bytes,
            width_px,
            height_px,
        }
    }
}

/// PDF から DOCX への最小変換で発生するエラーを表す。
#[derive(Debug)]
pub enum ConvertError {
    /// ファイル読み書き失敗。
    Io(std::io::Error),
    /// PDF 解析失敗。
    Parse(jade_pdf2docx_parser::ParseError),
    /// DOCX パッキング失敗。
    Pack(String),
    /// 出力先が変換元 PDF と同じファイルを指している。
    OutputAliasesInput,
    /// OCR 表 IR が不正。
    InvalidOcrTable(jade_pdf2docx_ocr::OcrTableError),
    /// HTML 表の解析に失敗。
    HtmlTableParse(HtmlTableParseError),
    /// レンダリング済みページ画像が DOCX 埋め込みに使えない。
    InvalidRenderedPage(String),
    /// ページ単位のOCRが失敗した。
    Ocr(String),
}

impl Display for ConvertError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::Io(err) => write!(f, "io error: {err}"),
            ConvertError::Parse(err) => write!(f, "parse error: {err}"),
            ConvertError::OutputAliasesInput => write!(f, "output path refers to the input PDF"),
            ConvertError::Pack(err) => write!(f, "docx pack error: {err}"),
            ConvertError::InvalidOcrTable(err) => write!(f, "invalid ocr table: {err}"),
            ConvertError::HtmlTableParse(err) => write!(f, "html table parse error: {err}"),
            ConvertError::InvalidRenderedPage(err) => write!(f, "invalid rendered page: {err}"),
            ConvertError::Ocr(err) => write!(f, "OCR failed: {err}"),
        }
    }
}

impl std::error::Error for ConvertError {}

impl From<std::io::Error> for ConvertError {
    fn from(value: std::io::Error) -> Self {
        ConvertError::Io(value)
    }
}

impl From<jade_pdf2docx_parser::ParseError> for ConvertError {
    fn from(value: jade_pdf2docx_parser::ParseError) -> Self {
        ConvertError::Parse(value)
    }
}

impl From<HtmlTableParseError> for ConvertError {
    fn from(value: HtmlTableParseError) -> Self {
        ConvertError::HtmlTableParse(value)
    }
}

/// 最小要件の PDF -> DOCX 変換を行う。
///
/// `clean_output` のときは本文・表・画像を出力し、それ以外はデバッグ情報を追加する。
/// 検出した画像置換は report へ返す。品質欠落を拒否する場合は options API を使う。
pub fn convert_minimum_pdf_to_docx(
    request: &MinimumPdfToDocxRequest,
) -> Result<MinimumConversionReport, ConvertError> {
    convert_minimum_pdf_to_docx_with_limits(request, &ParseLimits::default())
}

/// 指定された解析予算を適用し、資源超過時は既存出力を変更せずに返す。
pub fn convert_minimum_pdf_to_docx_with_limits(
    request: &MinimumPdfToDocxRequest,
    limits: &ParseLimits,
) -> Result<MinimumConversionReport, ConvertError> {
    convert_minimum_pdf_to_docx_with_options(
        request,
        &ParseOptions {
            limits: limits.clone(),
            strict: false,
            ..Default::default()
        },
    )
}

/// 品質方針・資源上限・協調キャンセルを適用し、拒否時は既存出力を保持する。
/// 最後の停止確認後に置換が完了した場合は成功を返す。
pub fn convert_minimum_pdf_to_docx_with_options(
    request: &MinimumPdfToDocxRequest,
    options: &ParseOptions,
) -> Result<MinimumConversionReport, ConvertError> {
    convert_pdf_file_with_check(request, options, |_| conversion_control::check(options))
}

/// 変換段階で共通の停止確認を呼び出し、ファイル変換を進める。
fn convert_pdf_file_with_check(
    request: &MinimumPdfToDocxRequest,
    options: &ParseOptions,
    mut check: impl FnMut(conversion_control::Stage) -> Result<(), ConvertError>,
) -> Result<MinimumConversionReport, ConvertError> {
    use conversion_control::Stage;
    check(Stage::Start)?;
    output::validate_distinct_paths(&request.input_pdf_path, &request.output_docx_path)?;
    let mut parsed = parse_pdf_with_options(&request.input_pdf_path, options)?;
    word_page_fit::assess(&mut parsed, options.strict)?;
    check(Stage::Parsed)?;
    drop_blank_page_backgrounds(&mut parsed);
    rule_glyphs::convert_to_graphics(&mut parsed);
    map_known_private_use_glyphs(&mut parsed);
    drop_rule_glyphs(&mut parsed);
    recover_decimal_points(&mut parsed);
    recover_angle_marks(&mut parsed);
    split_text_at_vertical_rules(&mut parsed);

    let version = format!("{}.{}", parsed.version.major, parsed.version.minor);
    let normalized_text_preview =
        resolve_text_preview(&parsed.text_preview, &request.ocr_text_lines);
    let normalized_text_preview = if normalized_text_preview.trim().is_empty() {
        "(no text extracted)".to_string()
    } else {
        normalized_text_preview
    };

    let positioned_tables = if request.ocr_table.is_none() {
        infer_positioned_tables_from_pdf_graphics(&parsed)
    } else {
        Vec::new()
    };
    let inferred_table = request
        .ocr_table
        .clone()
        .or_else(|| positioned_tables.first().map(|table| table.table.clone()));

    check(Stage::Layout)?;
    let docx = if request.clean_output
        && request.ocr_text_lines.is_none()
        && has_structured_pdf_content(&parsed, &positioned_tables)
    {
        build_structured_docx_from_owned_images(&mut parsed, &inferred_table, &positioned_tables)?
    } else if request.clean_output && has_body_content(&request.ocr_text_lines, &inferred_table) {
        build_body_docx(&request.ocr_text_lines, &inferred_table)?
    } else if request.clean_output {
        Docx::new().add_paragraph(Paragraph::new().add_run(text_run("(no extractable content)")))
    } else {
        let docx = Docx::new()
            .add_paragraph(Paragraph::new().add_run(text_run("minimum pdf to docx conversion")))
            .add_paragraph(Paragraph::new().add_run(text_run(format!(
                "source: {}",
                request.input_pdf_path.display()
            ))))
            .add_paragraph(Paragraph::new().add_run(text_run(format!("pdf version: {version}"))))
            .add_paragraph(
                Paragraph::new().add_run(text_run(format!("byte length: {}", parsed.byte_len))),
            );
        let mut docx = add_text_preview_paragraphs(docx, &normalized_text_preview);

        if let Some(ref table) = inferred_table {
            table.validate().map_err(ConvertError::InvalidOcrTable)?;
            docx = docx.add_table(docx_table_from_ocr_table(table));
        }
        docx
    };

    check(Stage::Built)?;
    // DOCXが画像を所有した後は解析側のコピーを解放し、ZIP生成と重複保持しない。
    parsed.image_elements.clear();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let file = package::pack_docx_to_file(docx, &parsed.embedded_fonts)?;
        check(Stage::Packed)?;
        output::write_atomic_from_reader(&request.output_docx_path, file, &mut check)?;
    }
    #[cfg(target_arch = "wasm32")]
    {
        let bytes = pack_docx_to_bytes(docx, &parsed.embedded_fonts)?;
        check(Stage::Packed)?;
        output::write_atomic(&request.output_docx_path, &bytes, &mut check)?;
    }

    Ok(MinimumConversionReport {
        quality: parsed.quality,
        source_pdf_path: parsed.path,
        source_pdf_byte_len: parsed.byte_len,
        source_pdf_header: parsed.header,
        source_pdf_version: version,
        source_text_preview: normalized_text_preview,
        output_docx_path: request.output_docx_path.clone(),
    })
}

/// PDF バイト列から最小 DOCX バイト列を生成する。
pub fn convert_minimum_pdf_bytes_to_docx_bytes(
    source_name: impl Into<PathBuf>,
    pdf_bytes: &[u8],
) -> Result<Vec<u8>, ConvertError> {
    convert_minimum_pdf_bytes_to_docx_bytes_with_options(
        source_name,
        pdf_bytes,
        &ParseOptions::default(),
    )
}

/// bytes 変換にも file 変換と同じ資源上限・strict 品質拒否・協調キャンセルを適用する。
pub fn convert_minimum_pdf_bytes_to_docx_bytes_with_options(
    source_name: impl Into<PathBuf>,
    pdf_bytes: &[u8],
    options: &ParseOptions,
) -> Result<Vec<u8>, ConvertError> {
    convert_pdf_bytes_with_check(source_name.into(), pdf_bytes, options, |_| {
        conversion_control::check(options)
    })
}

/// 段階ごとの停止確認を共有してDOCX bytesを構築する。
fn convert_pdf_bytes_with_check(
    source_name: PathBuf,
    pdf_bytes: &[u8],
    options: &ParseOptions,
    mut check: impl FnMut(conversion_control::Stage) -> Result<(), ConvertError>,
) -> Result<Vec<u8>, ConvertError> {
    use conversion_control::Stage;
    check(Stage::Start)?;
    let mut parsed = parse_pdf_bytes_with_options(source_name.clone(), pdf_bytes, options)?;
    word_page_fit::assess(&mut parsed, options.strict)?;
    check(Stage::Parsed)?;
    drop_blank_page_backgrounds(&mut parsed);
    rule_glyphs::convert_to_graphics(&mut parsed);
    map_known_private_use_glyphs(&mut parsed);
    drop_rule_glyphs(&mut parsed);
    recover_decimal_points(&mut parsed);
    recover_angle_marks(&mut parsed);
    split_text_at_vertical_rules(&mut parsed);
    let positioned_tables = infer_positioned_tables_from_pdf_graphics(&parsed);
    let inferred_table = positioned_tables.first().map(|table| table.table.clone());
    let mut request = MinimumPdfToDocxBytesRequest {
        source_pdf_path: source_name,
        parsed,
        ocr_text_lines: None,
        ocr_table: inferred_table,
        positioned_tables,
        clean_output: true,
    };
    check(Stage::Layout)?;
    let docx = if has_structured_pdf_content(&request.parsed, &request.positioned_tables) {
        build_structured_docx_from_owned_images(
            &mut request.parsed,
            &request.ocr_table,
            &request.positioned_tables,
        )?
    } else {
        build_minimum_docx(&request)?
    };
    check(Stage::Built)?;
    // ファイル変換と同様に、ZIP生成前に解析側の画像コピーを解放する。
    request.parsed.image_elements.clear();
    let bytes = pack_docx_to_bytes(docx, &request.parsed.embedded_fonts)?;
    check(Stage::Packed)?;
    Ok(bytes)
}

/// レンダリング済み PNG ページ列から DOCX バイト列を生成する。
pub fn convert_rendered_png_pages_to_docx_bytes(
    pages: &[RenderedPageImage],
) -> Result<Vec<u8>, ConvertError> {
    validate_rendered_pages(pages)?;
    let mut docx = Docx::new();
    for (index, page) in pages.iter().enumerate() {
        let run = Run::new().add_image(Pic::new_with_dimensions(
            page.png_bytes.clone(),
            page.width_px,
            page.height_px,
        ));
        let (page_width_twips, page_height_twips) = rendered_page_size_twips(page);
        let paragraph = rendered_page_paragraph(run).section_property(
            SectionProperty::new()
                .page_size(PageSize::new().size(page_width_twips, page_height_twips))
                .page_margin(rendered_page_margin()),
        );
        docx = docx.add_paragraph(paragraph);
        if index + 1 < pages.len() {
            docx = docx.add_paragraph(rendered_page_paragraph(
                Run::new().add_break(BreakType::Page),
            ));
        }
    }
    pack_docx_to_bytes(docx, &[])
}

/// レンダリング済みページ画像が DOCX へ埋め込める状態か検証する。
fn validate_rendered_pages(pages: &[RenderedPageImage]) -> Result<(), ConvertError> {
    if pages.is_empty() {
        return Err(ConvertError::InvalidRenderedPage(
            "at least one rendered page is required".to_string(),
        ));
    }
    for (index, page) in pages.iter().enumerate() {
        if page.width_px == 0 || page.height_px == 0 {
            return Err(ConvertError::InvalidRenderedPage(format!(
                "page {index} has zero dimensions"
            )));
        }
        if page.png_bytes.is_empty() {
            return Err(ConvertError::InvalidRenderedPage(format!(
                "page {index} has empty PNG bytes"
            )));
        }
        if !looks_like_png(&page.png_bytes) {
            return Err(ConvertError::InvalidRenderedPage(format!(
                "page {index} is not a PNG image"
            )));
        }
    }
    Ok(())
}

/// PNG signature を持つかを最小確認する。
fn looks_like_png(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

/// DOCX XML に書き込める文字だけを残す。
fn sanitize_docx_text(text: &str) -> String {
    text.chars().filter(|ch| is_valid_xml_char(*ch)).collect()
}

/// XML 1.0 text node に出せる文字か判定する。
fn is_valid_xml_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x09 | 0x0a | 0x0d | 0x20..=0xd7ff | 0xe000..=0xfffd | 0x10000..=0x10ffff
    )
}

/// XML 不正文字を除去した DOCX run を作る。
fn text_run(text: impl AsRef<str>) -> Run {
    Run::new().add_text(sanitize_docx_text(text.as_ref()))
}

/// レンダリング済みページ用の余白なしページ設定を返す。
fn rendered_page_margin() -> PageMargin {
    PageMargin::new()
        .top(0)
        .right(0)
        .bottom(0)
        .left(0)
        .header(0)
        .footer(0)
        .gutter(0)
}

/// Word のページサイズ（twips）を表示画像サイズから計算する。
fn rendered_page_size_twips(page: &RenderedPageImage) -> (u32, u32) {
    (
        page.width_px.saturating_mul(15),
        page.height_px.saturating_mul(15),
    )
}

/// レンダリングページ画像用の段落を作る。
fn rendered_page_paragraph(run: Run) -> Paragraph {
    Paragraph::new()
        .line_spacing(LineSpacing::new().before(0).after(0))
        .add_run(run)
}

/// 連続する表の視覚的な詰まりを避けるための空き段落を返す。
fn table_gap_paragraph() -> Paragraph {
    Paragraph::new().line_spacing(LineSpacing::new().after(160))
}

/// バイト列変換に必要な入力パラメータを表す。
struct MinimumPdfToDocxBytesRequest {
    /// 変換元 PDF の表示用パス。
    source_pdf_path: PathBuf,
    /// 解析済み PDF。
    parsed: ParsedDocument,
    /// OCR で得た行テキスト（任意）。
    ocr_text_lines: Option<Vec<String>>,
    /// OCR で得た表（セル格子）。
    ocr_table: Option<OcrTable>,
    /// PDF 座標上の位置を保持した推定表。
    positioned_tables: Vec<PositionedTable>,
    /// デバッグ用メタ段落を省略するか。
    clean_output: bool,
}

/// OCR 由来の本文（行または表）が `clean_output` に使えるか判定する。
fn has_body_content(ocr_text_lines: &Option<Vec<String>>, table: &Option<OcrTable>) -> bool {
    let lines_ok = ocr_text_lines
        .as_ref()
        .is_some_and(|lines| lines.iter().any(|line| !line.trim().is_empty()));
    lines_ok || table.is_some()
}

/// 構造化 DOCX 出力に回す PDF 由来要素があるか判定する。
fn has_structured_pdf_content(
    parsed: &ParsedDocument,
    positioned_tables: &[PositionedTable],
) -> bool {
    !parsed.text_elements.is_empty()
        || !parsed.image_elements.is_empty()
        || !positioned_tables.is_empty()
}

/// 最小 DOCX の本文・表・画像を組み立てる。
fn build_minimum_docx(request: &MinimumPdfToDocxBytesRequest) -> Result<Docx, ConvertError> {
    let version = format!(
        "{}.{}",
        request.parsed.version.major, request.parsed.version.minor
    );
    let normalized_text_preview =
        resolve_text_preview(&request.parsed.text_preview, &request.ocr_text_lines);
    let normalized_text_preview = if normalized_text_preview.trim().is_empty() {
        "(no text extracted)".to_string()
    } else {
        normalized_text_preview
    };

    let docx = if request.clean_output {
        if request.ocr_text_lines.is_none()
            && has_structured_pdf_content(&request.parsed, &request.positioned_tables)
        {
            build_structured_docx_from_parsed(
                &request.parsed,
                &request.ocr_table,
                &request.positioned_tables,
            )?
        } else {
            let body_lines = request
                .ocr_text_lines
                .clone()
                .or_else(|| text_preview_to_body_lines(&normalized_text_preview));
            if has_body_content(&body_lines, &request.ocr_table) {
                build_body_docx(&body_lines, &request.ocr_table)?
            } else {
                Docx::new()
                    .add_paragraph(Paragraph::new().add_run(text_run("(no extractable content)")))
            }
        }
    } else {
        let docx = Docx::new()
            .add_paragraph(Paragraph::new().add_run(text_run("minimum pdf to docx conversion")))
            .add_paragraph(Paragraph::new().add_run(text_run(format!(
                "source: {}",
                request.source_pdf_path.display()
            ))))
            .add_paragraph(Paragraph::new().add_run(text_run(format!("pdf version: {version}"))))
            .add_paragraph(Paragraph::new().add_run(text_run(format!(
                "byte length: {}",
                request.parsed.byte_len
            ))));
        let mut docx = add_text_preview_paragraphs(docx, &normalized_text_preview);

        if let Some(ref table) = request.ocr_table {
            table.validate().map_err(ConvertError::InvalidOcrTable)?;
            docx = docx.add_table(docx_table_from_ocr_table(table));
        }
        docx
    };

    Ok(docx)
}

/// 解析済み PDF の座標付き要素から structured DOCX を組み立てる。
fn build_structured_docx_from_parsed(
    parsed: &ParsedDocument,
    ocr_table: &Option<OcrTable>,
    positioned_tables: &[PositionedTable],
) -> Result<Docx, ConvertError> {
    build_structured_docx_with_page_ocr(
        parsed,
        ocr_table,
        positioned_tables,
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
}

/// 所有する解析画像を移し、構築後の解析結果にPNGのコピーを残さない。
fn build_structured_docx_from_owned_images(
    parsed: &mut ParsedDocument,
    ocr_table: &Option<OcrTable>,
    positioned_tables: &[PositionedTable],
) -> Result<Docx, ConvertError> {
    let mut images = Some(
        parsed
            .image_elements
            .iter_mut()
            .map(|image| std::mem::take(&mut image.png_bytes))
            .collect::<Vec<_>>(),
    );
    build_structured_docx_with_image_payloads(
        parsed,
        ocr_table,
        positioned_tables,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &mut images,
    )
}

/// 罫線用フォントで描いた細長い、または同じ字をつないだ私用領域の字（傍線・罫）を本文から除く。
///
/// 官報などは傍線を罫線用フォントの字で組み、私用領域の文字として抽出される。
/// 文字のまま残すと代替フォントで豆腐になり、セル内の行の判定も乱す。線として
/// graphics へ移すと表の行境界と取り違えるため、細長い字だけを取り除く。
/// 点のような小さい字（小数点など）は文字のまま残す。
fn drop_rule_glyphs(parsed: &mut ParsedDocument) {
    let is_private_use = |ch: char| ('\u{E000}'..='\u{F8FF}').contains(&ch);
    parsed.text_elements.retain(|text| {
        let body = text.text.trim();
        if body.is_empty() || !body.chars().all(is_private_use) {
            return true;
        }
        let bounds = normalized_bounds(text.bounds);
        let (width, height) = (bounds.x1 - bounds.x0, bounds.y1 - bounds.y0);
        let line_like = (height <= 1.5 && width >= 3.0) || (width <= 1.5 && height >= 3.0);
        // 同じ字を 3 つ以上つないだだけの文字片も、罫線を字で組んだものとみなす。
        let mut chars = body.chars();
        let first = chars.next();
        let repeated = body.chars().count() >= 3 && chars.all(|ch| Some(ch) == first);
        !(line_like || repeated)
    });
}

/// 官報で使われる DynaFont（フォント名に `DFHS` を含む）の、ToUnicode のない字形が
/// Pdfium で割り当てられる私用領域の符号位置と、その字。
///
/// 縦書きの見出しの数字・財務表の負の印・座標の度分秒・小数点・項目記号など。
/// 字形を目視で確認したものだけを載せる。
const DYNAFONT_PRIVATE_USE_GLYPHS: &[(char, &str)] = &[
    ('\u{E741}', "（注）"),
    ('\u{E888}', "△"),
    ('\u{E88A}', "0"),
    ('\u{E88B}', "1"),
    ('\u{E88C}', "2"),
    ('\u{E88D}', "3"),
    ('\u{E88E}', "4"),
    ('\u{E88F}', "5"),
    ('\u{E890}', "6"),
    ('\u{E891}', "7"),
    ('\u{E892}', "8"),
    ('\u{E893}', "9"),
    ('\u{EA6D}', "′"),
    ('\u{EA6E}', "″"),
    ('\u{EA6F}', "°"),
    ('\u{EA74}', "."),
    ('\u{EA75}', ","),
    ('\u{E9E5}', "（ア）"),
    ('\u{E9E6}', "（イ）"),
    ('\u{E9E7}', "（ウ）"),
    ('\u{E9E8}', "（エ）"),
    ('\u{E9E9}', "（オ）"),
    ('\u{E9EA}', "（カ）"),
    ('\u{E9EB}', "（キ）"),
    ('\u{E9F5}', "（チ）"),
    ('\u{E712}', "□"),
    ('\u{E61C}', "－"),
    ('\u{E68C}', "㎠"),
    ('\u{E467}', "辻"),
    ('\u{E469}', "鄭"),
];

/// 連番で並ぶ DynaFont の項目記号（先頭の私用領域の符号位置, 対応する Unicode の先頭, 個数）。
///
/// ⑴⑵⑶⑷ と ①② を目視で確認し、Unicode の連番と同じ順に並ぶことから範囲で対応させる。
const DYNAFONT_PRIVATE_USE_RUNS: &[(u32, u32, u32)] = &[(0xE7B2, 0x2474, 20), (0xE7D1, 0x2460, 20)];

/// 括弧付きのローマ数字（先頭の私用領域の符号位置, ローマ数字の先頭, 個数）。kanpo-parser が
/// 原本の字形で確かめた（Ⅰ）〜（Ⅸ）・（ⅰ）〜（ⅹ）の範囲。
const DYNAFONT_PARENTHESIZED_ROMAN_RUNS: &[(u32, u32, u32)] =
    &[(0xE82F, 0x2160, 9), (0xE839, 0x2170, 10)];

/// DynaFont の私用領域の字に対応する文字列を返す。
fn dynafont_private_use_glyph(ch: char) -> Option<String> {
    if let Some((_, value)) = DYNAFONT_PRIVATE_USE_GLYPHS
        .iter()
        .find(|(glyph, _)| *glyph == ch)
    {
        return Some((*value).to_string());
    }
    let code = ch as u32;
    if let Some((start, roman, _)) = DYNAFONT_PARENTHESIZED_ROMAN_RUNS
        .iter()
        .find(|(start, _, count)| (*start..*start + *count).contains(&code))
    {
        return char::from_u32(roman + code - start).map(|numeral| format!("（{numeral}）"));
    }
    DYNAFONT_PRIVATE_USE_RUNS
        .iter()
        .find(|(start, _, count)| (*start..*start + *count).contains(&code))
        .and_then(|(start, unicode, _)| char::from_u32(unicode + code - start))
        .map(String::from)
}

/// 縦書き用の約物を横組みの字へ戻し、既知のフォントの私用領域の字・Unicode の対応が無い字を
/// Unicode の字へ置き換え、罫線用フォントの字は除く。
fn map_known_private_use_glyphs(parsed: &mut ParsedDocument) {
    let is_private_use = |ch: char| ('\u{E000}'..='\u{F8FF}').contains(&ch);
    parsed.text_elements.retain_mut(|text| {
        if let Some(normalized) = vertical_forms::normalize(&text.text) {
            text.text = normalized;
        }
        if text
            .text
            .chars()
            .any(|ch| raw_cid_glyphs::raw_cid(ch).is_some())
        {
            let rule_font = text
                .font_name
                .as_deref()
                .is_some_and(|font| font.contains("Line"));
            text.text = raw_cid_glyphs::restore(&text.text, rule_font);
            if text.text.trim().is_empty() {
                return false;
            }
        }
        let Some(font) = text.font_name.as_deref() else {
            return true;
        };
        if !text.text.chars().any(is_private_use) {
            return true;
        }
        // 罫線用フォント（MFHSLine など）の私用領域の字は、線を字で組んだもの。
        if font.contains("Line") {
            text.text = text
                .text
                .chars()
                .filter(|ch| !is_private_use(*ch))
                .collect();
            return !text.text.trim().is_empty();
        }
        if font.contains("DFHS") {
            let mut mapped = String::with_capacity(text.text.len());
            for ch in text.text.chars() {
                match dynafont_private_use_glyph(ch) {
                    Some(value) => mapped.push_str(&value),
                    None => mapped.push(ch),
                }
            }
            text.text = mapped;
        }
        true
    });
}

/// 数字に挟まれた、Unicode の分からない小さな点の字を小数点（`.`）に戻す。
///
/// 官報の数値は小数点だけを ToUnicode のない別フォントで組むことがあり、私用領域の
/// 文字として抽出される。同じ行で左右にすき間なく数字が続き、字の大きさが文字サイズの
/// 3 割以下でベースライン付近にある 1 文字だけを小数点とみなす。
fn recover_decimal_points(parsed: &mut ParsedDocument) {
    let is_private_use = |ch: char| ('\u{E000}'..='\u{F8FF}').contains(&ch);
    let candidates = parsed
        .text_elements
        .iter()
        .enumerate()
        .filter(|(_, text)| {
            let mut chars = text.text.trim().chars();
            matches!((chars.next(), chars.next()), (Some(ch), None) if is_private_use(ch))
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    for index in candidates {
        let dot = normalized_bounds(parsed.text_elements[index].bounds);
        let page = parsed.text_elements[index].page;
        let size = (dot.x1 - dot.x0).max(dot.y1 - dot.y0);
        let neighbour = |want_left: bool| {
            parsed.text_elements.iter().find(|text| {
                let bounds = normalized_bounds(text.bounds);
                let em = text.font_size.max(bounds.y1 - bounds.y0).max(1.0);
                let digit = if want_left {
                    text.text.trim_end().chars().last()
                } else {
                    text.text.trim_start().chars().next()
                }
                .is_some_and(|ch| ch.is_ascii_digit() || ('０'..='９').contains(&ch));
                let touching = if want_left {
                    (dot.x0 - bounds.x1).abs() <= 0.25 * em
                } else {
                    (bounds.x0 - dot.x1).abs() <= 0.25 * em
                };
                // 点がその数字の行の下側（ベースライン付近）にあること。
                let low = dot.y0 >= bounds.y0 - 0.2 * em && dot.y1 <= bounds.y0 + 0.4 * em;
                text.page == page && digit && touching && low && size <= 0.3 * em
            })
        };
        if neighbour(true).is_some() && neighbour(false).is_some() {
            parsed.text_elements[index].text = ".".to_string();
        }
    }
}

/// 度分秒の記号を Unicode の分からない私用領域の字で組んだ座標から、記号を復元する。
///
/// 同じ行で続く文字片をつないだ文字列に「数字 P 数字 Q 数字（小数可）R」（P・Q・R は互いに
/// 異なる私用領域の字）が現れたら、P を `°`、Q を `′`、R を `″` と学習し、矛盾がなければ
/// 文書中の同じ字を置き換える。
fn recover_angle_marks(parsed: &mut ParsedDocument) {
    let mut votes = BTreeMap::<char, BTreeMap<char, usize>>::new();
    let pages = parsed
        .text_elements
        .iter()
        .map(|text| text.page)
        .collect::<BTreeSet<_>>();
    for page in pages {
        let mut texts = parsed
            .text_elements
            .iter()
            .filter(|text| text.page == page && !text.text.trim().is_empty())
            .collect::<Vec<_>>();
        texts.sort_by(|a, b| {
            let (a, b) = (normalized_bounds(a.bounds), normalized_bounds(b.bounds));
            b.y1.total_cmp(&a.y1).then(a.x0.total_cmp(&b.x0))
        });
        // 行ごとに x 順へ並べ、1.5 em 以内で続く文字片をつなぐ。
        let mut lines: Vec<Vec<&ParsedTextElement>> = Vec::new();
        for text in texts {
            let bounds = normalized_bounds(text.bounds);
            // 上付きの小さな記号も同じ行に入るよう、高さ方向に重なれば同じ行とみなす。
            match lines.iter_mut().find(|line| {
                let first = normalized_bounds(line[0].bounds);
                bounds.y0 < first.y1 && bounds.y1 > first.y0
            }) {
                Some(line) => line.push(text),
                None => lines.push(vec![text]),
            }
        }
        for mut line in lines {
            line.sort_by(|a, b| {
                a.bounds
                    .x0
                    .min(a.bounds.x1)
                    .total_cmp(&b.bounds.x0.min(b.bounds.x1))
            });
            let mut chain = String::new();
            let mut previous: Option<ParsedBounds> = None;
            for text in line {
                let bounds = normalized_bounds(text.bounds);
                let em = text.font_size.max(bounds.y1 - bounds.y0).max(1.0);
                if previous.is_some_and(|previous| bounds.x0 - previous.x1 > 1.5 * em) {
                    collect_angle_mark_votes(&chain, &mut votes);
                    chain.clear();
                }
                chain.extend(text.text.chars().filter(|ch| !ch.is_whitespace()));
                previous = Some(bounds);
            }
            collect_angle_mark_votes(&chain, &mut votes);
        }
    }
    // 1 つの字の 8 割以上が同じ記号に学習され、記号どうしも重ならない場合だけ置き換える。
    let mut mapping = BTreeMap::new();
    for (glyph, marks) in &votes {
        let total = marks.values().sum::<usize>();
        if let Some((&mark, &count)) = marks.iter().max_by_key(|(_, count)| **count)
            && count * 5 >= total * 4
        {
            mapping.insert(*glyph, mark);
        }
    }
    let marks = mapping.values().collect::<Vec<_>>();
    if marks.iter().collect::<BTreeSet<_>>().len() != marks.len() {
        return;
    }
    if mapping.is_empty() {
        return;
    }
    for text in &mut parsed.text_elements {
        if text.text.chars().any(|ch| mapping.contains_key(&ch)) {
            text.text = text
                .text
                .chars()
                .map(|ch| mapping.get(&ch).copied().unwrap_or(ch))
                .collect();
        }
    }
}

/// つないだ文字列から「数字 P 数字 Q 数字 R」を探し、P・Q・R の記号を投票する。
fn collect_angle_mark_votes(chain: &str, votes: &mut BTreeMap<char, BTreeMap<char, usize>>) {
    let is_private_use = |ch: char| ('\u{E000}'..='\u{F8FF}').contains(&ch);
    let is_digit = |ch: char| ch.is_ascii_digit() || ('０'..='９').contains(&ch);
    let chars = chain.chars().collect::<Vec<_>>();
    let digits_from = |mut index: usize, decimal: bool| {
        let start = index;
        while index < chars.len() && is_digit(chars[index]) {
            index += 1;
        }
        if decimal
            && index > start
            && index + 1 < chars.len()
            && chars[index] == '.'
            && is_digit(chars[index + 1])
        {
            index += 1;
            while index < chars.len() && is_digit(chars[index]) {
                index += 1;
            }
        }
        (index > start).then_some(index)
    };
    for start in 0..chars.len() {
        if !is_digit(chars[start]) || (start > 0 && is_digit(chars[start - 1])) {
            continue;
        }
        let Some(p) = digits_from(start, false) else {
            continue;
        };
        let Some(&degree) = chars.get(p).filter(|ch| is_private_use(**ch)) else {
            continue;
        };
        let Some(q) = digits_from(p + 1, false) else {
            continue;
        };
        let Some(&minute) = chars.get(q).filter(|ch| is_private_use(**ch)) else {
            continue;
        };
        let Some(r) = digits_from(q + 1, true) else {
            continue;
        };
        let Some(&second) = chars.get(r).filter(|ch| is_private_use(**ch)) else {
            continue;
        };
        if degree == minute || minute == second || degree == second {
            continue;
        }
        for (glyph, mark) in [(degree, '°'), (minute, '′'), (second, '″')] {
            *votes.entry(glyph).or_default().entry(mark).or_default() += 1;
        }
    }
}

/// 縦罫線をまたいで 1 つにまとめられた文字片を、罫線の位置で分ける。
///
/// Pdfium は同じ高さに並ぶ文字を、間に罫線があっても 1 つの文字片にまとめることがある
/// （横に並んだ 2 つの表の行など）。表ごとの割り当てより前にページ全体で分けておく。
fn split_text_at_vertical_rules(parsed: &mut ParsedDocument) {
    let mut rules_by_page = BTreeMap::<usize, Vec<table_grid::Rule>>::new();
    for graphic in &parsed.graphics {
        let bounds = normalized_bounds(graphic.bounds);
        if is_vertical_line(&bounds) {
            rules_by_page
                .entry(graphic.page)
                .or_default()
                .push(table_grid::Rule {
                    at: bounds.x0,
                    start: bounds.y0,
                    end: bounds.y1,
                });
        }
    }
    if rules_by_page.is_empty() {
        return;
    }
    let mut output = Vec::with_capacity(parsed.text_elements.len());
    for text in parsed.text_elements.drain(..) {
        let Some(rules) = rules_by_page.get(&text.page) else {
            output.push(text);
            continue;
        };
        let bounds = normalized_bounds(text.bounds);
        let mut crossing = rules
            .iter()
            .map(|rule| rule.at)
            .filter(|x| *x > bounds.x0 + 1.0 && *x < bounds.x1 - 1.0)
            .filter(|x| table_grid::is_ruled(rules, *x, bounds.y0, bounds.y1))
            .collect::<Vec<_>>();
        if crossing.is_empty() {
            output.push(text);
            continue;
        }
        crossing.sort_by(f32::total_cmp);
        crossing.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        let fragment = table_cell_text::CellFragment {
            bounds,
            text: text.text.clone(),
            font_size: text.font_size,
        };
        for piece in table_cell_text::split_across_boundaries(&fragment, &crossing) {
            output.push(ParsedTextElement {
                bounds: piece.bounds,
                text: piece.text,
                ..text.clone()
            });
        }
    }
    parsed.text_elements = output;
}

/// 本文のあるページで、ページの半分以上を覆う白一色の背景画像を取り除く。
///
/// 白い全面画像を inline 画像として出すと、見た目を変えずに本文を次ページへ押し出してしまう。
/// 本文のないページ（スキャン・OCR 対象）や小さな画像は対象にしない。
fn drop_blank_page_backgrounds(parsed: &mut ParsedDocument) {
    let pages_with_text = parsed
        .text_elements
        .iter()
        .filter(|text| !text.text.trim().is_empty())
        .map(|text| text.page)
        .collect::<BTreeSet<_>>();
    let page_areas = parsed
        .pages
        .iter()
        .map(|page| {
            let (width, height) = parsed_page_display_size_points(page);
            (page.page_number, width * height)
        })
        .collect::<BTreeMap<_, _>>();
    parsed.image_elements.retain(|image| {
        let Some(page_area) = page_areas.get(&image.page) else {
            return true;
        };
        let bounds = normalized_bounds(image.bounds);
        let area = (bounds.x1 - bounds.x0) * (bounds.y1 - bounds.y0);
        !(pages_with_text.contains(&image.page)
            && area >= page_area * 0.5
            && jade_pdf2docx_parser::png_is_blank_white(&image.png_bytes))
    });
}

/// OCRで補ったページとnativeのページを同じページ順に構築する。
fn build_structured_docx_with_page_ocr(
    parsed: &ParsedDocument,
    ocr_table: &Option<OcrTable>,
    positioned_tables: &[PositionedTable],
    recognized: &BTreeMap<usize, whole_page_ocr::Lines>,
    region_lines: &BTreeMap<usize, Vec<StructuredTextLine>>,
) -> Result<Docx, ConvertError> {
    build_structured_docx_with_image_payloads(
        parsed,
        ocr_table,
        positioned_tables,
        recognized,
        region_lines,
        &mut None,
    )
}

/// 任意の所有PNG配列から画像を移し、座標・OCR共通の構築処理を行う。
fn build_structured_docx_with_image_payloads(
    parsed: &ParsedDocument,
    ocr_table: &Option<OcrTable>,
    positioned_tables: &[PositionedTable],
    recognized: &BTreeMap<usize, whole_page_ocr::Lines>,
    region_lines: &BTreeMap<usize, Vec<StructuredTextLine>>,
    image_payloads: &mut Option<Vec<Vec<u8>>>,
) -> Result<Docx, ConvertError> {
    let pages = text_lines_by_page_from_pdf_text_elements(parsed, positioned_tables);
    let page_chrome = extract_repeated_page_chrome(parsed, &pages);
    let mut pages = remove_repeated_page_chrome_lines(parsed, pages, &page_chrome);
    region_ocr::merge_lines(&mut pages, region_lines);
    let text_columns_by_page = pages
        .iter()
        .map(|page| {
            if region_lines.contains_key(&page.page) {
                None
            } else {
                detected_text_columns_for_page(parsed, page, positioned_tables)
            }
        })
        .collect::<Vec<_>>();
    let document_text_columns = (pages.len() == 1)
        .then(|| text_columns_by_page.first().cloned().flatten())
        .flatten();
    let has_lines = !recognized.is_empty()
        || !region_lines.is_empty()
        || pages
            .iter()
            .any(|page| page.lines.iter().any(|line| !line.text.trim().is_empty()));
    if !has_lines && parsed.image_elements.is_empty() && ocr_table.is_none() {
        return Ok(Docx::new()
            .add_paragraph(Paragraph::new().add_run(text_run("(no extractable content)"))));
    }
    let can_restore_multi_page_text_columns = recognized.is_empty()
        && region_lines.is_empty()
        && pages.len() > 1
        && text_columns_by_page.iter().any(Option::is_some)
        && pages.iter().all(|page| !page.lines.is_empty())
        && positioned_tables.is_empty()
        && parsed.image_elements.is_empty()
        && ocr_table.is_none();
    if can_restore_multi_page_text_columns {
        return build_multi_page_text_column_docx(
            parsed,
            &pages,
            positioned_tables,
            &page_chrome,
            &text_columns_by_page,
        );
    }

    let has_mixed_page_sizes = structured_pages_have_mixed_sizes(parsed, &pages);
    let document_layout_page = if has_mixed_page_sizes {
        pages.last()
    } else {
        pages.first()
    };
    let mut docx = document_layout_page
        .map(|page| structured_docx_with_page_layout(parsed, positioned_tables, page, &page_chrome))
        .unwrap_or_else(Docx::new);
    let base_section_property = docx.document.section_property.clone();
    let leading_single_column_section = document_text_columns
        .as_ref()
        .filter(|layout| layout.leading_single_column_line_count > 0)
        .map(|_| {
            let mut property = base_section_property.clone();
            property.columns = 1;
            property.section_type = None;
            property
        });
    let column_section_before_trailing = document_text_columns
        .as_ref()
        .filter(|layout| layout.trailing_single_column_line_count > 0)
        .map(|layout| {
            let mut property = base_section_property.clone();
            property.columns = layout.columns.len();
            property.section_type =
                (layout.leading_single_column_line_count > 0).then_some(SectionType::Continuous);
            property
        });
    if let Some(layout) = document_text_columns.as_ref() {
        let final_column_count = if layout.trailing_single_column_line_count > 0 {
            1
        } else {
            layout.columns.len()
        };
        docx = apply_column_count_to_docx(docx, final_column_count);
        if layout.leading_single_column_line_count > 0
            || layout.trailing_single_column_line_count > 0
        {
            docx.document.section_property.section_type = Some(SectionType::Continuous);
        }
    }
    docx = add_repeated_page_chrome_to_docx(docx, &page_chrome);

    for (page_index, page) in pages.iter().enumerate() {
        if page_index > 0 && !has_mixed_page_sizes {
            docx =
                docx.add_paragraph(Paragraph::new().add_run(Run::new().add_break(BreakType::Page)));
        }
        if has_mixed_page_sizes && page_index + 1 < pages.len() {
            let section =
                structured_section_with_page_layout(parsed, positioned_tables, page, &page_chrome);
            let section = if let Some(lines) = recognized.get(&page.page) {
                page_ocr::add_lines(section, lines)
            } else {
                add_structured_page_blocks(
                    section,
                    parsed,
                    page,
                    positioned_tables,
                    None,
                    StructuredPageSectionRefs::default(),
                    image_payloads,
                )?
            };
            docx = append_section_with_repeated_chrome(docx, section, &page_chrome)?;
        } else {
            let text_column_layout = (pages.len() == 1)
                .then_some(document_text_columns.as_ref())
                .flatten();
            docx = if let Some(lines) = recognized.get(&page.page) {
                page_ocr::add_lines(docx, lines)
            } else {
                add_structured_page_blocks(
                    docx,
                    parsed,
                    page,
                    positioned_tables,
                    text_column_layout,
                    StructuredPageSectionRefs {
                        leading_single_column: leading_single_column_section.as_ref(),
                        column_before_trailing: column_section_before_trailing.as_ref(),
                        page_end: None,
                    },
                    image_payloads,
                )?
            };
        }
    }
    if positioned_tables.is_empty()
        && let Some(table) = ocr_table
    {
        table.validate().map_err(ConvertError::InvalidOcrTable)?;
        docx = docx.add_table(docx_table_from_ocr_table(table));
    }
    Ok(docx)
}

/// structured page block を追加できる DOCX container を表す。
trait StructuredPageContainer: Sized {
    /// container へ段落を追加する。
    fn add_structured_paragraph(self, paragraph: Paragraph) -> Self;
    /// container へ表を追加する。
    fn add_structured_table(self, table: Table) -> Self;
}

impl StructuredPageContainer for Docx {
    fn add_structured_paragraph(self, paragraph: Paragraph) -> Self {
        self.add_paragraph(paragraph)
    }

    fn add_structured_table(self, table: Table) -> Self {
        self.add_table(table)
    }
}

impl StructuredPageContainer for Section {
    fn add_structured_paragraph(self, paragraph: Paragraph) -> Self {
        self.add_paragraph(paragraph)
    }

    fn add_structured_table(self, table: Table) -> Self {
        self.add_table(table)
    }
}

/// PDF 1ページ分の段落・表・画像を指定 container へ追加する。
fn add_structured_page_blocks<C: StructuredPageContainer>(
    mut container: C,
    parsed: &ParsedDocument,
    page: &StructuredTextPage,
    positioned_tables: &[PositionedTable],
    text_column_layout: Option<&DetectedTextColumnLayout>,
    section_refs: StructuredPageSectionRefs<'_>,
    image_payloads: &mut Option<Vec<Vec<u8>>>,
) -> Result<C, ConvertError> {
    let band_edges = column_bands::column_band_edges(parsed, page.page, positioned_tables);
    let band_page_width = parsed
        .pages
        .iter()
        .find(|parsed_page| parsed_page.page_number == page.page)
        .map(|parsed_page| parsed_page_display_size_points(parsed_page).0)
        .unwrap_or(f32::MAX);
    let mut blocks = page
        .lines
        .iter()
        .enumerate()
        .map(|(flow_index, line)| StructuredBlock::Paragraph {
            top_y: line.top_y,
            flow_index,
            line: line.clone(),
        })
        .collect::<Vec<_>>();
    for table in positioned_tables
        .iter()
        .filter(|table| table.page == page.page)
    {
        let top_y = table.bounds.y1;
        blocks.push(StructuredBlock::Table {
            band_rank: column_bands::reading_rank(
                (table.bounds.x0 + table.bounds.x1) / 2.0,
                &band_edges,
                band_page_width,
            ),
            order_index: column_bands::insertion_index_in_band(
                &page.lines,
                top_y,
                (table.bounds.x0 + table.bounds.x1) / 2.0,
                &band_edges,
                band_page_width,
            ),
            top_y,
            table: table.clone(),
        });
    }
    for (image_index, image) in parsed
        .image_elements
        .iter()
        .enumerate()
        .filter(|(_, image)| image.page == page.page)
    {
        let top_y = image.bounds.y1;
        blocks.push(StructuredBlock::Image {
            band_rank: column_bands::reading_rank(
                (image.bounds.x0 + image.bounds.x1) / 2.0,
                &band_edges,
                band_page_width,
            ),
            order_index: column_bands::insertion_index_in_band(
                &page.lines,
                top_y,
                (image.bounds.x0 + image.bounds.x1) / 2.0,
                &band_edges,
                band_page_width,
            ),
            top_y,
            image: image.clone(),
            image_index,
        });
    }
    blocks.sort_by(compare_structured_blocks);
    let section_origin_x = structured_page_left_x(parsed, positioned_tables, page);
    let origin_x = if section_origin_x.is_finite() {
        section_origin_x
    } else {
        0.0
    };
    let page_width = parsed
        .pages
        .iter()
        .find(|parsed_page| parsed_page.page_number == page.page)
        .map(|parsed_page| parsed_page_display_size_points(parsed_page).0);
    let gap_after_block = blocks
        .iter()
        .enumerate()
        .map(|(index, _)| matches!(blocks.get(index + 1), Some(StructuredBlock::Table { .. })))
        .collect::<Vec<_>>();
    let paragraph_after_spacing = blocks
        .iter()
        .enumerate()
        .map(|(index, block)| match block {
            StructuredBlock::Paragraph { line, .. }
                if matches!(
                    blocks.get(index + 1),
                    Some(StructuredBlock::Table { .. } | StructuredBlock::Image { .. })
                ) =>
            {
                blocks
                    .get(index + 1)
                    .and_then(|next| pdf_vertical_blank_gap_from_line_to_y(line, next.top_y()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut previous_line: Option<StructuredTextLine> = None;
    let mut previous_nonparagraph_bottom_y: Option<f32> = None;
    let mut previous_column_index = 0usize;
    for (block_index, block) in blocks.into_iter().enumerate() {
        match block {
            StructuredBlock::Paragraph {
                flow_index,
                mut line,
                ..
            } => {
                let text_columns = text_column_layout
                    .filter(|layout| layout.contains_column_line(flow_index))
                    .map(|layout| layout.columns.as_slice());
                let column_index = text_columns
                    .map(|columns| column_index_for_line(&line, columns))
                    .unwrap_or(0);
                let starts_new_column = column_index > previous_column_index;
                if starts_new_column {
                    previous_line = None;
                    previous_nonparagraph_bottom_y = None;
                }
                previous_column_index = column_index;
                let paragraph_origin_x = text_columns
                    .zip(page_width)
                    .map(|(columns, page_width)| {
                        docx_column_origin_x(page_width, origin_x, column_index, columns.len())
                    })
                    .unwrap_or(origin_x);
                if text_columns.is_some() {
                    line.alignment = ParagraphAlignment::Left;
                }
                let mut paragraph = paragraph_from_structured_line(
                    &line,
                    paragraph_origin_x,
                    page_width,
                    Some(section_origin_x),
                );
                let before = pdf_vertical_blank_gap_twips(previous_line.as_ref(), &line)
                    .or_else(|| {
                        previous_nonparagraph_bottom_y.and_then(|bottom_y| {
                            pdf_vertical_blank_gap_between_y(bottom_y, line.top_y)
                        })
                    })
                    .unwrap_or(0);
                let after = paragraph_after_spacing[block_index].unwrap_or(0);
                if let Some(spacing) =
                    native_line_pitch::spacing(structured_line_max_font_size(&line), before, after)
                {
                    paragraph = paragraph.line_spacing(spacing);
                } else if before > 0 || after > 0 {
                    paragraph =
                        paragraph.line_spacing(LineSpacing::new().before(before).after(after));
                }
                if starts_new_column {
                    paragraph.children.insert(
                        0,
                        ParagraphChild::Run(Box::new(Run::new().add_break(BreakType::Column))),
                    );
                }
                if let Some(section_property) = section_refs.leading_single_column
                    && text_column_layout
                        .is_some_and(|layout| flow_index + 1 == layout.column_start_index())
                {
                    paragraph = paragraph.section_property(section_property.clone());
                }
                if let Some(section_property) = section_refs.column_before_trailing
                    && text_column_layout
                        .is_some_and(|layout| flow_index + 1 == layout.column_end_index())
                {
                    paragraph = paragraph.section_property(section_property.clone());
                }
                if let Some(section_property) = section_refs.page_end
                    && flow_index + 1 == page.lines.len()
                {
                    paragraph = paragraph.section_property(section_property.clone());
                }
                container = container.add_structured_paragraph(paragraph);
                previous_line = Some(line);
                previous_nonparagraph_bottom_y = None;
            }
            StructuredBlock::Table { table, .. } => {
                previous_line = None;
                previous_nonparagraph_bottom_y = Some(table.bounds.y0.min(table.bounds.y1));
                table
                    .table
                    .validate()
                    .map_err(ConvertError::InvalidOcrTable)?;
                container = container
                    .add_structured_table(docx_table_from_structured_table(&table, origin_x));
                if gap_after_block[block_index] {
                    container = container.add_structured_paragraph(table_gap_paragraph());
                }
            }
            StructuredBlock::Image {
                mut image,
                image_index,
                ..
            } => {
                if let Some(payloads) = image_payloads {
                    image.png_bytes = std::mem::take(&mut payloads[image_index]);
                }
                previous_line = None;
                previous_nonparagraph_bottom_y = Some(image.bounds.y0.min(image.bounds.y1));
                container = container.add_structured_paragraph(
                    Paragraph::new().add_run(Run::new().add_image(pic_from_parsed_image(image))),
                );
            }
        }
    }
    Ok(container)
}

/// 複数 PDF ページの1段・2段構成をページ単位の DOCX section として組み立てる。
fn build_multi_page_text_column_docx(
    parsed: &ParsedDocument,
    pages: &[StructuredTextPage],
    positioned_tables: &[PositionedTable],
    page_chrome: &RepeatedPageChrome,
    text_columns_by_page: &[Option<DetectedTextColumnLayout>],
) -> Result<Docx, ConvertError> {
    let last_page = pages
        .last()
        .expect("multi-page text layout must have pages");
    let mut docx = add_repeated_page_chrome_to_docx(
        structured_docx_with_page_layout(parsed, positioned_tables, last_page, page_chrome),
        page_chrome,
    );
    let repeated_chrome_section = docx.document.section_property.clone();
    for (page_index, page) in pages.iter().enumerate() {
        let base_section = inherit_header_footer(
            structured_docx_with_page_layout(parsed, positioned_tables, page, page_chrome)
                .document
                .section_property,
            &repeated_chrome_section,
        );
        let text_column_layout = text_columns_by_page
            .get(page_index)
            .and_then(Option::as_ref);
        let sections =
            text_page_section_properties(&base_section, text_column_layout, page_index > 0);
        let is_last_page = page_index + 1 == pages.len();
        if is_last_page {
            docx.document.section_property = sections.final_section.clone();
        }
        docx = add_structured_page_blocks(
            docx,
            parsed,
            page,
            positioned_tables,
            text_column_layout,
            StructuredPageSectionRefs {
                leading_single_column: sections.leading_single_column.as_ref(),
                column_before_trailing: sections.column_before_trailing.as_ref(),
                page_end: (!is_last_page).then_some(&sections.final_section),
            },
            &mut None,
        )?;
    }
    Ok(docx)
}

/// 反復 header/footer part と参照をページ固有の section property へ継承する。
fn inherit_header_footer(
    mut section: SectionProperty,
    source: &SectionProperty,
) -> SectionProperty {
    section.header_reference = source.header_reference.clone();
    section.header = source.header.clone();
    section.first_header_reference = source.first_header_reference.clone();
    section.first_header = source.first_header.clone();
    section.even_header_reference = source.even_header_reference.clone();
    section.even_header = source.even_header.clone();
    section.footer_reference = source.footer_reference.clone();
    section.footer = source.footer.clone();
    section.first_footer_reference = source.first_footer_reference.clone();
    section.first_footer = source.first_footer.clone();
    section.even_footer_reference = source.even_footer_reference.clone();
    section.even_footer = source.even_footer.clone();
    section
}

/// 1ページ内の段組み区間とページ境界に対応する DOCX section property 群を作る。
fn text_page_section_properties(
    base_section: &SectionProperty,
    text_column_layout: Option<&DetectedTextColumnLayout>,
    starts_on_new_page: bool,
) -> TextPageSectionProperties {
    let page_start_type = starts_on_new_page.then_some(SectionType::NextPage);
    let Some(layout) = text_column_layout else {
        let mut final_section = base_section.clone();
        final_section.columns = 1;
        final_section.section_type = page_start_type;
        return TextPageSectionProperties {
            leading_single_column: None,
            column_before_trailing: None,
            final_section,
        };
    };

    let leading_single_column = (layout.leading_single_column_line_count > 0).then(|| {
        let mut property = base_section.clone();
        property.columns = 1;
        property.section_type = page_start_type;
        property
    });
    let column_before_trailing = (layout.trailing_single_column_line_count > 0).then(|| {
        let mut property = base_section.clone();
        property.columns = layout.columns.len();
        property.section_type = if layout.leading_single_column_line_count > 0 {
            Some(SectionType::Continuous)
        } else {
            page_start_type
        };
        property
    });
    let mut final_section = base_section.clone();
    final_section.columns = if layout.trailing_single_column_line_count > 0 {
        1
    } else {
        layout.columns.len()
    };
    final_section.section_type = if layout.leading_single_column_line_count > 0
        || layout.trailing_single_column_line_count > 0
    {
        Some(SectionType::Continuous)
    } else {
        page_start_type
    };
    TextPageSectionProperties {
        leading_single_column,
        column_before_trailing,
        final_section,
    }
}

/// 単一ページで安全に実段組へ変換できる text column 左端を返す。
fn detected_text_columns_for_page(
    parsed: &ParsedDocument,
    page: &StructuredTextPage,
    positioned_tables: &[PositionedTable],
) -> Option<DetectedTextColumnLayout> {
    if positioned_tables
        .iter()
        .any(|table| table.page == page.page)
        || parsed
            .image_elements
            .iter()
            .any(|image| image.page == page.page)
    {
        return None;
    }
    let page_width = parsed
        .pages
        .iter()
        .find(|parsed_page| parsed_page.page_number == page.page)
        .map(|parsed_page| parsed_page_display_size_points(parsed_page).0)?;
    let columns =
        infer_text_columns(&page.lines, page_width).filter(|columns| columns.len() == 2)?;
    let leading_single_column_line_count = leading_single_column_line_count(&page.lines);
    let trailing_single_column_line_count = trailing_single_column_line_count(&page.lines);
    Some(DetectedTextColumnLayout {
        columns,
        leading_single_column_line_count,
        column_line_count: page
            .lines
            .len()
            .saturating_sub(leading_single_column_line_count + trailing_single_column_line_count),
        trailing_single_column_line_count,
    })
}

/// 均等段組の指定 column が DOCX ページ上で始まる x 座標（pt）を返す。
fn docx_column_origin_x(
    page_width: f32,
    page_origin_x: f32,
    column_index: usize,
    column_count: usize,
) -> f32 {
    if column_count < 2 || !page_width.is_finite() || !page_origin_x.is_finite() {
        return page_origin_x;
    }
    let column_space = DEFAULT_DOCX_COLUMN_SPACE_TWIPS / 20.0;
    let usable_width = (page_width - page_origin_x).max(0.0);
    let total_space = column_space * column_count.saturating_sub(1) as f32;
    let column_width = ((usable_width - total_space) / column_count as f32).max(0.0);
    page_origin_x + column_index.min(column_count - 1) as f32 * (column_width + column_space)
}

/// DOCX 最終 section の均等段組数を設定する。
fn apply_column_count_to_docx(mut docx: Docx, column_count: usize) -> Docx {
    let document = std::mem::take(&mut docx.document).columns(column_count.max(1));
    docx.document = document;
    docx
}

/// 指定 PDF ページの寸法と配置原点を DOCX の最終 section へ反映する。
fn structured_docx_with_page_layout(
    parsed: &ParsedDocument,
    positioned_tables: &[PositionedTable],
    page: &StructuredTextPage,
    chrome: &RepeatedPageChrome,
) -> Docx {
    let Some(((width, height), margin)) =
        structured_page_layout(parsed, positioned_tables, page, chrome)
    else {
        return Docx::new();
    };
    let docx = Docx::new().page_margin(margin).page_size(width, height);
    apply_landscape_orientation_to_docx(docx, width, height)
}

/// 指定 PDF ページの寸法と配置原点を中間 DOCX section へ反映する。
fn structured_section_with_page_layout(
    parsed: &ParsedDocument,
    positioned_tables: &[PositionedTable],
    page: &StructuredTextPage,
    chrome: &RepeatedPageChrome,
) -> Section {
    let Some(((width, height), margin)) =
        structured_page_layout(parsed, positioned_tables, page, chrome)
    else {
        return Section::new();
    };
    let section = Section::new()
        .page_margin(margin)
        .page_size(PageSize::new().size(width, height));
    apply_landscape_orientation_to_section(section, width, height)
}

/// 横長ページへ Word の landscape orientation を明示する。
fn apply_landscape_orientation_to_docx(docx: Docx, width: u32, height: u32) -> Docx {
    if width > height {
        docx.page_orient(PageOrientationType::Landscape)
    } else {
        docx
    }
}

/// 横長の中間 section へ Word の landscape orientation を明示する。
fn apply_landscape_orientation_to_section(section: Section, width: u32, height: u32) -> Section {
    if width > height {
        section.page_orient(PageOrientationType::Landscape)
    } else {
        section
    }
}

/// 見出し・画像・図形も含め、実際のセクション左余白に使う最小座標を返す。
fn structured_page_left_x(
    parsed: &ParsedDocument,
    positioned_tables: &[PositionedTable],
    page: &StructuredTextPage,
) -> f32 {
    positioned_tables
        .iter()
        .filter(|table| table.page == page.page)
        .map(|table| table.bounds.x0)
        .chain(page.lines.iter().map(|line| line.left_x))
        .chain(
            parsed
                .image_elements
                .iter()
                .filter(|image| image.page == page.page && !image.floating)
                .map(|image| image.bounds.x0),
        )
        .chain(
            parsed
                .graphics
                .iter()
                .filter(|graphic| graphic.page == page.page)
                .map(|graphic| graphic.bounds.x0.min(graphic.bounds.x1)),
        )
        .filter(|value| value.is_finite() && *value >= 0.0)
        .fold(f32::INFINITY, f32::min)
}

/// 指定 PDF ページの page size と content 原点由来の margin を返す。
fn structured_page_layout(
    parsed: &ParsedDocument,
    positioned_tables: &[PositionedTable],
    page: &StructuredTextPage,
    chrome: &RepeatedPageChrome,
) -> Option<((u32, u32), PageMargin)> {
    let parsed_page = parsed
        .pages
        .iter()
        .find(|parsed_page| parsed_page.page_number == page.page)?;
    let min_x = structured_page_left_x(parsed, positioned_tables, page);
    let max_y = positioned_tables
        .iter()
        .filter(|table| table.page == page.page)
        .map(|table| table.bounds.y1)
        .chain(page.lines.iter().map(|line| line.top_y))
        .chain(
            parsed
                .image_elements
                .iter()
                .filter(|image| image.page == page.page && !image.floating)
                .map(|image| image.bounds.y1),
        )
        .filter(|value| value.is_finite())
        .fold(f32::NEG_INFINITY, f32::max);
    if !min_x.is_finite() || !max_y.is_finite() {
        return None;
    }
    let left = pdf_points_to_indent_twips(min_x).unwrap_or(0);
    let stroke_adjustment = top_margin_graphic_stroke_adjustment(parsed, page.page);
    let text_bearing_adjustment = rotated_helvetica_top_bearing_adjustment(
        parsed,
        parsed_page,
        positioned_tables,
        page,
        max_y,
    );
    let display_height = parsed_page_display_size_points(parsed_page).1;
    let top = pdf_points_to_floor_twips(
        (display_height - max_y - stroke_adjustment + text_bearing_adjustment).max(0.0),
    )
    .unwrap_or(0);
    let margin = PageMargin::new()
        .top(top)
        .left(left)
        .right(0)
        .bottom(0)
        .header(
            chrome
                .header
                .as_ref()
                .map(|line| line.edge_distance_twips)
                .unwrap_or(0),
        )
        .footer(
            chrome
                .footer
                .as_ref()
                .map(|line| line.edge_distance_twips)
                .unwrap_or(0),
        )
        .gutter(0);
    Some((parsed_page_size_twips(parsed_page)?, margin))
}

/// structured page 間で PDF page size が異なるか判定する。
fn structured_pages_have_mixed_sizes(
    parsed: &ParsedDocument,
    pages: &[StructuredTextPage],
) -> bool {
    let mut sizes = pages.iter().filter_map(|page| {
        parsed
            .pages
            .iter()
            .find(|parsed_page| parsed_page.page_number == page.page)
            .and_then(parsed_page_size_twips)
    });
    let Some(first) = sizes.next() else {
        return false;
    };
    sizes.any(|size| size != first)
}

/// PDF ページ寸法を Word のページサイズ（twips）へ変換する。
fn parsed_page_size_twips(page: &ParsedPage) -> Option<(u32, u32)> {
    let (display_width, display_height) = parsed_page_display_size_points(page);
    let width = u32::try_from(pdf_points_to_twips(display_width)?).ok()?;
    let height = u32::try_from(pdf_points_to_twips(display_height)?).ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

/// PDF page rotation適用後の表示幅と表示高さ（pt）を返す。
fn parsed_page_display_size_points(page: &ParsedPage) -> (f32, f32) {
    if page.rotation.rem_euclid(180) == 90 {
        (page.height, page.width)
    } else {
        (page.width, page.height)
    }
}

/// 上端の細い罫線が raster bbox に含まれる分を DOCX 上余白から差し引く。
fn top_margin_graphic_stroke_adjustment(parsed: &ParsedDocument, page: usize) -> f32 {
    parsed
        .graphics
        .iter()
        .filter(|graphic| graphic.page == page)
        .map(|graphic| graphic.stroke_width)
        .filter(|width| width.is_finite() && *width > 0.0 && *width < 0.75)
        .fold(0.0f32, f32::max)
        * 2.0
}

/// 90/270度回転ページで Helvetica を Arial 代替した際の上端 bearing 差を返す。
///
/// LibreOffice は Arial を Liberation Sans で描画するため、回転後の最上段が
/// Helvetica 本文だけのページでは PDF より 1pt 上に寄る。この既知差だけを補正し、
/// 表や画像がページ上端を決める場合には余白を変えない。
fn rotated_helvetica_top_bearing_adjustment(
    parsed: &ParsedDocument,
    parsed_page: &ParsedPage,
    positioned_tables: &[PositionedTable],
    page: &StructuredTextPage,
    max_y: f32,
) -> f32 {
    if parsed_page.rotation.rem_euclid(180) != 90
        || positioned_tables
            .iter()
            .any(|table| table.page == page.page)
        || parsed
            .image_elements
            .iter()
            .any(|image| image.page == page.page && !image.floating)
    {
        return 0.0;
    }
    let top_lines = page
        .lines
        .iter()
        .filter(|line| (line.top_y - max_y).abs() < 0.01)
        .collect::<Vec<_>>();
    let has_helvetica_run = top_lines.iter().flat_map(|line| &line.runs).any(|run| {
        run.style
            .font_name
            .as_deref()
            .is_some_and(pdf_font_name_is_helvetica)
    });
    let every_run_is_helvetica = top_lines.iter().flat_map(|line| &line.runs).all(|run| {
        run.style
            .font_name
            .as_deref()
            .is_some_and(pdf_font_name_is_helvetica)
    });
    if has_helvetica_run && every_run_is_helvetica {
        1.0
    } else {
        0.0
    }
}

/// PDF font 名が Helvetica family を表すか判定する。
fn pdf_font_name_is_helvetica(font_name: &str) -> bool {
    font_name
        .split_once('+')
        .map(|(_, name)| name)
        .unwrap_or(font_name)
        .trim()
        .to_ascii_lowercase()
        .starts_with("helvetica")
}

/// PDF point 座標を切り捨てで DOCX twips に変換する。
fn pdf_points_to_floor_twips(points: f32) -> Option<i32> {
    if !points.is_finite() || points < 0.0 {
        return None;
    }
    Some((points * 20.0).floor() as i32)
}

/// parser の画像要素から DOCX picture を構築する。
fn pic_from_parsed_image(mut image: ParsedImageElement) -> Pic {
    let mut pic = Pic::new_with_dimensions(
        std::mem::take(&mut image.png_bytes),
        image.width_px,
        image.height_px,
    );
    if image.floating || image.rotation_degrees != 0 {
        pic = pic
            .floating()
            .offset_x(pdf_points_to_emu(image.bounds.x0).unwrap_or(0))
            .offset_y(pdf_points_to_emu(image.bounds.y0).unwrap_or(0))
            .overlapping();
        if image.rotation_degrees != 0 {
            pic = pic.rotate(image.rotation_degrees);
        }
    }
    pic
}

/// PDF point 座標を EMU に変換する。
fn pdf_points_to_emu(points: f32) -> Option<i32> {
    if !points.is_finite() {
        return None;
    }
    Some((points * 12_700.0).round() as i32)
}

/// PDF point 座標を DOCX twips に変換する。
fn pdf_points_to_twips(points: f32) -> Option<usize> {
    if !points.is_finite() || points < 0.0 {
        return None;
    }
    Some((points * 20.0).round() as usize)
}

/// PDF point 座標を DOCX twips に変換する。
fn pdf_points_to_twips_f32(points: f32) -> Option<f32> {
    if !points.is_finite() || points < 0.0 {
        return None;
    }
    Some((points * 20.0).round())
}

/// PDF point 座標を DOCX paragraph indent 用 twips に変換する。
fn pdf_points_to_indent_twips(points: f32) -> Option<i32> {
    if !points.is_finite() || points < 0.0 {
        return None;
    }
    Some((points * 20.0).round() as i32)
}

/// PDF text element を page ごとの表示行へ復元する。
fn text_lines_by_page_from_pdf_text_elements(
    parsed: &ParsedDocument,
    positioned_tables: &[PositionedTable],
) -> Vec<StructuredTextPage> {
    let page_numbers = parsed
        .text_elements
        .iter()
        .map(|text| text.page)
        .chain(positioned_tables.iter().map(|table| table.page))
        .chain(parsed.image_elements.iter().map(|image| image.page))
        .collect::<BTreeSet<_>>();

    page_numbers
        .into_iter()
        .filter_map(|page| {
            let mut texts = parsed
                .text_elements
                .iter()
                .filter(|text| {
                    text.page == page
                        && !text.text.trim().is_empty()
                        && !positioned_tables
                            .iter()
                            .any(|table| text_element_is_in_table(parsed, text, table))
                })
                .collect::<Vec<_>>();
            if texts.is_empty()
                && !positioned_tables.iter().any(|table| table.page == page)
                && !parsed.image_elements.iter().any(|image| image.page == page)
            {
                return None;
            }
            texts.sort_by(|a, b| {
                native_baseline::line_y(b)
                    .total_cmp(&native_baseline::line_y(a))
                    .then(a.bounds.x0.total_cmp(&b.bounds.x0))
            });
            let page_width = page_width_for_layout(parsed, page, &texts);
            let page_graphics = parsed
                .graphics
                .iter()
                .filter(|graphic| graphic.page == page)
                .cloned()
                .collect::<Vec<_>>();
            // 縦罫線で段を区切るページは、段をまたいで行をつながない。
            let band_edges = column_bands::column_band_edges(parsed, page, positioned_tables);
            let partial_rules = column_bands::partial_band_rules(parsed, page, positioned_tables);
            let text_band = |text: &ParsedTextElement| {
                let x = (text.bounds.x0 + text.bounds.x1) / 2.0;
                let y = (text.bounds.y0 + text.bounds.y1) / 2.0;
                (
                    column_bands::band_of(x, &band_edges),
                    column_bands::partial_band_of(x, y, &partial_rules),
                )
            };
            let mut rows: Vec<Vec<&ParsedTextElement>> = Vec::new();
            for text in texts {
                if let Some(row) = rows.iter_mut().find(|row| {
                    (native_baseline::line_y(row[0]) - native_baseline::line_y(text)).abs()
                        <= TEXT_ROW_Y_TOLERANCE_PT
                        && text_band(row[0]) == text_band(text)
                }) {
                    row.push(text);
                } else {
                    rows.push(vec![text]);
                }
            }
            let lines = rows
                .iter_mut()
                .flat_map(|row| {
                    *row = native_spacing::with_interior_spaces(
                        &parsed.text_elements,
                        std::mem::take(row),
                    );
                    row.sort_by(|a, b| a.bounds.x0.total_cmp(&b.bounds.x0));
                    split_text_row_segments(row)
                        .into_iter()
                        .filter_map(|segment| {
                            structured_text_line_from_segment(&segment, &page_graphics, page_width)
                                .map(|mut line| {
                                    mark_embedded_fonts_in_line(&mut line, &parsed.embedded_fonts);
                                    line
                                })
                        })
                })
                .filter(|line| !is_pdf_page_number_line(parsed, page, line))
                .collect::<Vec<_>>();
            // 縦罫線で段を区切るページは、左の段から順に読む。
            let lines = if !band_edges.is_empty() {
                column_bands::merge_margin_band_lines(
                    column_bands::order_lines_by_band(lines, &band_edges, page_width),
                    &band_edges,
                    page_width,
                    parsed
                        .pages
                        .iter()
                        .find(|parsed_page| parsed_page.page_number == page)
                        .is_some_and(|parsed_page| parsed_page.rotation.rem_euclid(180) == 90),
                )
            } else if positioned_tables.iter().any(|table| table.page == page) {
                lines
            } else {
                order_text_lines_for_layout(parsed, page, lines)
            };
            (!lines.is_empty()
                || positioned_tables.iter().any(|table| table.page == page)
                || parsed.image_elements.iter().any(|image| image.page == page))
            .then_some(StructuredTextPage { page, lines })
        })
        .collect()
}

/// page width を layout 推定に使える値として取得する。
fn page_width_for_layout(
    parsed: &ParsedDocument,
    page: usize,
    texts: &[&ParsedTextElement],
) -> f32 {
    parsed
        .pages
        .iter()
        .find(|parsed_page| parsed_page.page_number == page)
        .map(|page| parsed_page_display_size_points(page).0)
        .unwrap_or_else(|| {
            texts
                .iter()
                .map(|text| text.bounds.x1)
                .fold(f32::NEG_INFINITY, f32::max)
        })
}

/// text element が推定表の矩形内にあるか判定する。
fn text_element_is_in_table(
    parsed: &ParsedDocument,
    text: &ParsedTextElement,
    table: &PositionedTable,
) -> bool {
    if text.page != table.page {
        return false;
    }
    // 横並びの表をまとめた外側の表は、メンバーの表の範囲だけを表内とする
    // （外接矩形にかかる表の外の本文を落とさない）。
    if is_side_by_side_container(table) {
        return table
            .nested_tables
            .iter()
            .any(|member| text_element_is_in_table(parsed, text, member));
    }
    let center_x = (text.bounds.x0 + text.bounds.x1) / 2.0;
    let center_y = (text.bounds.y0 + text.bounds.y1) / 2.0;
    if text_center_is_in_table_bounds(center_x, center_y, table) {
        return true;
    }
    // 上端原点の y で置いた文字片の救済は、表のセルにその文字列があるときだけ使う
    // （回転したページなどで、上下を反転した位置が別の表に重なる本文を落とさない）。
    table_accepts_top_origin_text_y(parsed, table)
        && page_height(parsed, text.page)
            .map(|height| text_center_is_in_table_bounds(center_x, height - center_y, table))
            .unwrap_or(false)
        && table_cells_contain_text(table, &text.text)
}

/// 表のいずれかのセルが、文字片の文字列（前後の空白を除く）を含むか判定する。
fn table_cells_contain_text(table: &PositionedTable, text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && table
            .table
            .rows
            .iter()
            .flatten()
            .any(|cell| cell.contains(text))
}

/// text center が推定表の矩形内に入るか判定する。
fn text_center_is_in_table_bounds(center_x: f32, center_y: f32, table: &PositionedTable) -> bool {
    center_x >= table.bounds.x0 - 1.0
        && center_x <= table.bounds.x1 + 1.0
        && center_y >= table.bounds.y0 - 1.0
        && center_y <= table.bounds.y1 + 1.0
}

/// 指定 page の高さを返す。
fn page_height(parsed: &ParsedDocument, page: usize) -> Option<f32> {
    parsed
        .pages
        .iter()
        .find(|parsed_page| parsed_page.page_number == page)
        .map(|page| parsed_page_display_size_points(page).1)
}

/// 1 行分の text element を、段組などの大きな水平ギャップで分割する。
fn split_text_row_segments<'a>(row: &[&'a ParsedTextElement]) -> Vec<TextRowSegment<'a>> {
    let Some(first) = row.first() else {
        return Vec::new();
    };
    let avg_height = row
        .iter()
        .map(|text| (text.bounds.y1 - text.bounds.y0).abs().max(1.0))
        .sum::<f32>()
        / row.len() as f32;
    let split_gap = avg_height * 8.0;
    let mut segments: Vec<Vec<&ParsedTextElement>> = vec![vec![*first]];
    let mut right = first.bounds.x1;
    for text in row.iter().copied().skip(1) {
        let gap = text.bounds.x0 - right;
        if gap > split_gap {
            segments.push(vec![text]);
        } else if let Some(segment) = segments.last_mut() {
            segment.push(text);
        }
        right = right.max(text.bounds.x1);
    }
    let split_from_row = segments.len() > 1;
    segments
        .into_iter()
        .map(|texts| TextRowSegment {
            texts,
            split_from_row,
        })
        .collect()
}

/// text row segment を DOCX 出力用の structured line に変換する。
fn structured_text_line_from_segment(
    segment: &TextRowSegment<'_>,
    page_graphics: &[ParsedGraphicElement],
    page_width: f32,
) -> Option<StructuredTextLine> {
    let runs = structured_text_runs_from_row(&segment.texts, page_graphics);
    if runs.is_empty() {
        return None;
    }
    let text = runs.iter().map(|run| run.text.as_str()).collect::<String>();
    let left_x = segment
        .texts
        .iter()
        .map(|text| text.bounds.x0)
        .fold(f32::INFINITY, f32::min);
    let right_x = segment
        .texts
        .iter()
        .map(|text| text.bounds.x1)
        .fold(f32::NEG_INFINITY, f32::max);
    Some(StructuredTextLine {
        top_y: segment
            .texts
            .iter()
            .map(|text| text.bounds.y1)
            .fold(f32::NEG_INFINITY, f32::max),
        left_x,
        right_x,
        alignment: infer_paragraph_alignment(left_x, right_x, page_width),
        runs,
        split_from_row: segment.split_from_row,
        text,
    })
}

/// PDF フッター由来のページ番号だけの行か判定する。
fn is_pdf_page_number_line(
    parsed: &ParsedDocument,
    page: usize,
    line: &StructuredTextLine,
) -> bool {
    let Some(height) = page_height(parsed, page) else {
        return false;
    };
    let footer_limit = height * 0.12;
    line.top_y <= footer_limit && is_page_number_text(&line.text)
}

/// ページ番号として無視してよい文字列か判定する。
fn is_page_number_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalized = trimmed
        .trim_matches(|ch: char| ch == '-' || ch == '－' || ch.is_whitespace())
        .trim();
    !normalized.is_empty()
        && normalized
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '０'..='９'))
}

/// PDF の水平位置から DOCX 段落配置を推定する。
fn infer_paragraph_alignment(left_x: f32, right_x: f32, page_width: f32) -> ParagraphAlignment {
    if !left_x.is_finite() || !right_x.is_finite() || !page_width.is_finite() || page_width <= 0.0 {
        return ParagraphAlignment::Left;
    }
    let line_width = (right_x - left_x).max(0.0);
    let line_center = (left_x + right_x) / 2.0;
    let page_center = page_width / 2.0;
    let center_tolerance = (page_width * 0.08).max(18.0);
    if line_width <= page_width * 0.75 && (line_center - page_center).abs() <= center_tolerance {
        return ParagraphAlignment::Center;
    }
    let right_margin = page_width - right_x;
    let right_tolerance = (page_width * 0.12).max(24.0);
    if left_x >= page_width * 0.55 && right_margin <= right_tolerance {
        return ParagraphAlignment::Right;
    }
    ParagraphAlignment::Left
}

/// 本文の相対原点と実セクション余白を区別して配置情報つき段落を作る。
fn paragraph_from_structured_line(
    line: &StructuredTextLine,
    origin_x: f32,
    page_width: Option<f32>,
    section_origin_x: Option<f32>,
) -> Paragraph {
    let paragraph = line.runs.iter().fold(Paragraph::new(), |paragraph, run| {
        paragraph.add_run(docx_run_from_structured_run(run))
    });
    match line.alignment {
        ParagraphAlignment::Center => {
            let paragraph = paragraph.align(AlignmentType::Center);
            if let Some((left, right)) = page_width.and_then(|width| {
                paragraph_anchor::centred_indents(
                    line.left_x,
                    line.right_x,
                    section_origin_x.unwrap_or(origin_x),
                    width,
                )
            }) {
                return paragraph.indent(Some(left), None, Some(right), None);
            }
            if let Some(indent) = pdf_points_to_indent_twips(origin_x)
                && indent > 0
            {
                return paragraph.indent(None, None, Some(indent), None);
            }
            paragraph
        }
        ParagraphAlignment::Right => {
            let paragraph = paragraph.align(AlignmentType::Right);
            if let Some(right_margin) = page_width.map(|width| (width - line.right_x).max(0.0))
                && let Some(indent) = pdf_points_to_indent_twips(right_margin)
                && indent > 0
            {
                return paragraph.indent(None, None, Some(indent), None);
            }
            paragraph
        }
        ParagraphAlignment::Left => {
            if let Some(indent) = pdf_points_to_indent_twips((line.left_x - origin_x).max(0.0))
                && indent > 0
            {
                return paragraph.indent(Some(indent), None, None, None);
            }
            paragraph
        }
    }
}

/// 連続する PDF 本文行の間にある空白を DOCX paragraph spacing へ変換する。
fn pdf_vertical_blank_gap_twips(
    previous: Option<&StructuredTextLine>,
    current: &StructuredTextLine,
) -> Option<u32> {
    let previous = previous?;
    pdf_vertical_blank_gap_from_line_to_y(previous, current.top_y)
}

/// PDF 本文行の下端から次 block 上端までの空白を twips へ変換する。
fn pdf_vertical_blank_gap_from_line_to_y(
    line: &StructuredTextLine,
    next_top_y: f32,
) -> Option<u32> {
    let line_bottom = line.top_y - structured_line_max_font_size(line);
    pdf_vertical_blank_gap_between_y(line_bottom, next_top_y)
}

/// 先行 block 下端と次 block 上端の PDF 座標差を twips へ変換する。
fn pdf_vertical_blank_gap_between_y(previous_bottom_y: f32, next_top_y: f32) -> Option<u32> {
    let gap_points = previous_bottom_y - next_top_y;
    u32::try_from(pdf_points_to_floor_twips(gap_points)?).ok()
}

/// PDF text row から style 情報つき structured run を作る。
/// 行の両端だけをtrimし、描画命令の境界にある明示的な空白を保持する。
fn structured_text_runs_from_row(
    row: &[&ParsedTextElement],
    page_graphics: &[ParsedGraphicElement],
) -> Vec<StructuredTextRun> {
    let Some(first) = row.first() else {
        return Vec::new();
    };
    let mut runs = Vec::new();
    let mut right = first.bounds.x1;
    let mut height_sum = (first.bounds.y1 - first.bounds.y0).abs().max(1.0);
    for (index, text) in row.iter().enumerate() {
        let mut value = row_fragment_text(&text.text, index, row.len()).to_string();
        if value.is_empty() {
            continue;
        }
        if index > 0 {
            let gap = text.bounds.x0 - right;
            let avg_height = height_sum / index as f32;
            if gap > avg_height * 0.8
                && !value.starts_with(char::is_whitespace)
                && !runs
                    .last()
                    .is_some_and(|run: &StructuredTextRun| run.text.ends_with(char::is_whitespace))
            {
                value.insert(0, ' ');
            }
        }
        runs.push(StructuredTextRun {
            text: value,
            style: StructuredTextStyle {
                font_size: text.font_size,
                font_name: text.font_name.clone(),
                font_embedded: false,
                fill_color: text.fill_color.clone(),
                underlined: parsed_text_has_underline(text, page_graphics),
            },
        });
        right = right.max(text.bounds.x1);
        height_sum += (text.bounds.y1 - text.bounds.y0).abs().max(1.0);
    }
    runs
}

/// 行の外側の空白だけを除き、断片間の空白は原文通り返す。
fn row_fragment_text(text: &str, index: usize, len: usize) -> &str {
    let text = if index == 0 { text.trim_start() } else { text };
    if index + 1 == len {
        text.trim_end()
    } else {
        text
    }
}

/// structured line の各runへPDF埋め込みfont利用可否を付与する。
fn mark_embedded_fonts_in_line(
    line: &mut StructuredTextLine,
    embedded_fonts: &[ParsedEmbeddedFont],
) {
    for run in &mut line.runs {
        run.style.font_embedded = run.style.font_name.as_deref().is_some_and(|font_name| {
            embedded_fonts
                .iter()
                .any(|font| font.font_name == font_name)
        });
    }
}

/// structured run から DOCX run を作る。
fn docx_run_from_structured_run(structured_run: &StructuredTextRun) -> Run {
    let mut run = text_run(structured_run.text.as_str());
    if let Some(size) = pdf_font_size_to_half_points(structured_run.style.font_size) {
        run = run.size(size);
    }
    if let Some(font_name) = structured_run.style.font_name.as_deref()
        && !font_name.trim().is_empty()
    {
        run = apply_pdf_font_name_to_run(run, font_name, structured_run.style.font_embedded);
    }
    if let Some(color) = structured_run.style.fill_color.as_deref()
        && is_rgb_hex_color(color)
    {
        run = run.color(color);
    }
    if structured_run.style.underlined {
        run = run.underline("single");
    }
    run
}

/// PDF font 名とそこから推定できる太字・斜体を DOCX run へ反映する。
fn apply_pdf_font_name_to_run(mut run: Run, font_name: &str, embedded: bool) -> Run {
    run = run.fonts(if embedded {
        run_fonts(font_name)
    } else {
        run_fonts_from_pdf_font_name(font_name)
    });
    let normalized = font_name.to_ascii_lowercase();
    if normalized.contains("bold") || normalized.contains("black") {
        run = run.bold();
    }
    if normalized.contains("italic") || normalized.contains("oblique") {
        run = run.italic();
    }
    run
}

/// PDF font size（pt）を DOCX の half-points に変換する。
fn pdf_font_size_to_half_points(font_size: f32) -> Option<usize> {
    if !font_size.is_finite() || font_size <= 0.0 {
        return None;
    }
    Some((font_size * 2.0).round().max(1.0) as usize)
}

/// PDF BaseFont 名から DOCX run fonts を作る。
fn run_fonts_from_pdf_font_name(font_name: &str) -> RunFonts {
    let font_name = docx_font_name_from_pdf_font_name(font_name);
    run_fonts(font_name.as_str())
}

/// 解決済みfont名からDOCX run fontsを作る。
fn run_fonts(font_name: &str) -> RunFonts {
    RunFonts::new()
        .ascii(font_name)
        .hi_ansi(font_name)
        .east_asia(font_name)
        .cs(font_name)
}

/// PDF の subset/PostScript フォント名を DOCX renderer が解決しやすい名前へ正規化する。
fn docx_font_name_from_pdf_font_name(font_name: &str) -> String {
    let stripped = font_name
        .split_once('+')
        .map(|(_, name)| name)
        .unwrap_or(font_name)
        .trim();
    let lower = stripped.to_ascii_lowercase();
    if pdf_font_name_is_mincho(&lower) {
        return "Hiragino Mincho ProN".to_string();
    }
    if pdf_font_name_is_ms_pgothic(&lower) {
        return "Arial Unicode MS".to_string();
    }
    if lower.contains("hirakaku")
        || lower.contains("hiragino")
        || lower.contains("kozgo")
        || lower.contains("yugothic")
        || lower.contains("gothic")
    {
        return "Noto Sans CJK JP".to_string();
    }
    if lower.starts_with("helvetica") {
        return "Arial".to_string();
    }
    if lower == "times-roman"
        || lower.starts_with("times-bold")
        || lower.starts_with("times-italic")
    {
        return "Times New Roman".to_string();
    }
    if lower.starts_with("courier") {
        return "Courier New".to_string();
    }
    match stripped {
        "ArialMT" => "Arial".to_string(),
        "Arial-BoldMT" => "Arial".to_string(),
        "TimesNewRomanPSMT" => "Times New Roman".to_string(),
        "TimesNewRomanPS-BoldMT" => "Times New Roman".to_string(),
        "TimesNewRomanPS-ItalicMT" => "Times New Roman".to_string(),
        "TimesNewRomanPS-BoldItalicMT" => "Times New Roman".to_string(),
        _ => stripped.to_string(),
    }
}

/// PDF font 名が和文明朝系を表すか判定する。
fn pdf_font_name_is_mincho(font_name: &str) -> bool {
    let lower = font_name.to_ascii_lowercase();
    lower.contains("hiramin")
        || lower.contains("kozmin")
        || lower.contains("yumin")
        || lower.contains("mincho")
}

/// PDF font 名が MS PGothic を表すか判定する。
fn pdf_font_name_is_ms_pgothic(font_name: &str) -> bool {
    let lower = font_name.to_ascii_lowercase();
    lower.contains("ms-pgothic") || lower.contains("mspgothic")
}

/// 代替フォントの字幅差を補う DOCX 横scale（percent）を返す。
fn docx_font_stretch_from_pdf_font_name(font_name: &str) -> Option<i32> {
    let stripped = font_name
        .split_once('+')
        .map(|(_, name)| name)
        .unwrap_or(font_name)
        .trim();
    pdf_font_name_is_ms_pgothic(stripped).then_some(92)
}

/// 表内で共通する解決可能な PDF font 名を返す。
fn common_usable_pdf_font_name<'a>(
    font_names: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let mut common = None::<(String, String)>;
    for font_name in font_names {
        let stripped = font_name.trim().trim_start_matches('/');
        if stripped.is_empty()
            || (stripped.len() > 1
                && stripped.starts_with('F')
                && stripped[1..].chars().all(|ch| ch.is_ascii_digit()))
        {
            continue;
        }
        let normalized = docx_font_name_from_pdf_font_name(stripped);
        match common.as_ref() {
            Some((_, current)) if current != &normalized => return None,
            Some(_) => {}
            None => common = Some((font_name.to_string(), normalized)),
        }
    }
    common.map(|(font_name, _)| font_name)
}

/// 表内で共通する妥当な PDF font size（pt）を返す。
fn common_usable_pdf_font_size(font_sizes: impl IntoIterator<Item = f32>) -> Option<f32> {
    let sizes = font_sizes
        .into_iter()
        .filter(|size| size.is_finite() && (4.0..=96.0).contains(size))
        .collect::<Vec<_>>();
    let first = *sizes.first()?;
    if sizes.iter().any(|size| (size - first).abs() > 0.25) {
        return None;
    }
    Some(sizes.iter().sum::<f32>() / sizes.len() as f32)
}

/// RGB hex 色として扱えるかを判定する。
fn is_rgb_hex_color(color: &str) -> bool {
    color.len() == 6 && color.chars().all(|ch| ch.is_ascii_hexdigit())
}

/// ページ内の行を通常 flow または段組 flow の順に並べる。
fn order_text_lines_for_layout(
    parsed: &ParsedDocument,
    page: usize,
    lines: Vec<StructuredTextLine>,
) -> Vec<StructuredTextLine> {
    if lines.is_empty() {
        return lines;
    }
    let split_count = lines.iter().filter(|line| line.split_from_row).count();
    let Some((column_top_y, column_bottom_y)) = text_column_vertical_bounds(&lines) else {
        return lines;
    };
    let full_width_lines_are_outside_columns = lines
        .iter()
        .filter(|line| !line.split_from_row)
        .all(|line| {
            line.top_y > column_top_y + TEXT_ROW_Y_TOLERANCE_PT
                || line.top_y < column_bottom_y - TEXT_ROW_Y_TOLERANCE_PT
        });
    if split_count < 4
        || (split_count * 5 < lines.len() * 4 && !full_width_lines_are_outside_columns)
    {
        return lines;
    }
    let page_width = parsed
        .pages
        .iter()
        .find(|parsed_page| parsed_page.page_number == page)
        .map(|page| parsed_page_display_size_points(page).0)
        .unwrap_or_else(|| {
            lines
                .iter()
                .map(|line| line.right_x)
                .fold(f32::NEG_INFINITY, f32::max)
        });
    let Some(columns) = infer_text_columns(&lines, page_width) else {
        return lines;
    };
    let mut leading_lines = Vec::new();
    let mut column_lines = Vec::new();
    let mut trailing_lines = Vec::new();
    for line in lines {
        if !line.split_from_row && line.top_y > column_top_y + TEXT_ROW_Y_TOLERANCE_PT {
            leading_lines.push(line);
        } else if !line.split_from_row && line.top_y < column_bottom_y - TEXT_ROW_Y_TOLERANCE_PT {
            trailing_lines.push(line);
        } else {
            column_lines.push(line);
        }
    }
    leading_lines.sort_by(|a, b| {
        b.top_y
            .total_cmp(&a.top_y)
            .then(a.left_x.total_cmp(&b.left_x))
    });
    column_lines.sort_by(|a, b| {
        column_index_for_line(a, &columns)
            .cmp(&column_index_for_line(b, &columns))
            .then(b.top_y.total_cmp(&a.top_y))
            .then(a.left_x.total_cmp(&b.left_x))
    });
    trailing_lines.sort_by(|a, b| {
        b.top_y
            .total_cmp(&a.top_y)
            .then(a.left_x.total_cmp(&b.left_x))
    });
    leading_lines.extend(column_lines);
    leading_lines.extend(trailing_lines);
    leading_lines
}

/// 分割 row 由来の段組本文が占める上端・下端 y 座標を返す。
fn text_column_vertical_bounds(lines: &[StructuredTextLine]) -> Option<(f32, f32)> {
    let mut column_y = lines
        .iter()
        .filter(|line| line.split_from_row)
        .map(|line| line.top_y);
    let first = column_y.next()?;
    Some(column_y.fold((first, first), |(top, bottom), y| {
        (top.max(y), bottom.min(y))
    }))
}

/// 段組本文より上にある1段行の数を返す。
fn leading_single_column_line_count(lines: &[StructuredTextLine]) -> usize {
    let Some((column_top_y, _)) = text_column_vertical_bounds(lines) else {
        return 0;
    };
    lines
        .iter()
        .take_while(|line| {
            !line.split_from_row && line.top_y > column_top_y + TEXT_ROW_Y_TOLERANCE_PT
        })
        .count()
}

/// 段組本文より下にある1段行の数を返す。
fn trailing_single_column_line_count(lines: &[StructuredTextLine]) -> usize {
    let Some((_, column_bottom_y)) = text_column_vertical_bounds(lines) else {
        return 0;
    };
    lines
        .iter()
        .rev()
        .take_while(|line| {
            !line.split_from_row && line.top_y < column_bottom_y - TEXT_ROW_Y_TOLERANCE_PT
        })
        .count()
}

/// 段組とみなせる column 左端を推定する。
fn infer_text_columns(lines: &[StructuredTextLine], page_width: f32) -> Option<Vec<f32>> {
    if lines.len() < 4 {
        return None;
    }
    let mut lefts = lines
        .iter()
        .filter(|line| line.split_from_row)
        .map(|line| line.left_x)
        .collect::<Vec<_>>();
    lefts.sort_by(f32::total_cmp);
    let cluster_gap = (page_width * 0.2).max(48.0);
    let mut clusters: Vec<Vec<f32>> = Vec::new();
    for left in lefts {
        if let Some(cluster) = clusters
            .iter_mut()
            .find(|cluster| (cluster[0] - left).abs() <= cluster_gap)
        {
            cluster.push(left);
        } else {
            clusters.push(vec![left]);
        }
    }
    let columns = clusters
        .iter()
        .filter(|cluster| cluster.len() >= 2)
        .map(|cluster| cluster.iter().sum::<f32>() / cluster.len() as f32)
        .collect::<Vec<_>>();
    (columns.len() >= 2).then_some(columns)
}

/// line が属する column index を返す。
fn column_index_for_line(line: &StructuredTextLine, columns: &[f32]) -> usize {
    columns
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (line.left_x - **a)
                .abs()
                .total_cmp(&(line.left_x - **b).abs())
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}

/// 大きな水平ギャップで分割した text row segment。
struct TextRowSegment<'a> {
    /// segment 内の text element。
    texts: Vec<&'a ParsedTextElement>,
    /// 元 row が複数 segment に分割されたか。
    split_from_row: bool,
}

/// structured DOCX へ出力する page 単位のテキスト行。
#[derive(Debug, Clone, PartialEq)]
struct StructuredTextPage {
    /// PDF page 番号。
    page: usize,
    /// page 内の上から下へ並ぶ段落行。
    lines: Vec<StructuredTextLine>,
}

/// 単一ページ内で検出した段組本文と、その前後にある全幅行を表す。
#[derive(Debug, Clone, PartialEq)]
struct DetectedTextColumnLayout {
    /// 段組本文の各 column 左端（pt）。
    columns: Vec<f32>,
    /// 段組本文より前に置く1段行の数。
    leading_single_column_line_count: usize,
    /// 段組本文として出力する行数。
    column_line_count: usize,
    /// 段組本文より後に置く1段行の数。
    trailing_single_column_line_count: usize,
}

/// 1 PDF ページの開始・段組み切り替え・終了に使う DOCX section property 群。
struct TextPageSectionProperties {
    /// 段組み本文より前の全幅 section property。
    leading_single_column: Option<SectionProperty>,
    /// 末尾の全幅注記より前に閉じる段組み section property。
    column_before_trailing: Option<SectionProperty>,
    /// ページ末尾の section property。
    final_section: SectionProperty,
}

/// structured page の段落へ付与する section property 参照群。
#[derive(Default)]
struct StructuredPageSectionRefs<'a> {
    /// 段組み本文より前の全幅 section property。
    leading_single_column: Option<&'a SectionProperty>,
    /// 末尾の全幅注記より前に閉じる段組み section property。
    column_before_trailing: Option<&'a SectionProperty>,
    /// 非最終 PDF ページの末尾へ付与する section property。
    page_end: Option<&'a SectionProperty>,
}

impl DetectedTextColumnLayout {
    /// 段組本文が始まる flow index を返す。
    fn column_start_index(&self) -> usize {
        self.leading_single_column_line_count
    }

    /// 段組本文が終わる排他的 flow index を返す。
    fn column_end_index(&self) -> usize {
        self.column_start_index() + self.column_line_count
    }

    /// 指定した flow index が段組本文に含まれるか返す。
    fn contains_column_line(&self, flow_index: usize) -> bool {
        self.column_start_index() <= flow_index && flow_index < self.column_end_index()
    }
}

/// structured DOCX へ出力する 1 行。
#[derive(Debug, Clone, PartialEq)]
struct StructuredTextLine {
    /// 行上端の PDF y 座標。
    top_y: f32,
    /// 行左端の PDF x 座標。
    left_x: f32,
    /// 行右端の PDF x 座標。
    right_x: f32,
    /// PDF 水平位置から推定した段落配置。
    alignment: ParagraphAlignment,
    /// PDF text element ごとの DOCX run。
    runs: Vec<StructuredTextRun>,
    /// 大きな水平ギャップで分割された row に由来するか。
    split_from_row: bool,
    /// 行テキスト。
    text: String,
}

/// DOCX page edge からの距離を伴う反復 header/footer 行。
#[derive(Debug, Clone, PartialEq)]
struct RepeatedPageChromeLine {
    /// PDF 由来の配置・style 付き行。
    line: StructuredTextLine,
    /// 対応する page edge からの距離（twips）。
    edge_distance_twips: i32,
    /// PDF 行右端から page 右端までの距離（twips）。
    right_indent_twips: i32,
}

/// PDF page の反復 header/footer 候補。
#[derive(Debug, Clone, PartialEq, Default)]
struct RepeatedPageChrome {
    /// DOCX header へ移す配置・style 付き反復行。
    header: Option<RepeatedPageChromeLine>,
    /// DOCX footer へ移す配置・style 付き反復行。
    footer: Option<RepeatedPageChromeLine>,
}

/// 全 page の boundary に反復する PDF テキストを header/footer 候補として抽出する。
fn extract_repeated_page_chrome(
    parsed: &ParsedDocument,
    pages: &[StructuredTextPage],
) -> RepeatedPageChrome {
    if pages.len() < 2 {
        return RepeatedPageChrome::default();
    }
    let page_count = pages.len();
    let mut header_counts: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let mut footer_counts: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for page in pages {
        for line in &page.lines {
            let text = line.text.trim();
            if text.is_empty() {
                continue;
            }
            if is_pdf_header_line(parsed, page.page, line) {
                header_counts
                    .entry(text.to_string())
                    .or_default()
                    .insert(page.page);
            }
            if is_pdf_footer_line(parsed, page.page, line) {
                footer_counts
                    .entry(text.to_string())
                    .or_default()
                    .insert(page.page);
            }
        }
    }
    let header = repeated_text_seen_on_all_pages(header_counts, page_count)
        .and_then(|text| repeated_chrome_line(parsed, pages, text.as_str(), true));
    let footer = repeated_text_seen_on_all_pages(footer_counts, page_count)
        .and_then(|text| repeated_chrome_line(parsed, pages, text.as_str(), false));
    RepeatedPageChrome { header, footer }
}

/// 全 page で検出された反復テキストを返す。
fn repeated_text_seen_on_all_pages(
    counts: BTreeMap<String, BTreeSet<usize>>,
    page_count: usize,
) -> Option<String> {
    counts
        .into_iter()
        .find_map(|(text, pages)| (pages.len() == page_count).then_some(text))
}

/// 反復テキストに対応する配置・style 付き代表行を返す。
fn repeated_chrome_line(
    parsed: &ParsedDocument,
    pages: &[StructuredTextPage],
    text: &str,
    header: bool,
) -> Option<RepeatedPageChromeLine> {
    pages.iter().find_map(|page| {
        let line = page.lines.iter().find(|line| {
            line.text.trim() == text
                && if header {
                    is_pdf_header_line(parsed, page.page, line)
                } else {
                    is_pdf_footer_line(parsed, page.page, line)
                }
        })?;
        let parsed_page = parsed
            .pages
            .iter()
            .find(|parsed_page| parsed_page.page_number == page.page)?;
        let (display_width, display_height) = parsed_page_display_size_points(parsed_page);
        let distance_points = if header {
            display_height - line.top_y
        } else {
            line.top_y - structured_line_max_font_size(line)
        };
        Some(RepeatedPageChromeLine {
            line: line.clone(),
            edge_distance_twips: pdf_points_to_floor_twips(distance_points.max(0.0))?,
            right_indent_twips: pdf_points_to_indent_twips(
                (display_width - line.right_x).max(0.0),
            )?,
        })
    })
}

/// structured line 内で最大の PDF font size を返す。
fn structured_line_max_font_size(line: &StructuredTextLine) -> f32 {
    line.runs
        .iter()
        .map(|run| run.style.font_size)
        .filter(|size| size.is_finite() && *size > 0.0)
        .fold(0.0f32, f32::max)
}

/// 反復 page chrome 行を PDF の右端余白も含む DOCX paragraph へ変換する。
fn paragraph_from_repeated_page_chrome_line(chrome_line: &RepeatedPageChromeLine) -> Paragraph {
    let paragraph = paragraph_from_structured_line(&chrome_line.line, 0.0, None, None);
    if chrome_line.line.alignment == ParagraphAlignment::Right && chrome_line.right_indent_twips > 0
    {
        paragraph.indent(None, None, Some(chrome_line.right_indent_twips), None)
    } else {
        paragraph
    }
}

/// 反復 page chrome を DOCX の header/footer part へ追加する。
fn add_repeated_page_chrome_to_docx(mut docx: Docx, chrome: &RepeatedPageChrome) -> Docx {
    if let Some(header) = chrome.header.as_ref() {
        docx = docx
            .header(Header::new().add_paragraph(paragraph_from_repeated_page_chrome_line(header)));
    }
    if let Some(footer) = chrome.footer.as_ref() {
        docx = docx
            .footer(Footer::new().add_paragraph(paragraph_from_repeated_page_chrome_line(footer)));
    }
    docx
}

/// 反復header/footerを順に確定し、混在ページの中間sectionへ本文とともに追加する。
/// docx-rs 0.4.22の同時設定は未確定footerを落とすため、footer確定後にheaderを追加する。
/// 一時追加した同じsectionをmoveして戻し、本文・画像payloadの複製を避ける。
fn append_section_with_repeated_chrome(
    mut docx: Docx,
    mut section: Section,
    chrome: &RepeatedPageChrome,
) -> Result<Docx, ConvertError> {
    if let Some(footer) = chrome.footer.as_ref() {
        section = section
            .footer(Footer::new().add_paragraph(paragraph_from_repeated_page_chrome_line(footer)));
    }
    docx = docx.add_section(section);
    if let Some(header) = chrome.header.as_ref() {
        let Some(docx_rs::DocumentChild::Section(section)) = docx.document.children.pop() else {
            return Err(ConvertError::Pack(
                "missing intermediate DOCX section".into(),
            ));
        };
        docx =
            docx.add_section(section.header(
                Header::new().add_paragraph(paragraph_from_repeated_page_chrome_line(header)),
            ));
    }
    Ok(docx)
}

/// DOCX header/footer に移した PDF 行を本文 flow から除外する。
fn remove_repeated_page_chrome_lines(
    parsed: &ParsedDocument,
    pages: Vec<StructuredTextPage>,
    chrome: &RepeatedPageChrome,
) -> Vec<StructuredTextPage> {
    pages
        .into_iter()
        .map(|mut page| {
            page.lines
                .retain(|line| !is_repeated_page_chrome_line(parsed, page.page, line, chrome));
            page
        })
        .collect()
}

/// 行が DOCX header/footer へ移した反復 page chrome か判定する。
fn is_repeated_page_chrome_line(
    parsed: &ParsedDocument,
    page: usize,
    line: &StructuredTextLine,
    chrome: &RepeatedPageChrome,
) -> bool {
    chrome.header.as_ref().is_some_and(|header| {
        line.text.trim() == header.line.text.trim() && is_pdf_header_line(parsed, page, line)
    }) || chrome.footer.as_ref().is_some_and(|footer| {
        line.text.trim() == footer.line.text.trim() && is_pdf_footer_line(parsed, page, line)
    })
}

/// PDF page 上部にある header らしい行か判定する。
fn is_pdf_header_line(parsed: &ParsedDocument, page: usize, line: &StructuredTextLine) -> bool {
    let Some(height) = page_height(parsed, page) else {
        return false;
    };
    line.top_y >= height * 0.88
}

/// PDF page 下部にある footer らしい行か判定する。
fn is_pdf_footer_line(parsed: &ParsedDocument, page: usize, line: &StructuredTextLine) -> bool {
    let Some(height) = page_height(parsed, page) else {
        return false;
    };
    line.top_y <= height * 0.12
}

/// structured DOCX へ出力する 1 run。
#[derive(Debug, Clone, PartialEq)]
struct StructuredTextRun {
    /// run テキスト。
    text: String,
    /// PDF text state から推定した run style。
    style: StructuredTextStyle,
}

/// structured DOCX へ反映する text style。
#[derive(Debug, Clone, PartialEq)]
struct StructuredTextStyle {
    /// font size（pt）。
    font_size: f32,
    /// PDF BaseFont 名。
    font_name: Option<String>,
    /// PDFに埋め込まれた同名subset fontをDOCXへ再利用できるか。
    font_embedded: bool,
    /// RGB hex fill color。
    fill_color: Option<String>,
    /// PDF の短い水平線から推定した下線。
    underlined: bool,
}

impl Default for StructuredTextStyle {
    fn default() -> Self {
        Self {
            font_size: 10.0,
            font_name: None,
            font_embedded: false,
            fill_color: None,
            underlined: false,
        }
    }
}

/// DOCX 段落へ反映する水平配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParagraphAlignment {
    /// 左寄せ。
    Left,
    /// 中央寄せ。
    Center,
    /// 右寄せ。
    Right,
}

/// structured DOCX の page 内ブロック。
#[derive(Debug, Clone, PartialEq)]
enum StructuredBlock {
    /// 段落。
    Paragraph {
        /// ブロック上端の PDF y 座標。
        top_y: f32,
        /// 段落 flow 内の順序。
        flow_index: usize,
        /// 段落行。
        line: StructuredTextLine,
    },
    /// 表。
    Table {
        /// flow 内の挿入順序。
        order_index: usize,
        /// 段を読む順の順位（同じ挿入順序の表・画像を段の順に並べる）。
        band_rank: (bool, usize),
        /// ブロック上端の PDF y 座標。
        top_y: f32,
        /// 表。
        table: PositionedTable,
    },
    /// 画像。
    Image {
        /// flow 内の挿入順序。
        order_index: usize,
        /// 段を読む順の順位（同じ挿入順序の表・画像を段の順に並べる）。
        band_rank: (bool, usize),
        /// ブロック上端の PDF y 座標。
        top_y: f32,
        /// 画像要素。
        image: ParsedImageElement,
        /// 解析画像配列内の位置。
        image_index: usize,
    },
}

impl StructuredBlock {
    /// ブロック上端の PDF y 座標を返す。
    fn top_y(&self) -> f32 {
        match self {
            StructuredBlock::Paragraph { top_y, .. }
            | StructuredBlock::Table { top_y, .. }
            | StructuredBlock::Image { top_y, .. } => *top_y,
        }
    }

    /// 表・画像の段を読む順の順位を返す（段落は段の順に並べ済み）。
    fn band_rank(&self) -> (bool, usize) {
        match self {
            StructuredBlock::Paragraph { .. } => (false, 0),
            StructuredBlock::Table { band_rank, .. } | StructuredBlock::Image { band_rank, .. } => {
                *band_rank
            }
        }
    }

    /// 同じ座標のブロックを安定して並べるための種別順を返す。
    fn kind_order(&self) -> usize {
        match self {
            StructuredBlock::Paragraph { .. } => 0,
            StructuredBlock::Table { .. } => 1,
            StructuredBlock::Image { .. } => 2,
        }
    }

    /// 段落 flow 内へ table/image を挿入するための順序を返す。
    fn flow_order(&self) -> usize {
        match self {
            StructuredBlock::Paragraph { flow_index, .. } => flow_index.saturating_mul(2) + 1,
            StructuredBlock::Table { order_index, .. }
            | StructuredBlock::Image { order_index, .. } => *order_index,
        }
    }
}

/// structured block を DOCX 出力順へ並べる。
fn compare_structured_blocks(a: &StructuredBlock, b: &StructuredBlock) -> std::cmp::Ordering {
    a.flow_order()
        .cmp(&b.flow_order())
        .then(a.kind_order().cmp(&b.kind_order()))
        .then(a.band_rank().cmp(&b.band_rank()))
        .then(b.top_y().total_cmp(&a.top_y()))
}

/// 段落 flow に対して table/image を挿入する順序を作る。
fn block_insertion_order_index(lines: &[StructuredTextLine], top_y: f32) -> usize {
    lines
        .iter()
        .position(|line| line.top_y < top_y)
        .map(|index| index.saturating_mul(2))
        .unwrap_or_else(|| lines.len().saturating_mul(2))
}

/// PDF 上の位置を保持した推定表。
#[derive(Debug, Clone, PartialEq)]
struct PositionedTable {
    /// PDF page 番号。
    page: usize,
    /// 表全体の bounds。
    bounds: ParsedBounds,
    /// 表罫線のおおまかな線幅。
    stroke_width: f32,
    /// 表セルで共通する解決可能な PDF BaseFont 名。
    font_name: Option<String>,
    /// 表セルの共通fontをPDF埋め込みsubsetから再利用できるか。
    font_embedded: bool,
    /// 表セルで共通する妥当な PDF font size（pt）。
    font_size: Option<f32>,
    /// DOCX 出力用の列幅（twips）。
    column_widths_twips: Vec<usize>,
    /// 表セル。
    table: OcrTable,
    /// セル内テキストへ DOCX 下線を付けるか。
    cell_underlines: Vec<Vec<bool>>,
    /// PDF座標から推定したセル内テキストの水平配置。
    cell_alignments: Vec<Vec<ParagraphAlignment>>,
    /// PDF座標から推定したセル内テキストの縦位置（`None` は従来どおり中央）。
    cell_vertical_alignments: Vec<Vec<Option<CellVerticalAlignment>>>,
    /// DOCX 出力用のセル結合情報。
    docx_table: Option<DocxTableModel>,
    /// 外側セル内に含まれる子表。
    nested_tables: Vec<PositionedTable>,
    /// 上から順の各行の高さ（pt）。空の場合は表の高さを行数で等分する。
    row_heights_pt: Vec<f32>,
    /// 上から順の各行で、1 セル内に並ぶ最大の行数。
    row_line_counts: Vec<usize>,
    /// 左寄せ・中央揃えのセルで観測した、セル左端から文字までの最小距離（pt）。
    cell_padding_pt: Option<f32>,
    /// 上から順の各マスの文字サイズ（pt、マス内の中央値）。
    cell_font_sizes: Vec<Vec<Option<f32>>>,
    /// 上から順の各マスの段落ごとのタブ位置。
    cell_tab_stops: Vec<Vec<CellParagraphTabs>>,
    /// 上から順の各マスへ割り当てた文字片（子表を除いて組み直すために保持する）。
    cell_fragments: Vec<Vec<Vec<table_cell_text::CellFragment>>>,
    /// 上から順の各マスの背景色（RGB hex）。
    cell_shading: Vec<Vec<Option<String>>>,
}

/// 1 マスの段落ごとのタブ位置（セル左端からの距離 pt と、右揃えか）。
type CellParagraphTabs = Vec<Vec<(f32, bool)>>;

/// DOCX 出力用の表モデル。
#[derive(Debug, Clone, PartialEq)]
struct DocxTableModel {
    /// 行。
    rows: Vec<Vec<DocxTableCellModel>>,
}

/// DOCX 出力用のセルモデル。
#[derive(Debug, Clone, PartialEq)]
struct DocxTableCellModel {
    /// セルテキスト。
    text: String,
    /// セルテキストへ下線を付ける。
    underlined: bool,
    /// セル段落の水平配置。
    alignment: ParagraphAlignment,
    /// セル内の縦位置（`None` は中央）。
    vertical_alignment: Option<CellVerticalAlignment>,
    /// 横方向の結合セル数。
    grid_span: usize,
    /// 縦方向の結合種別。
    vertical_merge: Option<DocxVerticalMerge>,
    /// セル幅（twips）。
    width_twips: Option<usize>,
    /// PDF graphics 由来のセル罫線。
    borders: Option<DocxCellBorders>,
}

/// DOCX 出力用のセル罫線。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DocxCellBorders {
    /// 上罫線を出力する。
    top: bool,
    /// 左罫線を出力する。
    left: bool,
    /// 下罫線を出力する。
    bottom: bool,
    /// 右罫線を出力する。
    right: bool,
    /// DOCX の border size（1/8pt 単位）。
    size_eighths_pt: usize,
    /// 破線で描く辺（上・左・下・右の順）。
    dashed: [bool; 4],
}

impl DocxCellBorders {
    /// 上下左右すべての罫線を持つセル罫線を返す。
    fn all() -> Self {
        Self::all_with_size(2)
    }

    /// 指定した DOCX border size で上下左右すべての罫線を持つセル罫線を返す。
    fn all_with_size(size_eighths_pt: usize) -> Self {
        Self {
            top: true,
            left: true,
            bottom: true,
            right: true,
            size_eighths_pt,
            dashed: [false; 4],
        }
    }
}

/// セル内の縦位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellVerticalAlignment {
    /// 上寄せ。
    Top,
    /// 中央。
    Center,
    /// 下寄せ。
    Bottom,
}

/// DOCX 出力用の縦結合種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocxVerticalMerge {
    /// 縦結合の開始セル。
    Restart,
    /// 縦結合の継続セル。
    Continue,
}

/// デバッグ段落なしで OCR 本文だけを載せた DOCX を組み立てる。
fn build_body_docx(
    ocr_text_lines: &Option<Vec<String>>,
    ocr_table: &Option<OcrTable>,
) -> Result<Docx, ConvertError> {
    let mut docx = Docx::new();
    if let Some(lines) = ocr_text_lines.as_ref() {
        for line in normalize_ocr_text_lines(lines) {
            docx = docx.add_paragraph(Paragraph::new().add_run(text_run(line.as_str())));
        }
    }
    if let Some(table) = ocr_table {
        table.validate().map_err(ConvertError::InvalidOcrTable)?;
        docx = docx.add_table(docx_table_from_ocr_table(table));
    }
    Ok(docx)
}

/// DOCX 出力に使うテキストプレビューを決定する。
///
/// `ocr_text_lines` が有効値を持つ場合はそれを優先し、無ければ parser preview を使う。
fn resolve_text_preview(parser_text_preview: &str, ocr_text_lines: &Option<Vec<String>>) -> String {
    if let Some(lines) = ocr_text_lines.as_ref() {
        let normalized = normalize_ocr_text_lines(lines);
        if !normalized.is_empty() {
            return normalized.join("\n");
        }
    }
    parser_text_preview.to_string()
}

/// OCR が文中で分割した行を、段落化前に自然な本文行へ正規化する。
fn normalize_ocr_text_lines(lines: &[String]) -> Vec<String> {
    let mut normalized: Vec<String> = Vec::new();
    for line in lines
        .iter()
        .flat_map(|line| line.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(previous) = normalized.last_mut()
            && should_join_ocr_line(previous, line)
        {
            if should_insert_space_between_joined_ocr_lines(previous, line) {
                previous.push(' ');
            }
            previous.push_str(line);
            continue;
        }
        normalized.push(line.to_string());
    }
    normalized
}

/// OCR 行同士が意図しない文中改行として連結できるか判定する。
fn should_join_ocr_line(previous: &str, next: &str) -> bool {
    contains_cjk(previous)
        && contains_cjk(next)
        && !ends_with_sentence_boundary(previous)
        && !starts_with_structural_marker(next)
}

/// OCR 行連結時に空白を挟むべきか判定する。
fn should_insert_space_between_joined_ocr_lines(previous: &str, next: &str) -> bool {
    previous
        .chars()
        .last()
        .is_some_and(|ch| ch.is_ascii_alphanumeric())
        && next
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric())
}

/// CJK 文字を含むか判定する。
fn contains_cjk(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch as u32,
            0x3040..=0x30ff | 0x3400..=0x9fff | 0xf900..=0xfaff | 0xff66..=0xff9f
        )
    })
}

/// 文や段落の終端として扱う文字で終わるか判定する。
fn ends_with_sentence_boundary(text: &str) -> bool {
    text.chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| {
            matches!(
                ch,
                '。' | '．' | '.' | '？' | '?' | '！' | '!' | '」' | '』' | '）' | ')' | ']' | '】'
            )
        })
}

/// 箇条書き・番号・見出しらしい行頭か判定する。
fn starts_with_structural_marker(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with('・')
        || trimmed.starts_with('●')
        || trimmed.starts_with('○')
        || trimmed.starts_with('※')
        || trimmed.starts_with('第')
        || trimmed
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit() || matches!(ch, '(' | '（' | '[' | '【'))
}

/// text preview を browser 出力用の本文行に変換する。
fn text_preview_to_body_lines(preview: &str) -> Option<Vec<String>> {
    let lines = preview
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "(no text extracted)")
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if lines.is_empty() { None } else { Some(lines) }
}

/// テキストプレビューを DOCX 段落へ追加する。
///
/// 複数行の preview は 1 行ごとに段落を分割し、差分確認しやすくする。
fn add_text_preview_paragraphs(docx: Docx, preview: &str) -> Docx {
    let mut next = docx.add_paragraph(Paragraph::new().add_run(text_run("text preview:")));
    for line in preview.lines() {
        next = next.add_paragraph(Paragraph::new().add_run(text_run(line)));
    }
    next
}

/// `OcrTable` から `docx-rs` の表要素を生成する。
fn docx_table_from_ocr_table(table: &OcrTable) -> Table {
    if table
        .spans
        .iter()
        .flatten()
        .any(|span| span.row_span > 1 || span.col_span > 1)
    {
        return docx_table_from_model(&docx_table_model_from_ocr_table(table));
    }

    let mut rows: Vec<TableRow> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let cells: Vec<TableCell> = row
            .iter()
            .map(|cell_text| {
                TableCell::new()
                    .add_paragraph(Paragraph::new().add_run(text_run(cell_text.as_str())))
            })
            .collect();
        rows.push(TableRow::new(cells));
    }
    Table::new(rows)
}

/// 公開 `OcrTable` の span 情報を DOCX 出力用モデルへ変換する。
fn docx_table_model_from_ocr_table(table: &OcrTable) -> DocxTableModel {
    let rows_len = table.rows.len();
    let cols_len = table.rows.first().map_or(0, Vec::len);
    let mut covered_by = vec![vec![None::<(usize, usize)>; cols_len]; rows_len];
    for row in 0..rows_len {
        for col in 0..cols_len {
            if covered_by[row][col].is_some() {
                continue;
            }
            let span = table.spans[row][col];
            for r in row..(row + span.row_span).min(rows_len) {
                for c in col..(col + span.col_span).min(cols_len) {
                    covered_by[r][c] = Some((row, col));
                }
            }
        }
    }

    let mut rows = Vec::with_capacity(rows_len);
    for row in 0..rows_len {
        let mut modeled_row = Vec::new();
        let mut col = 0usize;
        while col < cols_len {
            let root = covered_by[row][col].unwrap_or((row, col));
            let root_span = table.spans[root.0][root.1];
            if root.0 == row && root.1 == col {
                modeled_row.push(DocxTableCellModel {
                    text: table.rows[row][col].clone(),
                    underlined: false,
                    alignment: ParagraphAlignment::Left,
                    vertical_alignment: None,
                    grid_span: root_span.col_span,
                    vertical_merge: (root_span.row_span > 1).then_some(DocxVerticalMerge::Restart),
                    width_twips: None,
                    borders: Some(DocxCellBorders::all()),
                });
            } else if root.1 == col {
                modeled_row.push(DocxTableCellModel {
                    text: String::new(),
                    underlined: false,
                    alignment: ParagraphAlignment::Left,
                    vertical_alignment: None,
                    grid_span: root_span.col_span,
                    vertical_merge: (root_span.row_span > 1).then_some(DocxVerticalMerge::Continue),
                    width_twips: None,
                    borders: Some(DocxCellBorders::all()),
                });
            }
            col += root_span.col_span.max(1);
        }
        rows.push(modeled_row);
    }
    DocxTableModel { rows }
}

/// 位置付き表モデルから `docx-rs` の表要素を生成する。
fn docx_table_from_structured_table(table: &PositionedTable, origin_x: f32) -> Table {
    let row_heights = if table.nested_tables.is_empty() {
        positioned_table_row_heights(table)
    } else {
        Vec::new()
    };
    let border_size = table_border_size_for_pdf_rule(table.stroke_width);
    let docx_table = table
        .docx_table
        .as_ref()
        .map(|model| {
            let model = docx_table_model_with_column_widths(model, &table.column_widths_twips);
            let model = docx_table_model_with_cell_underlines(&model, &table.cell_underlines);
            let model = docx_table_model_with_border_size(&model, border_size);
            docx_table_from_model_with_row_height_and_nested(&model, &row_heights, Some(table))
        })
        .unwrap_or_else(|| {
            let model = docx_table_model_with_column_widths(
                &docx_table_model_from_ocr_table(&table.table),
                &table.column_widths_twips,
            );
            let model = docx_table_model_with_cell_underlines(&model, &table.cell_underlines);
            let model = docx_table_model_with_cell_alignments(&model, &table.cell_alignments);
            let model = docx_table_model_with_cell_vertical_alignments(
                &model,
                &table.cell_vertical_alignments,
            );
            let model = docx_table_model_with_border_size(&model, border_size);
            docx_table_from_model_with_row_height_and_nested(&model, &row_heights, Some(table))
        });
    apply_positioned_table_layout(docx_table, table, origin_x)
}

/// DOCX 表モデルへ PDF 由来の列幅を割り当てる。
fn docx_table_model_with_column_widths(
    model: &DocxTableModel,
    column_widths_twips: &[usize],
) -> DocxTableModel {
    if column_widths_twips.is_empty() {
        return model.clone();
    }
    let rows = model
        .rows
        .iter()
        .map(|row| {
            let mut column = 0usize;
            row.iter()
                .map(|cell| {
                    let span = cell.grid_span.max(1);
                    let width_twips = column_widths_twips
                        .get(column..column.saturating_add(span))
                        .map(|widths| widths.iter().sum::<usize>());
                    column += span;
                    let mut cell = cell.clone();
                    cell.width_twips = width_twips;
                    cell
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    DocxTableModel { rows }
}

/// DOCX 表モデルへ PDF 由来のセル下線情報を割り当てる。
fn docx_table_model_with_cell_underlines(
    model: &DocxTableModel,
    cell_underlines: &[Vec<bool>],
) -> DocxTableModel {
    if cell_underlines.is_empty() {
        return model.clone();
    }
    let rows = model
        .rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            let mut column = 0usize;
            row.iter()
                .map(|cell| {
                    let span = cell.grid_span.max(1);
                    let underlined = (column..column.saturating_add(span)).any(|col| {
                        cell_underlines
                            .get(row_index)
                            .and_then(|row| row.get(col))
                            .copied()
                            .unwrap_or(false)
                    });
                    column += span;
                    let mut cell = cell.clone();
                    cell.underlined |= underlined;
                    cell
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    DocxTableModel { rows }
}

/// DOCX表モデルへPDF座標から推定したセル段落配置を割り当てる。
fn docx_table_model_with_cell_alignments(
    model: &DocxTableModel,
    cell_alignments: &[Vec<ParagraphAlignment>],
) -> DocxTableModel {
    if cell_alignments.is_empty() {
        return model.clone();
    }
    let rows = model
        .rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            let mut column = 0usize;
            row.iter()
                .map(|cell| {
                    let span = cell.grid_span.max(1);
                    let alignment = cell_alignments
                        .get(row_index)
                        .and_then(|row| row.get(column..column.saturating_add(span)))
                        .and_then(|alignments| {
                            alignments
                                .iter()
                                .copied()
                                .find(|alignment| *alignment != ParagraphAlignment::Left)
                                .or_else(|| alignments.first().copied())
                        })
                        .unwrap_or(ParagraphAlignment::Left);
                    column += span;
                    let mut cell = cell.clone();
                    cell.alignment = alignment;
                    cell
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    DocxTableModel { rows }
}

/// 結合のない DOCX 表モデルへ、マスごとに推定した縦位置を割り当てる。
fn docx_table_model_with_cell_vertical_alignments(
    model: &DocxTableModel,
    vertical_alignments: &[Vec<Option<CellVerticalAlignment>>],
) -> DocxTableModel {
    if vertical_alignments.is_empty() {
        return model.clone();
    }
    let rows = model
        .rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            let mut column = 0usize;
            row.iter()
                .map(|cell| {
                    let mut cell = cell.clone();
                    cell.vertical_alignment = vertical_alignments
                        .get(row_index)
                        .and_then(|row| row.get(column))
                        .copied()
                        .flatten();
                    column += cell.grid_span.max(1);
                    cell
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    DocxTableModel { rows }
}

/// DOCX 表モデル内の罫線サイズを PDF 由来の代表線幅に合わせる。
fn docx_table_model_with_border_size(
    model: &DocxTableModel,
    size_eighths_pt: usize,
) -> DocxTableModel {
    let rows = model
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| {
                    let mut cell = cell.clone();
                    if let Some(mut borders) = cell.borders {
                        borders.size_eighths_pt = size_eighths_pt;
                        cell.borders = Some(borders);
                    }
                    cell
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    DocxTableModel { rows }
}

/// PDF 上の各行の高さを DOCX の行高へ変換する。
///
/// 1 行だけの行は PDF の高さで固定し、折り返しや改行を含む行は内容に合わせて伸ばせるようにする。
fn positioned_table_row_heights(table: &PositionedTable) -> Vec<DocxRowHeight> {
    let row_count = table.table.rows.len();
    if row_count > 0 && table.row_heights_pt.len() == row_count {
        let adjustment = thin_table_rule_height_adjustment(table.stroke_width) / row_count as f32;
        return table
            .row_heights_pt
            .iter()
            .enumerate()
            .filter_map(|(row, height)| {
                let twips = pdf_points_to_twips_f32(height + adjustment)?;
                let lines = table.row_line_counts.get(row).copied().unwrap_or(1);
                Some(DocxRowHeight {
                    twips,
                    exact: lines <= 1,
                })
            })
            .collect();
    }
    positioned_table_row_height_twips(table)
        .map(|twips| vec![DocxRowHeight { twips, exact: true }; row_count])
        .unwrap_or_default()
}

/// DOCX の 1 行分の高さ指定。
#[derive(Debug, Clone, Copy, PartialEq)]
struct DocxRowHeight {
    /// 行の高さ（twips）。
    twips: f32,
    /// `exact` で固定するか。`false` は `atLeast` で内容に合わせて伸ばす。
    exact: bool,
}

/// PDF 上の表高さを DOCX の行高（twips）に変換する。
fn positioned_table_row_height_twips(table: &PositionedTable) -> Option<f32> {
    let row_count = table.table.rows.len();
    if row_count == 0 {
        return None;
    }
    let height = (table.bounds.y1 - table.bounds.y0).abs()
        + thin_table_rule_height_adjustment(table.stroke_width);
    pdf_points_to_twips_f32(height / row_count as f32)
}

/// 細い PDF 罫線が raster bbox に含まれる分を表高さへ足す。
fn thin_table_rule_height_adjustment(stroke_width: f32) -> f32 {
    if stroke_width.is_finite() && stroke_width > 0.0 && stroke_width < 0.75 {
        stroke_width * 2.0
    } else {
        0.0
    }
}

/// PDF 上の表矩形を DOCX 表プロパティへ反映する。
fn apply_positioned_table_layout(
    docx_table: Table,
    table: &PositionedTable,
    origin_x: f32,
) -> Table {
    let width = (table.bounds.x1 - table.bounds.x0).abs();
    let top_cell_margin = positioned_table_top_cell_margin_twips(table);
    let (horizontal_cell_margin, right_cell_margin) = table_cell_horizontal_margins(table);
    let mut docx_table = docx_table
        .align(TableAlignmentType::Left)
        .layout(TableLayoutType::Fixed)
        .margins(TableCellMargins::new().margin(
            top_cell_margin,
            horizontal_cell_margin,
            0,
            right_cell_margin,
        ));
    if !table.column_widths_twips.is_empty() {
        docx_table = docx_table.set_grid(table.column_widths_twips.clone());
    }
    if let Some(width_twips) = pdf_points_to_twips(width)
        && width_twips > 0
    {
        docx_table = docx_table.width(width_twips, WidthType::Dxa);
    }
    if let Some(indent) = pdf_points_to_indent_twips((table.bounds.x0 - origin_x).max(0.0))
        && indent > 0
    {
        docx_table = docx_table.indent(indent);
    }
    docx_table
}

/// 表セルの左右の余白（twips）を返す。
///
/// PDF のセル内余白が既定より狭い表は同じ左余白にし、右余白は半分にして
/// 代替フォントの字幅の差で列の中の文字が折り返さないよう余裕を残す。
fn table_cell_horizontal_margins(table: &PositionedTable) -> (usize, usize) {
    let horizontal_cell_margin = table
        .font_name
        .as_deref()
        .filter(|font_name| !table.font_embedded && pdf_font_name_is_ms_pgothic(font_name))
        .map(|_| MS_PGOTHIC_TABLE_CELL_HORIZONTAL_MARGIN_TWIPS)
        .unwrap_or(POSITIONED_TABLE_CELL_HORIZONTAL_MARGIN_TWIPS);
    table
        .cell_padding_pt
        .and_then(pdf_points_to_twips)
        .filter(|padding| *padding < horizontal_cell_margin)
        .map(|padding| (padding, padding / 2))
        .unwrap_or((horizontal_cell_margin, horizontal_cell_margin))
}

/// 表形状・font・セル配置から上セル余白のbaseline補正値を返す。
fn positioned_table_top_cell_margin_twips(table: &PositionedTable) -> usize {
    let is_mincho = table
        .font_name
        .as_deref()
        .is_some_and(pdf_font_name_is_mincho);
    if !is_mincho {
        return 0;
    }
    let is_single_cell = table.table.rows.len() == 1 && table.table.col_count() == Some(1);
    let is_centered = table
        .cell_alignments
        .first()
        .and_then(|row| row.first())
        .is_some_and(|alignment| *alignment == ParagraphAlignment::Center);
    let table_height = (table.bounds.y1 - table.bounds.y0).abs();
    let is_tall = table
        .font_size
        .is_some_and(|font_size| table_height >= font_size * 8.0);
    if is_single_cell && is_centered && is_tall {
        CENTERED_MINCHO_CONTAINER_TOP_MARGIN_TWIPS
    } else {
        MINCHO_TABLE_CELL_TOP_MARGIN_TWIPS
    }
}

/// 結合情報つき表モデルから `docx-rs` の表要素を生成する。
fn docx_table_from_model(model: &DocxTableModel) -> Table {
    docx_table_from_model_with_row_height(model, None)
}

/// 結合情報つき表モデルから行高指定つき `docx-rs` の表要素を生成する。
fn docx_table_from_model_with_row_height(
    model: &DocxTableModel,
    row_height_twips: Option<f32>,
) -> Table {
    let row_heights = row_height_twips
        .map(|twips| vec![DocxRowHeight { twips, exact: true }; model.rows.len()])
        .unwrap_or_default();
    docx_table_from_model_with_row_height_and_nested(model, &row_heights, None)
}

/// 結合情報つき表モデルから子表を含めて `docx-rs` の表要素を生成する。
fn docx_table_from_model_with_row_height_and_nested(
    model: &DocxTableModel,
    row_heights: &[DocxRowHeight],
    parent: Option<&PositionedTable>,
) -> Table {
    let rows = model
        .rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            let mut grid_col = 0usize;
            let cells = row
                .iter()
                .map(|cell| {
                    let nested_tables = parent
                        .map(|parent| {
                            nested_tables_for_cell(parent, row_index, grid_col, cell.grid_span)
                        })
                        .unwrap_or_default();
                    let has_nested_tables = !nested_tables.is_empty();
                    // 子表を含むセルは、子表の範囲にある文字片を除いて本文を組み直す。
                    let recomposed = parent.filter(|_| has_nested_tables).and_then(|parent| {
                        cell_text_outside_nested_tables(
                            parent,
                            row_index,
                            grid_col,
                            cell.grid_span,
                            &nested_tables,
                        )
                    });
                    let text = recomposed
                        .as_ref()
                        .map(|parts| parts.above.0.clone())
                        .unwrap_or_else(|| {
                            cell_text_without_nested_tables(cell.text.as_str(), &nested_tables)
                        });
                    let mut table_cell = TableCell::new();
                    let cell_font_size = parent
                        .and_then(|table| table.cell_font_sizes.get(row_index))
                        .and_then(|row| row.get(grid_col))
                        .copied()
                        .flatten();
                    let cell_tabs = match recomposed.as_ref() {
                        Some(parts) => Some(&parts.above.1),
                        None => parent
                            .and_then(|table| table.cell_tab_stops.get(row_index))
                            .and_then(|row| row.get(grid_col)),
                    };
                    // 子表の上に本文がないセルは、空の段落を置かずに子表から始める。
                    let skip_empty_lead = recomposed.is_some() && text.trim().is_empty();
                    for (line_index, line) in
                        text.split('\n').enumerate().filter(|_| !skip_empty_lead)
                    {
                        let stops = cell_tabs
                            .and_then(|tabs| tabs.get(line_index))
                            .map(Vec::as_slice)
                            .unwrap_or(&[]);
                        table_cell = table_cell.add_paragraph(table_cell_paragraph(
                            line,
                            cell,
                            parent,
                            cell_font_size,
                            stops,
                        ));
                    }
                    if parent.is_some() {
                        table_cell = table_cell.vertical_align(match cell.vertical_alignment {
                            Some(CellVerticalAlignment::Top) => VAlignType::Top,
                            Some(CellVerticalAlignment::Bottom) => VAlignType::Bottom,
                            Some(CellVerticalAlignment::Center) | None => VAlignType::Center,
                        });
                    }
                    for nested_table in nested_tables {
                        table_cell = table_cell.add_table(docx_table_from_structured_table(
                            nested_table,
                            nested_table.bounds.x0,
                        ));
                    }
                    // 子表より下の本文は子表の後に置く（セルは段落で終わる必要もある）。
                    let below = recomposed
                        .as_ref()
                        .map(|parts| &parts.below)
                        .filter(|(text, _)| !text.trim().is_empty());
                    if let Some((text, tabs)) = below {
                        for (line_index, line) in text.split('\n').enumerate() {
                            let stops = tabs.get(line_index).map(Vec::as_slice).unwrap_or(&[]);
                            table_cell = table_cell.add_paragraph(table_cell_paragraph(
                                line,
                                cell,
                                parent,
                                cell_font_size,
                                stops,
                            ));
                        }
                    } else if has_nested_tables {
                        table_cell = table_cell.add_paragraph(Paragraph::new());
                    }
                    if cell.grid_span > 1 {
                        table_cell = table_cell.grid_span(cell.grid_span);
                    }
                    if let Some(width_twips) = cell.width_twips
                        && width_twips > 0
                    {
                        table_cell = table_cell.width(width_twips, WidthType::Dxa);
                    }
                    if let Some(vertical_merge) = cell.vertical_merge {
                        table_cell = table_cell.vertical_merge(match vertical_merge {
                            DocxVerticalMerge::Restart => VMergeType::Restart,
                            DocxVerticalMerge::Continue => VMergeType::Continue,
                        });
                    }
                    if let Some(borders) = cell.borders {
                        table_cell = table_cell.set_borders(table_cell_borders(borders));
                    }
                    if let Some(color) = parent
                        .and_then(|table| table.cell_shading.get(row_index))
                        .and_then(|row| row.get(grid_col))
                        .cloned()
                        .flatten()
                    {
                        table_cell =
                            table_cell.shading(Shading::new().shd_type(ShdType::Clear).fill(color));
                    }
                    grid_col += cell.grid_span.max(1);
                    table_cell
                })
                .collect::<Vec<_>>();
            // 見出し行がページ末に取り残されないよう、先頭行を次の行と同じページに置く。
            let mut cells = cells;
            if row_index == 0 && model.rows.len() > 1 {
                keep_cells_with_next_row(&mut cells);
            }
            let mut table_row = TableRow::new(cells);
            if parent.is_some() {
                table_row = table_row.cant_split();
            }
            if let Some(height) = row_heights.get(row_index)
                && height.twips > 0.0
            {
                table_row = table_row
                    .row_height(height.twips)
                    .height_rule(if height.exact {
                        HeightRule::Exact
                    } else {
                        HeightRule::AtLeast
                    });
            }
            table_row
        })
        .collect::<Vec<_>>();
    Table::without_borders(rows)
}

/// 各セルの最後の段落に keepNext を付け、行を次の行と同じページに置く。
///
/// 行そのものを分割不可にしたり繰り返し見出しにしたりはせず、長い行がページをまたげるようにする
/// （lawrenceanum の kanpo-docx と同じ）。
fn keep_cells_with_next_row(cells: &mut [TableCell]) {
    for cell in cells {
        if let Some(TableCellContent::Paragraph(paragraph)) = cell
            .children
            .iter_mut()
            .rev()
            .find(|content| matches!(content, TableCellContent::Paragraph(_)))
        {
            **paragraph = std::mem::take(&mut **paragraph).keep_next(true);
        }
    }
}

/// セル本文の 1 段落分を、表の font・下線・配置を適用して作る。
fn table_cell_paragraph(
    text: &str,
    cell: &DocxTableCellModel,
    parent: Option<&PositionedTable>,
    cell_font_size: Option<f32>,
    tab_stops: &[(f32, bool)],
) -> Paragraph {
    let mut run = Run::new();
    for (index, segment) in text.split('\t').enumerate() {
        if index > 0 {
            run = run.add_tab();
        }
        run = run.add_text(sanitize_docx_text(segment));
    }
    if let Some(font_name) = parent.and_then(|table| table.font_name.as_deref()) {
        let embedded = parent.is_some_and(|table| table.font_embedded);
        run = apply_pdf_font_name_to_run(run, font_name, embedded);
        if !embedded && let Some(stretch) = docx_font_stretch_from_pdf_font_name(font_name) {
            run = run.stretch(stretch);
        }
    }
    // マスの文字サイズ、なければ表で共通の文字サイズを使う。
    if let Some(size) = cell_font_size
        .or_else(|| parent.and_then(|table| table.font_size))
        .and_then(pdf_font_size_to_half_points)
        .filter(|size| *size != DEFAULT_DOCX_FONT_SIZE_HALF_POINTS)
    {
        run = run.size(size);
    }
    if cell.underlined && !text.trim().is_empty() {
        run = run.underline("single");
    }
    let mut paragraph = Paragraph::new().add_run(run);
    // タブの後の語は PDF と同じ位置へ置く（数値は右端に右揃え、それ以外は左端に左揃え）。
    let left_margin = parent
        .map(|table| table_cell_horizontal_margins(table).0)
        .unwrap_or(POSITIONED_TABLE_CELL_HORIZONTAL_MARGIN_TWIPS);
    let tab_count = text.matches('\t').count();
    // タブ位置はセルの内容幅（左右の余白を除いた幅）より 1pt 内側に収める。
    // 内容幅を超えた右揃えタブは、Word や LibreOffice で値の末尾が切れる。
    let right_margin = parent
        .map(|table| table_cell_horizontal_margins(table).1)
        .unwrap_or(POSITIONED_TABLE_CELL_HORIZONTAL_MARGIN_TWIPS);
    let content_limit = cell
        .width_twips
        .and_then(|width| width.checked_sub(left_margin + right_margin + 20));
    if tab_count > 0 && tab_stops.len() == tab_count {
        for (offset, right) in tab_stops {
            let Some(position) = pdf_points_to_twips(*offset)
                .and_then(|twips| twips.checked_sub(left_margin))
                .map(|position| content_limit.map_or(position, |limit| position.min(limit)))
                .filter(|position| *position > 0)
            else {
                continue;
            };
            paragraph = paragraph.add_tab(
                Tab::new()
                    .val(if *right {
                        TabValueType::Right
                    } else {
                        TabValueType::Left
                    })
                    .pos(position),
            );
        }
    } else if tab_count > 0
        && let Some(width) = cell.width_twips
        && width > 2 * left_margin
    {
        // 位置が分からないタブは、セルの内側右端の右揃えタブにする。
        paragraph = paragraph.add_tab(
            Tab::new()
                .val(TabValueType::Right)
                .pos(width - 2 * left_margin),
        );
    }
    paragraph = match cell.alignment {
        ParagraphAlignment::Left => paragraph,
        ParagraphAlignment::Center => paragraph.align(AlignmentType::Center),
        ParagraphAlignment::Right => paragraph.align(AlignmentType::Right),
    };
    if parent.is_some() {
        paragraph = paragraph.line_spacing(LineSpacing::new().before(0).after(0));
    }
    paragraph
}

/// 親表セルに含まれる子表を返す。
fn nested_tables_for_cell(
    parent: &PositionedTable,
    row_index: usize,
    col_index: usize,
    col_span: usize,
) -> Vec<&PositionedTable> {
    let Some(col_count) = parent.table.col_count() else {
        return Vec::new();
    };
    let row_count = parent.table.row_count();
    if row_count == 0 || col_count == 0 {
        return Vec::new();
    }
    let width = (parent.bounds.x1 - parent.bounds.x0) / col_count as f32;
    let height = (parent.bounds.y1 - parent.bounds.y0) / row_count as f32;
    // 列幅が分かる場合は実際の列境界、分からない場合は等分の境界を使う。
    let column_x = |col: usize| {
        if parent.column_widths_twips.len() == col_count {
            parent.bounds.x0
                + parent.column_widths_twips[..col.min(col_count)]
                    .iter()
                    .map(|twips| *twips as f32 / 20.0)
                    .sum::<f32>()
        } else {
            parent.bounds.x0 + width * col.min(col_count) as f32
        }
    };
    let x0 = column_x(col_index);
    let x1 = column_x(col_index + col_span.max(1));
    let y1 = parent.bounds.y1 - height * row_index as f32;
    let y0 = parent.bounds.y1 - height * (row_index + 1).min(row_count) as f32;
    parent
        .nested_tables
        .iter()
        .filter(|table| {
            let center_x = (table.bounds.x0 + table.bounds.x1) / 2.0;
            let center_y = (table.bounds.y0 + table.bounds.y1) / 2.0;
            center_x >= x0 - 1.0
                && center_x <= x1 + 1.0
                && center_y >= y0 - 1.0
                && center_y <= y1 + 1.0
        })
        .collect()
}

/// 親表のマスの文字片から、子表の範囲にある文字片を除いて本文を組み直す。
///
/// 文字片を保持していない表は `None` を返し、文字列の除去に任せる。
fn cell_text_outside_nested_tables(
    parent: &PositionedTable,
    row_index: usize,
    grid_col: usize,
    grid_span: usize,
    nested_tables: &[&PositionedTable],
) -> Option<NestedCellText> {
    let row = parent.cell_fragments.get(row_index)?;
    let cols = parent.column_widths_twips.len();
    if cols == 0 || row.len() != cols {
        return None;
    }
    let column_x = |col: usize| {
        parent.bounds.x0
            + parent.column_widths_twips[..col.min(cols)]
                .iter()
                .map(|twips| *twips as f32 / 20.0)
                .sum::<f32>()
    };
    let inside_child = |fragment: &table_cell_text::CellFragment| {
        let x = (fragment.bounds.x0 + fragment.bounds.x1) / 2.0;
        let y = (fragment.bounds.y0 + fragment.bounds.y1) / 2.0;
        nested_tables.iter().any(|child| {
            let bounds = normalized_bounds(child.bounds);
            x >= bounds.x0 - 1.0
                && x <= bounds.x1 + 1.0
                && y >= bounds.y0 - 1.0
                && y <= bounds.y1 + 1.0
        })
    };
    let fragments = row
        .get(grid_col..(grid_col + grid_span.max(1)).min(cols))?
        .iter()
        .flatten()
        .filter(|fragment| !inside_child(fragment))
        .cloned()
        .collect::<Vec<_>>();
    // 最初の子表の上端より上の文字片は子表の前、それ以外は子表の後に置く。
    let child_top = nested_tables
        .iter()
        .map(|child| normalized_bounds(child.bounds).y1)
        .fold(f32::NEG_INFINITY, f32::max);
    let (above, below): (Vec<_>, Vec<_>) = fragments
        .into_iter()
        .partition(|fragment| (fragment.bounds.y0 + fragment.bounds.y1) / 2.0 > child_top);
    let left = column_x(grid_col);
    let right = column_x(grid_col + grid_span.max(1));
    let compose = |fragments: &[table_cell_text::CellFragment]| {
        let composed = table_cell_text::compose_cell(fragments, left, right);
        let tabs = composed
            .tab_stops
            .iter()
            .map(|stops| {
                stops
                    .iter()
                    .map(|stop| (stop.x - left, stop.right))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        (composed.text, tabs)
    };
    Some(NestedCellText {
        above: compose(&above),
        below: compose(&below),
    })
}

/// 子表を含むセルの、子表より上と下の本文（段落ごとのタブ位置つき）。
struct NestedCellText {
    /// 子表より上の本文。
    above: (String, CellParagraphTabs),
    /// 子表より下の本文。
    below: (String, CellParagraphTabs),
}

/// 親セル文字列から子表に割り当て済みの文字を除く。
fn cell_text_without_nested_tables(text: &str, nested_tables: &[&PositionedTable]) -> String {
    let mut out = text.to_string();
    for nested_table in nested_tables {
        for nested_text in nested_table.table.rows.iter().flatten() {
            let nested_text = nested_text.trim();
            if !nested_text.is_empty() {
                out = out.replace(nested_text, "");
            }
        }
    }
    out.split('\n')
        .map(|line| {
            line.split('\t')
                .map(|segment| segment.split_whitespace().collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// セル罫線フラグを DOCX の `w:tcBorders` へ変換する。
fn table_cell_borders(borders: DocxCellBorders) -> TableCellBorders {
    let mut out = TableCellBorders::with_empty();
    for (position, present, dashed) in [
        (TableCellBorderPosition::Top, borders.top, borders.dashed[0]),
        (
            TableCellBorderPosition::Left,
            borders.left,
            borders.dashed[1],
        ),
        (
            TableCellBorderPosition::Bottom,
            borders.bottom,
            borders.dashed[2],
        ),
        (
            TableCellBorderPosition::Right,
            borders.right,
            borders.dashed[3],
        ),
    ] {
        let border = table_cell_border_with_size(position, present, borders.size_eighths_pt);
        out = out.set(if present && dashed {
            border.border_type(BorderType::Dashed)
        } else {
            border
        });
    }
    out
}

/// 1 辺分の DOCX セル罫線を指定 size で返す。
fn table_cell_border_with_size(
    position: TableCellBorderPosition,
    present: bool,
    size_eighths_pt: usize,
) -> TableCellBorder {
    let border = TableCellBorder::new(position)
        .size(size_eighths_pt)
        .color("000000");
    if present {
        border
    } else {
        border.border_type(BorderType::Nil)
    }
}

/// PDF 罫線の代表線幅から DOCX border size（1/8pt 単位）を推定する。
fn table_border_size_for_pdf_rule(stroke_width: f32) -> usize {
    if stroke_width.is_finite() && stroke_width > 0.0 {
        ((stroke_width * 8.0).round() as usize).clamp(2, 96)
    } else {
        2
    }
}

/// PDF から抽出した要素を位置付き `OcrTable` 群へ変換する。
fn infer_positioned_tables_from_pdf_graphics(parsed: &ParsedDocument) -> Vec<PositionedTable> {
    let mut tables = infer_ruled_positioned_tables(parsed);
    // 罫線から作った表にかからない範囲で、罫線のない表を文字の並びから補う。
    let ruleless = ruleless_table::infer_ruleless_tables(parsed, &tables);
    if !ruleless.is_empty() {
        tables.extend(ruleless);
        tables.sort_by(|a, b| {
            a.page
                .cmp(&b.page)
                .then(b.bounds.y1.total_cmp(&a.bounds.y1))
        });
    }
    tables
}

/// 罫線（と罫線のない頁の文字格子）から表を推定する。
fn infer_ruled_positioned_tables(parsed: &ParsedDocument) -> Vec<PositionedTable> {
    let mut graphic_tables = parsed
        .graphics
        .iter()
        .map(|graphic| graphic.page)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .flat_map(|page| infer_positioned_tables_from_pdf_graphics_page(parsed, page))
        .collect::<Vec<_>>();
    if !graphic_tables.is_empty() {
        if graphic_tables.len() == 1 {
            let text_tables = infer_positioned_tables_from_text_grid(parsed);
            if let Some(best_text_table) = text_tables
                .iter()
                .max_by_key(|table| ocr_table_non_empty_cell_count(&table.table))
            {
                let has_graphic_span_model = graphic_tables
                    .iter()
                    .any(|table| table.docx_table.is_some());
                let max_graphic_non_empty = graphic_tables
                    .iter()
                    .map(|table| ocr_table_non_empty_cell_count(&table.table))
                    .max()
                    .unwrap_or(0);
                let best_text_cols = best_text_table.table.col_count().unwrap_or(0);
                let max_graphic_cols = graphic_tables
                    .iter()
                    .filter_map(|table| table.table.col_count())
                    .max()
                    .unwrap_or(0);
                let text_non_empty = ocr_table_non_empty_cell_count(&best_text_table.table);
                let text_gain = text_non_empty.saturating_sub(max_graphic_non_empty);
                if !has_graphic_span_model && best_text_cols <= max_graphic_cols && text_gain > 2 {
                    return text_tables;
                }
            }
        }
        graphic_tables.sort_by(|a, b| {
            a.page
                .cmp(&b.page)
                .then(b.bounds.y1.total_cmp(&a.bounds.y1))
        });
        return group_side_by_side_tables(parsed, attach_nested_positioned_tables(graphic_tables));
    }
    // 罫線のないページの文字格子の表は、罫線を付けずに出す。
    infer_positioned_tables_from_text_grid(parsed)
        .into_iter()
        .map(borderless_text_table)
        .collect()
}

/// 同じ高さに横へ並ぶ表を、枠なし 1 行の外側表の各セルへ入れ子にしてまとめる。
///
/// DOCX の本文は上から下へ流れるため、横に並んだ表を別々に出すと縦に積まれてしまう。
/// 縦方向の範囲が互いに半分以上重なり、横方向には重ならない表どうしを 1 つにまとめる。
fn group_side_by_side_tables(
    parsed: &ParsedDocument,
    tables: Vec<PositionedTable>,
) -> Vec<PositionedTable> {
    // 段を区切る縦罫線の左右にある表は、別の段の表なのでまとめない（段の順に読む）。
    let mut dividers: BTreeMap<usize, Vec<ParsedBounds>> = BTreeMap::new();
    for page in tables
        .iter()
        .map(|table| table.page)
        .collect::<BTreeSet<_>>()
    {
        let mut rules = column_bands::partial_band_rules(parsed, page, &[]);
        rules.extend(
            column_bands::column_band_edges(parsed, page, &[])
                .into_iter()
                .map(|x| ParsedBounds {
                    x0: x,
                    y0: f32::NEG_INFINITY,
                    x1: x,
                    y1: f32::INFINITY,
                }),
        );
        dividers.insert(page, rules);
    }
    let divided = |a: ParsedBounds, b: ParsedBounds, page: usize| {
        let (left, right) = if a.x0 <= b.x0 { (a, b) } else { (b, a) };
        dividers.get(&page).is_some_and(|rules| {
            rules.iter().any(|rule| {
                rule.x0 >= left.x1 - 2.0
                    && rule.x0 <= right.x0 + 2.0
                    && rule.y0 < left.y1.max(right.y1)
                    && rule.y1 > left.y0.min(right.y0)
            })
        })
    };
    let overlaps = |a: &PositionedTable, b: &PositionedTable| {
        let (a_bounds, b_bounds) = (normalized_bounds(a.bounds), normalized_bounds(b.bounds));
        let vertical = a_bounds.y1.min(b_bounds.y1) - a_bounds.y0.max(b_bounds.y0);
        let smaller = (a_bounds.y1 - a_bounds.y0).min(b_bounds.y1 - b_bounds.y0);
        let horizontal = a_bounds.x1.min(b_bounds.x1) - a_bounds.x0.max(b_bounds.x0);
        a.page == b.page
            && smaller > 0.0
            && vertical >= smaller * 0.5
            && horizontal < 0.0
            && !divided(a_bounds, b_bounds, a.page)
    };
    let mut groups: Vec<Vec<PositionedTable>> = Vec::new();
    for table in tables {
        match groups
            .iter_mut()
            .find(|group| group.iter().any(|member| overlaps(member, &table)))
        {
            Some(group) => group.push(table),
            None => groups.push(vec![table]),
        }
    }
    groups
        .into_iter()
        .map(|mut group| {
            if group.len() < 2 {
                return group.pop().expect("group is not empty");
            }
            group.sort_by(|a, b| a.bounds.x0.total_cmp(&b.bounds.x0));
            side_by_side_container(group)
        })
        .collect()
}

/// 横並びの表をまとめた外側の表（本文を持たず子表だけを持つ 1 行の表）か判定する。
fn is_side_by_side_container(table: &PositionedTable) -> bool {
    !table.nested_tables.is_empty()
        && table.table.row_count() == 1
        && table
            .table
            .rows
            .iter()
            .flatten()
            .all(|cell| cell.is_empty())
}

/// 横並びの表を子表として持つ、枠なし 1 行の外側表を作る。
fn side_by_side_container(members: Vec<PositionedTable>) -> PositionedTable {
    let bounds = members
        .iter()
        .map(|table| normalized_bounds(table.bounds))
        .reduce(|a, b| ParsedBounds {
            x0: a.x0.min(b.x0),
            y0: a.y0.min(b.y0),
            x1: a.x1.max(b.x1),
            y1: a.y1.max(b.y1),
        })
        .expect("members are not empty");
    let mut column_widths_twips = Vec::new();
    let mut right = bounds.x0;
    for table in &members {
        let table_bounds = normalized_bounds(table.bounds);
        if table_bounds.x0 > right + 0.5 {
            column_widths_twips.extend(pdf_points_to_twips(table_bounds.x0 - right));
        }
        column_widths_twips.extend(pdf_points_to_twips(table_bounds.x1 - table_bounds.x0));
        right = table_bounds.x1;
    }
    let cols = column_widths_twips.len();
    let no_borders = DocxCellBorders {
        top: false,
        left: false,
        bottom: false,
        right: false,
        size_eighths_pt: 0,
        dashed: [false; 4],
    };
    let row = (0..cols)
        .map(|_| DocxTableCellModel {
            text: String::new(),
            underlined: false,
            alignment: ParagraphAlignment::Left,
            vertical_alignment: None,
            grid_span: 1,
            vertical_merge: None,
            width_twips: None,
            borders: Some(no_borders),
        })
        .collect::<Vec<_>>();
    PositionedTable {
        page: members[0].page,
        bounds,
        stroke_width: 0.0,
        font_name: None,
        font_embedded: false,
        font_size: None,
        column_widths_twips,
        table: OcrTable::new(vec![vec![String::new(); cols]]),
        cell_underlines: vec![vec![false; cols]],
        cell_alignments: vec![vec![ParagraphAlignment::Left; cols]],
        cell_vertical_alignments: vec![vec![Some(CellVerticalAlignment::Top); cols]],
        docx_table: Some(DocxTableModel { rows: vec![row] }),
        nested_tables: members,
        row_heights_pt: Vec::new(),
        row_line_counts: Vec::new(),
        // 外側の表のセル余白で子表の幅が縮まないよう、余白は 0 にする。
        cell_padding_pt: Some(0.0),
        cell_font_sizes: Vec::new(),
        cell_tab_stops: Vec::new(),
        cell_fragments: Vec::new(),
        cell_shading: Vec::new(),
    }
}

/// 包含された表を親表の子表として組み直す。
fn attach_nested_positioned_tables(tables: Vec<PositionedTable>) -> Vec<PositionedTable> {
    let mut parent_for = vec![None::<usize>; tables.len()];
    for child_index in 0..tables.len() {
        parent_for[child_index] = tables
            .iter()
            .enumerate()
            .filter(|(parent_index, parent)| {
                *parent_index != child_index
                    && parent.page == tables[child_index].page
                    && positioned_table_can_nest_child(parent, &tables[child_index])
            })
            .min_by(|(_, a), (_, b)| {
                table_bounds_area(a.bounds).total_cmp(&table_bounds_area(b.bounds))
            })
            .map(|(parent_index, _)| parent_index);
    }

    let mut children = vec![Vec::<usize>::new(); tables.len()];
    for (child_index, parent_index) in parent_for.iter().copied().enumerate() {
        if let Some(parent_index) = parent_index {
            children[parent_index].push(child_index);
        }
    }

    (0..tables.len())
        .filter(|index| parent_for[*index].is_none())
        .map(|index| positioned_table_tree(index, &tables, &children))
        .collect()
}

/// 指定 index の表と子孫表を再帰的に組み立てる。
fn positioned_table_tree(
    index: usize,
    tables: &[PositionedTable],
    children: &[Vec<usize>],
) -> PositionedTable {
    let mut table = tables[index].clone();
    table.nested_tables = children[index]
        .iter()
        .copied()
        .map(|child_index| positioned_table_tree(child_index, tables, children))
        .collect();
    table
}

/// 親表 bounds が子表候補を含むか判定する。
fn table_bounds_contain_nested_candidate(parent: ParsedBounds, child: ParsedBounds) -> bool {
    let parent = normalized_bounds(parent);
    let child = normalized_bounds(child);
    let parent_area = table_bounds_area(parent);
    let child_area = table_bounds_area(child);
    if child_area <= 0.0 || parent_area <= child_area * 1.2 {
        return false;
    }
    let center_x = (child.x0 + child.x1) / 2.0;
    let center_y = (child.y0 + child.y1) / 2.0;
    center_x >= parent.x0 - 2.0
        && center_x <= parent.x1 + 2.0
        && center_y >= parent.y0 - 2.0
        && center_y <= parent.y1 + 2.0
        && child.x0 >= parent.x0 - 3.0
        && child.y0 >= parent.y0 - 3.0
        && child.x1 <= parent.x1 + 3.0
        && child.y1 <= parent.y1 + 3.0
}

/// 子表が親表全体ではなく、親表の単一セルに収まるかを含めて判定する。
fn positioned_table_can_nest_child(parent: &PositionedTable, child: &PositionedTable) -> bool {
    table_bounds_contain_nested_candidate(parent.bounds, child.bounds)
        && positioned_table_child_fits_in_single_cell(parent, child.bounds)
}

/// 子表 bounds が親表の単一列内に収まるか判定する。
fn positioned_table_child_fits_in_single_cell(
    parent: &PositionedTable,
    child: ParsedBounds,
) -> bool {
    let Some(col_count) = parent.table.col_count() else {
        return false;
    };
    if parent.table.row_count() == 0 || col_count == 0 {
        return false;
    }
    let parent_bounds = normalized_bounds(parent.bounds);
    let child = normalized_bounds(child);
    let width = (parent_bounds.x1 - parent_bounds.x0) / col_count as f32;
    if child.y0 < parent_bounds.y0 - 3.0 || child.y1 > parent_bounds.y1 + 3.0 {
        return false;
    }
    for col in 0..col_count {
        let x0 = parent_bounds.x0 + width * col as f32;
        let x1 = parent_bounds.x0 + width * (col + 1) as f32;
        if child.x0 >= x0 - 3.0 && child.x1 <= x1 + 3.0 {
            return true;
        }
    }
    false
}

/// 表 bounds の面積を返す。
fn table_bounds_area(bounds: ParsedBounds) -> f32 {
    let bounds = normalized_bounds(bounds);
    (bounds.x1 - bounds.x0).abs() * (bounds.y1 - bounds.y0).abs()
}

/// PDF から抽出した罫線 graphics を矩形の `OcrTable` へ変換する。
pub fn infer_ocr_tables_from_pdf_graphics(parsed: &ParsedDocument) -> Vec<OcrTable> {
    infer_positioned_tables_from_pdf_graphics(parsed)
        .into_iter()
        .map(|table| table.table)
        .collect()
}

/// PDF から抽出した罫線 graphics を代表矩形の `OcrTable` へ変換する。
pub fn infer_ocr_table_from_pdf_graphics(parsed: &ParsedDocument) -> Option<OcrTable> {
    infer_positioned_tables_from_pdf_graphics(parsed)
        .into_iter()
        .max_by_key(|table| {
            let non_empty = table
                .table
                .rows
                .iter()
                .flatten()
                .filter(|cell| !cell.trim().is_empty())
                .count();
            (
                non_empty,
                table.table.row_count(),
                table.table.col_count().unwrap_or(0),
            )
        })
        .map(|table| table.table)
}

/// 指定 page の PDF 罫線 graphics を位置付き `OcrTable` 群へ変換する。
fn infer_positioned_tables_from_pdf_graphics_page(
    parsed: &ParsedDocument,
    page: usize,
) -> Vec<PositionedTable> {
    // 上下の枠・表が共有する外側の縦罫線は、表ごとに切り分けてから連結成分を求める。
    let page_graphics = stacked_rules::split_shared_vertical_rules(
        &parsed
            .graphics
            .iter()
            .filter(|graphic| graphic.page == page)
            .cloned()
            .collect::<Vec<_>>(),
    );
    let loose_component_candidates =
        positioned_table_candidates_from_graphic_components(parsed, page, &page_graphics, 2.0);
    let strict_component_candidates =
        positioned_table_candidates_from_graphic_components(parsed, page, &page_graphics, 0.0);
    let loose_nested_component_tables =
        component_tables_with_nested_parents(&loose_component_candidates);
    let strict_nested_component_tables =
        component_tables_with_nested_parents(&strict_component_candidates);
    let strict_nested_score = nested_parent_gain(
        &strict_component_candidates,
        &strict_nested_component_tables,
    ) + nested_candidate_pair_count(&strict_component_candidates);
    let loose_nested_score =
        nested_parent_gain(&loose_component_candidates, &loose_nested_component_tables)
            + nested_candidate_pair_count(&loose_component_candidates);
    let component_candidates = if strict_nested_score > loose_nested_score {
        strict_component_candidates
    } else {
        loose_component_candidates
    };
    let component_tables = component_candidates
        .iter()
        .filter(|table| positioned_table_is_supported_graphic_table(table))
        .cloned()
        .collect::<Vec<_>>();
    let nested_component_tables = component_tables_with_nested_parents(&component_candidates);
    let has_nested_component_parent = nested_component_tables.len() > component_tables.len();
    if let Some(page_table) = infer_positioned_table_from_graphics(parsed, page, &page_graphics)
        && positioned_table_is_supported_graphic_table(&page_table)
    {
        let page_cols = page_table.table.col_count().unwrap_or(0);
        let max_component_cols = component_tables
            .iter()
            .filter_map(|table| table.table.col_count())
            .max()
            .unwrap_or(0);
        if component_tables.len() > 8 && page_cols >= 2 {
            return component_tables;
        }
        if component_tables.is_empty()
            || (!has_nested_component_parent
                && component_tables.len() == 1
                && page_cols > max_component_cols)
        {
            return vec![page_table];
        }
    }
    if nested_component_tables.is_empty() {
        component_tables
    } else {
        nested_component_tables
    }
}

/// 罫線連結成分ごとの表候補を抽出する。
fn positioned_table_candidates_from_graphic_components(
    parsed: &ParsedDocument,
    page: usize,
    page_graphics: &[ParsedGraphicElement],
    tolerance: f32,
) -> Vec<PositionedTable> {
    let mut candidates = Vec::new();
    for component in split_graphics_into_components_with_tolerance(page_graphics, tolerance) {
        let Some(component_table) = infer_positioned_table_from_graphics(parsed, page, &component)
        else {
            continue;
        };
        let component_table =
            positioned_table_with_cell_underlines(parsed, component_table, page_graphics);
        let edge_nested_tables = infer_edge_aligned_nested_tables_from_component(
            parsed,
            page,
            &component_table,
            &component,
            page_graphics,
        );
        if edge_nested_tables.is_empty() {
            candidates.push(component_table);
            continue;
        }
        if positioned_table_should_keep_as_nested_parent(&component_table) {
            candidates.push(component_table);
        } else {
            candidates.push(parent_container_table_from_component(&component_table));
        }
        candidates.extend(edge_nested_tables);
    }
    candidates
}

/// 子表を含む実表として、空 1 セルコンテナへ落とさず残すべき親表か判定する。
fn positioned_table_should_keep_as_nested_parent(table: &PositionedTable) -> bool {
    let Some(cols) = table.table.col_count() else {
        return false;
    };
    cols == 2
        && (2..=4).contains(&table.table.row_count())
        && positioned_table_is_supported_graphic_table(table)
}

/// 外枠辺を共有する内側表をコンポーネント内から抽出する。
fn infer_edge_aligned_nested_tables_from_component(
    parsed: &ParsedDocument,
    page: usize,
    component_table: &PositionedTable,
    component: &[ParsedGraphicElement],
    page_graphics: &[ParsedGraphicElement],
) -> Vec<PositionedTable> {
    if component_table.table.row_count() > 3 || component_table.table.col_count().unwrap_or(0) > 4 {
        return Vec::new();
    }
    let Some(component_bounds) = graphics_bounds(component) else {
        return Vec::new();
    };
    let component_bounds = normalized_bounds(component_bounds);
    let component_width = component_bounds.x1 - component_bounds.x0;
    let component_height = component_bounds.y1 - component_bounds.y0;
    if component_width <= 0.0 || component_height <= 0.0 {
        return Vec::new();
    }
    let inner_lines = component
        .iter()
        .filter(|graphic| {
            let bounds = normalized_bounds(graphic.bounds);
            let width = bounds.x1 - bounds.x0;
            let height = bounds.y1 - bounds.y0;
            (is_horizontal_line(&bounds) && width > 8.0 && width < component_width * 0.95)
                || (is_vertical_line(&bounds) && height > 8.0 && height < component_height * 0.95)
        })
        .cloned()
        .collect::<Vec<_>>();
    if inner_lines.len() < 3 {
        return Vec::new();
    }

    split_graphics_into_components_with_tolerance(&inner_lines, 2.0)
        .into_iter()
        .filter_map(|inner_component| {
            if !graphic_component_has_horizontal_and_vertical_lines(&inner_component) {
                return None;
            }
            let Some(inner_bounds) = graphics_bounds(&inner_component) else {
                return None;
            };
            let inner_bounds = normalized_bounds(inner_bounds);
            if table_bounds_area(inner_bounds) >= table_bounds_area(component_bounds) * 0.45 {
                return None;
            }
            if !bounds_are_near_any_edge(component_bounds, inner_bounds, 6.0) {
                return None;
            }
            if !table_bounds_contain_nested_candidate(component_bounds, inner_bounds) {
                return None;
            }
            let mut child_graphics = inner_component;
            add_clipped_component_edges(&mut child_graphics, component, inner_bounds);
            infer_positioned_table_from_graphics(parsed, page, &child_graphics)
                .filter(positioned_table_is_supported_graphic_table)
                .map(|table| positioned_table_with_cell_underlines(parsed, table, page_graphics))
        })
        .collect()
}

/// graphic component が水平・垂直両方の実罫線を持つか判定する。
fn graphic_component_has_horizontal_and_vertical_lines(component: &[ParsedGraphicElement]) -> bool {
    let has_horizontal = component
        .iter()
        .map(|graphic| normalized_bounds(graphic.bounds))
        .any(|bounds| is_horizontal_line(&bounds));
    let has_vertical = component
        .iter()
        .map(|graphic| normalized_bounds(graphic.bounds))
        .any(|bounds| is_vertical_line(&bounds));
    has_horizontal && has_vertical
}

/// child bounds が parent のいずれかの辺に近いか判定する。
fn bounds_are_near_any_edge(parent: ParsedBounds, child: ParsedBounds, tolerance: f32) -> bool {
    (child.x0 - parent.x0).abs() <= tolerance
        || (child.x1 - parent.x1).abs() <= tolerance
        || (child.y0 - parent.y0).abs() <= tolerance
        || (child.y1 - parent.y1).abs() <= tolerance
}

/// 子表で共有される親罫線を子表 bounds に切り出して補う。
fn add_clipped_component_edges(
    child_graphics: &mut Vec<ParsedGraphicElement>,
    component: &[ParsedGraphicElement],
    child_bounds: ParsedBounds,
) {
    let stroke_width = component
        .iter()
        .map(|graphic| graphic.stroke_width)
        .filter(|width| width.is_finite() && *width > 0.0)
        .fold(0.5f32, f32::min);
    for x in [child_bounds.x0, child_bounds.x1] {
        if !child_graphics.iter().any(|graphic| {
            let bounds = normalized_bounds(graphic.bounds);
            is_vertical_line(&bounds) && (bounds.x0 - x).abs() <= 1.5
        }) {
            child_graphics.push(ParsedGraphicElement {
                page: component.first().map(|graphic| graphic.page).unwrap_or(0),
                bounds: ParsedBounds {
                    x0: x,
                    y0: child_bounds.y0,
                    x1: x,
                    y1: child_bounds.y1,
                },
                stroke_width,
                dashed: false,
            });
        }
    }
    for y in [child_bounds.y0, child_bounds.y1] {
        if !child_graphics.iter().any(|graphic| {
            let bounds = normalized_bounds(graphic.bounds);
            is_horizontal_line(&bounds) && (bounds.y0 - y).abs() <= 1.5
        }) {
            child_graphics.push(ParsedGraphicElement {
                page: component.first().map(|graphic| graphic.page).unwrap_or(0),
                bounds: ParsedBounds {
                    x0: child_bounds.x0,
                    y0: y,
                    x1: child_bounds.x1,
                    y1: y,
                },
                stroke_width,
                dashed: false,
            });
        }
    }
}

/// コンポーネント全体を 1 セルの親表として作り直す。
fn parent_container_table_from_component(table: &PositionedTable) -> PositionedTable {
    let width_twips = pdf_points_to_twips(
        normalized_bounds(table.bounds).x1 - normalized_bounds(table.bounds).x0,
    )
    .into_iter()
    .collect::<Vec<_>>();
    PositionedTable {
        page: table.page,
        bounds: table.bounds,
        stroke_width: table.stroke_width,
        font_name: None,
        font_embedded: false,
        font_size: None,
        column_widths_twips: width_twips,
        table: OcrTable::new(vec![vec![String::new()]]),
        cell_underlines: vec![vec![false]],
        cell_alignments: vec![vec![ParagraphAlignment::Left]],
        cell_vertical_alignments: Vec::new(),
        docx_table: None,
        nested_tables: Vec::new(),
        row_heights_pt: Vec::new(),
        row_line_counts: Vec::new(),
        cell_padding_pt: None,
        cell_font_sizes: Vec::new(),
        cell_tab_stops: Vec::new(),
        cell_fragments: Vec::new(),
        cell_shading: Vec::new(),
    }
}

/// ページ全体の graphics を使って表セル内テキスト下線を補う。
fn positioned_table_with_cell_underlines(
    parsed: &ParsedDocument,
    mut table: PositionedTable,
    graphics: &[ParsedGraphicElement],
) -> PositionedTable {
    let Some(col_count) = table.table.col_count() else {
        return table;
    };
    let row_count = table.table.row_count();
    if row_count == 0 || col_count == 0 {
        return table;
    }
    let mut cell_underlines = if table.cell_underlines.len() == row_count
        && table
            .cell_underlines
            .iter()
            .all(|row| row.len() == col_count)
    {
        table.cell_underlines.clone()
    } else {
        vec![vec![false; col_count]; row_count]
    };
    let bounds = normalized_bounds(table.bounds);
    // 列幅・行高が分かる場合は実際の境界、分からない場合は等分の境界を使う。
    let xs = if table.column_widths_twips.len() == col_count {
        std::iter::once(bounds.x0)
            .chain(
                table
                    .column_widths_twips
                    .iter()
                    .scan(bounds.x0, |x, twips| {
                        *x += *twips as f32 / 20.0;
                        Some(*x)
                    }),
            )
            .collect::<Vec<_>>()
    } else {
        (0..=col_count)
            .map(|col| bounds.x0 + (bounds.x1 - bounds.x0) * col as f32 / col_count as f32)
            .collect::<Vec<_>>()
    };
    let ys = if table.row_heights_pt.len() == row_count {
        let mut ys = std::iter::once(bounds.y1)
            .chain(table.row_heights_pt.iter().scan(bounds.y1, |y, height| {
                *y -= height;
                Some(*y)
            }))
            .collect::<Vec<_>>();
        ys.reverse();
        ys
    } else {
        (0..=row_count)
            .map(|row| bounds.y0 + (bounds.y1 - bounds.y0) * row as f32 / row_count as f32)
            .collect::<Vec<_>>()
    };
    let x_edges = float_edges_to_i32(&xs);
    let y_edges = float_edges_to_i32(&ys);
    let underline_graphics = graphics_without_row_rules(graphics, &ys);
    // セルの字の幅のうち下線の付いた字の幅（下線付き, 全体）。セルの一部の字（ラベルの右の
    // 数値など）だけに引いた下線で、セル全体に下線を付けない。
    let mut widths = vec![vec![(0.0f32, 0.0f32); col_count]; row_count];
    for text in parsed
        .text_elements
        .iter()
        .filter(|text| text.page == table.page && !text.text.trim().is_empty())
    {
        let underlined = parsed_text_has_underline(text, &underline_graphics);
        let center_x = (text.bounds.x0 + text.bounds.x1) / 2.0;
        let center_y = (text.bounds.y0 + text.bounds.y1) / 2.0;
        let Some(col) = column_index_for_x(center_x, &x_edges) else {
            continue;
        };
        let Some(row_from_bottom) = row_index_for_y(center_y, &y_edges) else {
            continue;
        };
        let row = row_count.saturating_sub(1 + row_from_bottom);
        if let Some(width) = widths.get_mut(row).and_then(|row| row.get_mut(col)) {
            let text_width = (text.bounds.x1 - text.bounds.x0).abs();
            width.1 += text_width;
            if underlined {
                width.0 += text_width;
            }
        }
    }
    for (row, row_widths) in widths.iter().enumerate() {
        for (col, (underlined, total)) in row_widths.iter().enumerate() {
            if *total > 0.0
                && *underlined >= 0.8 * total
                && let Some(cell) = cell_underlines
                    .get_mut(row)
                    .and_then(|row| row.get_mut(col))
            {
                *cell = true;
            }
        }
    }
    table.cell_underlines = cell_underlines;
    table
}

/// PDF 座標 edge を既存 grid helper 用に丸める。
fn float_edges_to_i32(edges: &[f32]) -> Vec<i32> {
    edges.iter().map(|edge| edge.round() as i32).collect()
}

/// graphics 群の外接 bounds を返す。
fn graphics_bounds(graphics: &[ParsedGraphicElement]) -> Option<ParsedBounds> {
    let mut bounds = graphics
        .iter()
        .map(|graphic| normalized_bounds(graphic.bounds));
    let first = bounds.next()?;
    Some(bounds.fold(first, |acc, bounds| ParsedBounds {
        x0: acc.x0.min(bounds.x0),
        y0: acc.y0.min(bounds.y0),
        x1: acc.x1.max(bounds.x1),
        y1: acc.y1.max(bounds.y1),
    }))
}

/// 親として追加保持されたネスト外枠候補数を返す。
fn nested_parent_gain(
    candidates: &[PositionedTable],
    nested_component_tables: &[PositionedTable],
) -> usize {
    let supported_count = candidates
        .iter()
        .filter(|table| positioned_table_is_supported_graphic_table(table))
        .count();
    nested_component_tables
        .len()
        .saturating_sub(supported_count)
}

/// 候補同士の包含関係数を返す。
fn nested_candidate_pair_count(candidates: &[PositionedTable]) -> usize {
    candidates
        .iter()
        .enumerate()
        .map(|(parent_index, parent)| {
            candidates
                .iter()
                .enumerate()
                .filter(|(child_index, child)| {
                    *child_index != parent_index
                        && parent.page == child.page
                        && positioned_table_is_supported_graphic_table(parent)
                        && positioned_table_is_supported_graphic_table(child)
                        && table_bounds_contain_nested_candidate(parent.bounds, child.bounds)
                })
                .count()
        })
        .sum()
}

/// 子表を含む外枠コンポーネントを親表候補として残す。
fn component_tables_with_nested_parents(candidates: &[PositionedTable]) -> Vec<PositionedTable> {
    let mut keep = candidates
        .iter()
        .map(positioned_table_is_supported_graphic_table)
        .collect::<Vec<_>>();
    for (parent_index, parent) in candidates.iter().enumerate() {
        if keep[parent_index] || !positioned_table_is_wide_single_cell_container(parent) {
            continue;
        }
        if candidates.iter().enumerate().any(|(child_index, child)| {
            child_index != parent_index
                && parent.page == child.page
                && keep[child_index]
                && table_bounds_contain_nested_candidate(parent.bounds, child.bounds)
        }) {
            keep[parent_index] = true;
        }
    }

    candidates
        .iter()
        .zip(keep)
        .filter_map(|(table, keep)| keep.then_some(table.clone()))
        .collect()
}

/// OCR table の非空セル数を返す。
fn ocr_table_non_empty_cell_count(table: &OcrTable) -> usize {
    table
        .rows
        .iter()
        .flatten()
        .filter(|cell| !cell.trim().is_empty())
        .count()
}

/// 表として出力できる罫線グリッドか判定する。
fn positioned_table_is_supported_graphic_table(table: &PositionedTable) -> bool {
    let Some(cols) = table.table.col_count() else {
        return false;
    };
    if cols >= 2 {
        return true;
    }
    let width = normalized_bounds(table.bounds).x1 - normalized_bounds(table.bounds).x0;
    cols == 1
        && width >= WIDE_SINGLE_COLUMN_TABLE_MIN_WIDTH_PT
        && ocr_table_non_empty_cell_count(&table.table) > 0
}

/// 子表を格納できる幅を持つ 1 セル外枠か判定する。
fn positioned_table_is_wide_single_cell_container(table: &PositionedTable) -> bool {
    let Some(cols) = table.table.col_count() else {
        return false;
    };
    let width = normalized_bounds(table.bounds).x1 - normalized_bounds(table.bounds).x0;
    cols == 1 && table.table.row_count() == 1 && width >= WIDE_SINGLE_COLUMN_TABLE_MIN_WIDTH_PT
}

/// tolerance 付きで罫線 graphics の連結成分を抽出する。
fn split_graphics_into_components_with_tolerance(
    graphics: &[ParsedGraphicElement],
    tolerance: f32,
) -> Vec<Vec<ParsedGraphicElement>> {
    let mut components: Vec<Vec<ParsedGraphicElement>> = Vec::new();
    for graphic in graphics {
        let mut matching = Vec::new();
        for (index, component) in components.iter().enumerate() {
            if component
                .iter()
                .any(|other| graphics_are_connected_with_tolerance(graphic, other, tolerance))
            {
                matching.push(index);
            }
        }
        if matching.is_empty() {
            components.push(vec![graphic.clone()]);
            continue;
        }
        let first = matching[0];
        components[first].push(graphic.clone());
        for index in matching.into_iter().skip(1).rev() {
            let mut merged = components.remove(index);
            components[first].append(&mut merged);
        }
    }
    components
}

/// 2 つの graphics が tolerance 内で同じ表罫線成分に属するか判定する。
fn graphics_are_connected_with_tolerance(
    a: &ParsedGraphicElement,
    b: &ParsedGraphicElement,
    tolerance: f32,
) -> bool {
    if a.page != b.page {
        return false;
    }
    let a = normalized_bounds(a.bounds);
    let b = normalized_bounds(b.bounds);
    bounds_overlap_with_tolerance(a, b, tolerance)
}

/// 2 つの bounds が tolerance つきで交差するか判定する。
fn bounds_overlap_with_tolerance(a: ParsedBounds, b: ParsedBounds, tolerance: f32) -> bool {
    a.x0 <= b.x1 + tolerance
        && a.x1 + tolerance >= b.x0
        && a.y0 <= b.y1 + tolerance
        && a.y1 + tolerance >= b.y0
}

/// 指定 graphics 群を位置付き `OcrTable` へ変換する。
fn infer_positioned_table_from_graphics(
    parsed: &ParsedDocument,
    page: usize,
    page_graphics: &[ParsedGraphicElement],
) -> Option<PositionedTable> {
    let horizontal = page_graphics
        .iter()
        .map(|graphic| normalized_bounds(graphic.bounds))
        .filter(is_horizontal_line)
        .collect::<Vec<_>>();
    let vertical = page_graphics
        .iter()
        .map(|graphic| normalized_bounds(graphic.bounds))
        .filter(is_vertical_line)
        .collect::<Vec<_>>();
    // 縦罫線のない表は、同じ幅で揃った 3 本以上の横罫線がある場合だけ扱う（列は文字から求める）。
    let horizontal_only = vertical.is_empty() && aligned_full_width_rules(&horizontal) >= 3;
    if horizontal.is_empty() || (vertical.is_empty() && !horizontal_only) {
        return None;
    }

    let max_horizontal_len = horizontal
        .iter()
        .map(|bounds| bounds.x1 - bounds.x0)
        .fold(0.0f32, f32::max);
    let max_vertical_len = vertical
        .iter()
        .map(|bounds| bounds.y1 - bounds.y0)
        .fold(0.0f32, f32::max);
    let full_horizontal = horizontal
        .iter()
        .filter(|bounds| bounds.x1 - bounds.x0 >= max_horizontal_len * 0.8)
        .collect::<Vec<_>>();
    let full_vertical = vertical
        .iter()
        .filter(|bounds| bounds.y1 - bounds.y0 >= max_vertical_len * 0.8)
        .collect::<Vec<_>>();

    let mut xs = full_vertical
        .iter()
        .map(|bounds| bounds.x0.round() as i32)
        .collect::<Vec<_>>();
    if horizontal_only {
        let left = full_horizontal
            .iter()
            .map(|bounds| bounds.x0)
            .fold(f32::INFINITY, f32::min);
        let right = full_horizontal
            .iter()
            .map(|bounds| bounds.x1)
            .fold(f32::NEG_INFINITY, f32::max);
        xs = vec![left.round() as i32, right.round() as i32];
    }
    let mut ys = full_horizontal
        .iter()
        .map(|bounds| bounds.y0.round() as i32)
        .collect::<Vec<_>>();

    xs.sort();
    xs.dedup();
    ys.sort();
    ys.dedup();
    // セルごとに枠を描く表や二重線は、間隔の狭い平行線を 1 本の境界にまとめる。
    xs = merge_close_edges(&xs);
    ys = merge_close_edges(&ys);
    // 上下（左右）の外枠を描かない表は、揃った縦（横）罫線の端を外枠の位置として補う。
    let open_ys = open_table_edges(&full_vertical, &full_horizontal, &ys, true);
    let open_xs = open_table_edges(&full_horizontal, &full_vertical, &xs, false);
    let has_open_edges = horizontal_only || open_ys.len() > ys.len() || open_xs.len() > xs.len();
    ys = open_ys;
    xs = open_xs;
    if xs.len() < 2
        && let Some(augmented_xs) =
            synthesize_xs_from_text_columns_and_graphics(parsed, page, &page_graphics)
    {
        xs = augmented_xs;
    }
    if ys.len() < 2
        && let Some(augmented_ys) =
            synthesize_ys_from_text_rows_and_graphics(parsed, page, &page_graphics)
    {
        ys = augmented_ys;
    }
    xs = merge_close_edges(&augment_xs_from_partial_verticals(&xs, &ys, &vertical));
    ys = merge_close_edges(&augment_ys_from_partial_horizontals(&xs, &ys, &horizontal));
    let mut text_augmented_columns = false;
    // 内側の縦罫線で列が確定している表は、文字の隙間から列を増やさない。
    if horizontal_only
        && ys.len() >= 2
        && let Some(projected_xs) = text_projection_columns(parsed, page, &xs, &ys)
    {
        text_augmented_columns = true;
        xs = projected_xs;
    } else if xs.len() == 2
        && ys.len() >= 2
        && let Some(augmented_xs) = augment_xs_from_text_columns(parsed, page, &xs, &ys)
    {
        text_augmented_columns = augmented_xs != xs;
        xs = augmented_xs;
    }
    let allow_top_origin_text_y = grid_accepts_top_origin_text_y(parsed, page, &ys)
        && !text_directly_intersects_grid(parsed, page, &xs, &ys);
    if xs.len() < 2
        || ys.len() < 2
        || (!has_grid_lines(&page_graphics, &xs, &ys)
            && !has_partial_grid_lines(&page_graphics, &xs, &ys))
            && !has_span_grid_lines(&page_graphics, &xs, &ys)
            && !has_relaxed_span_grid_lines(&page_graphics, &xs, &ys)
            && !has_borderless_grid_lines(&page_graphics, &xs, &ys)
            && !(has_open_edges && has_open_grid_lines(&page_graphics, &xs, &ys))
            && !has_covered_grid_lines(&page_graphics, &xs, &ys)
    {
        return None;
    }
    // 横罫線のない背の高い行に並んだ項目の行は、文字の行ごとの行に分ける。
    // 分けた境界には罫線を引かず、セルの結合だけを止める（線幅 0 の見えない境界として渡す）。
    let mut model_graphics = page_graphics.to_vec();
    if !text_augmented_columns {
        let split_ys = text_line_rows::split_rows_at_text_lines(parsed, page, &xs, &ys);
        if let (Some(left), Some(right)) = (xs.first(), xs.last()) {
            for y in split_ys.iter().filter(|y| !ys.contains(y)) {
                model_graphics.push(ParsedGraphicElement {
                    page,
                    bounds: ParsedBounds {
                        x0: *left as f32,
                        y0: *y as f32,
                        x1: *right as f32,
                        y1: *y as f32,
                    },
                    stroke_width: 0.0,
                    dashed: false,
                });
            }
        }
        ys = split_ys;
    }

    let rows = ys.len() - 1;
    let cols = xs.len() - 1;
    let mut cells = vec![vec![String::new(); cols]; rows];
    let mut cell_underlines = vec![vec![false; cols]; rows];
    let mut cell_text_bounds = vec![vec![None::<ParsedBounds>; cols]; rows];
    let mut assigned_font_names = Vec::new();
    let mut assigned_font_sizes = Vec::new();
    let mut cell_fragments = vec![vec![Vec::<table_cell_text::CellFragment>::new(); cols]; rows];
    // マスごとの（下線のある文字片の数, 文字片の数）。
    let mut underline_counts = vec![vec![(0usize, 0usize); cols]; rows];
    let interior_xs = xs[1..xs.len() - 1]
        .iter()
        .map(|x| *x as f32)
        .collect::<Vec<_>>();
    let (_, vertical_rules) = grid_rules_from_graphics(page_graphics);
    let underline_graphics = graphics_without_row_rules(
        page_graphics,
        &ys.iter().map(|y| *y as f32).collect::<Vec<_>>(),
    );
    for text in parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page && !text.text.trim().is_empty())
    {
        let center_y = (text.bounds.y0 + text.bounds.y1) / 2.0;
        let Some((row, _)) =
            row_index_for_text_y(parsed, page, center_y, &ys, allow_top_origin_text_y)
        else {
            continue;
        };
        let fragment = table_cell_text::CellFragment {
            bounds: normalized_bounds(text.bounds),
            text: text.text.clone(),
            font_size: text.font_size,
        };
        // 列の罫線をまたいで 1 要素にまとまった文字列は、その高さで実際に縦罫線が引かれた
        // 境界ごとに分けてから割り当てる（罫線のない境界は結合セルの本文として残す）。
        let ruled_xs = interior_xs
            .iter()
            .copied()
            .filter(|x| {
                table_grid::is_ruled(&vertical_rules, *x, fragment.bounds.y0, fragment.bounds.y1)
            })
            .collect::<Vec<_>>();
        let mut assigned = false;
        for piece in table_cell_text::split_across_boundaries(&fragment, &ruled_xs) {
            let center_x = (piece.bounds.x0 + piece.bounds.x1) / 2.0;
            let Some(col) = column_index_for_x(center_x, &xs) else {
                continue;
            };
            let bounds = piece.bounds;
            cell_fragments[row][col].push(piece);
            underline_counts[row][col].1 += 1;
            if parsed_text_has_underline(text, &underline_graphics) {
                underline_counts[row][col].0 += 1;
            }
            cell_text_bounds[row][col] = Some(match cell_text_bounds[row][col] {
                Some(existing) => ParsedBounds {
                    x0: existing.x0.min(bounds.x0),
                    y0: existing.y0.min(bounds.y0),
                    x1: existing.x1.max(bounds.x1),
                    y1: existing.y1.max(bounds.y1),
                },
                None => bounds,
            });
            assigned = true;
        }
        if !assigned {
            continue;
        }
        if let Some(font_name) = text.font_name.as_deref() {
            assigned_font_names.push(font_name.to_string());
        }
        assigned_font_sizes.push(text.font_size);
    }
    // 一部の値だけに引かれた傍線でセル全体を下線にしないよう、ほぼ全ての文字片に
    // 下線がある場合だけセルの下線とする。
    for (row, counts) in underline_counts.iter().enumerate() {
        for (col, (underlined, total)) in counts.iter().enumerate() {
            cell_underlines[row][col] = *total > 0 && underlined * 5 >= total * 4;
        }
    }
    let cell_font_sizes = cell_fragments
        .iter()
        .map(|row| {
            row.iter()
                .map(|fragments| {
                    let mut sizes = fragments
                        .iter()
                        .map(|fragment| fragment.font_size)
                        .filter(|size| size.is_finite() && (4.0..=96.0).contains(size))
                        .collect::<Vec<_>>();
                    sizes.sort_by(f32::total_cmp);
                    sizes.get(sizes.len() / 2).copied()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut row_line_counts = vec![0usize; rows];
    // 字の高さに対して余白の少ない行（行の高さが字の高さの 1.6 倍未満）は、代替フォントの
    // 行の高さで字の上下が切れるため、固定せず内容に合わせて伸ばせるようにする。
    for (row_index, row) in cell_text_bounds.iter().enumerate() {
        let glyph_height = row
            .iter()
            .flatten()
            .map(|bounds| bounds.y1 - bounds.y0)
            .fold(0.0f32, f32::max);
        let row_height = (ys[rows - row_index] - ys[rows - row_index - 1]) as f32;
        if glyph_height > 0.0 && row_height < 1.6 * glyph_height {
            row_line_counts[row_index] = row_line_counts[row_index].max(2);
        }
    }
    // 文字の行ごとに分けた行は、PDF の行間が詰まっているので高さを固定しない。
    if model_graphics.len() > page_graphics.len() {
        for count in &mut row_line_counts {
            *count = (*count).max(2);
        }
    }
    // 文字がセル幅の 85% を超える行は、代替フォントの字幅で折り返すと固定の行高では
    // 2 行目が隠れるため、行数を 2 として扱い内容に合わせて伸ばせるようにする。
    for (row_index, row) in cell_text_bounds.iter().enumerate() {
        for (col, bounds) in row.iter().enumerate() {
            let Some(bounds) = bounds else {
                continue;
            };
            let width = (xs[col + 1] - xs[col]) as f32;
            if width > 0.0 && bounds.x1 - bounds.x0 > 0.85 * width {
                row_line_counts[row_index] = row_line_counts[row_index].max(2);
            }
        }
    }
    let mut cell_tab_stops = vec![vec![Vec::new(); cols]; rows];
    for (row_index, row) in cell_fragments.iter().enumerate() {
        for (col, fragments) in row.iter().enumerate() {
            let composed =
                table_cell_text::compose_cell(fragments, xs[col] as f32, xs[col + 1] as f32);
            cell_tab_stops[row_index][col] = composed
                .tab_stops
                .iter()
                .map(|stops| {
                    stops
                        .iter()
                        .map(|stop| (stop.x - xs[col] as f32, stop.right))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            cells[row_index][col] = composed.text;
            row_line_counts[row_index] =
                row_line_counts[row_index].max(table_cell_text::cell_line_count(fragments));
        }
    }
    let row_heights_pt = ys
        .windows(2)
        .rev()
        .map(|pair| (pair[1] - pair[0]) as f32)
        .collect::<Vec<_>>();
    // 各マスの文字範囲とマスの位置から、セル内の横配置と縦位置を推定する。
    let (cell_alignments, cell_vertical_alignments): (Vec<Vec<_>>, Vec<Vec<_>>) = cell_text_bounds
        .iter()
        .enumerate()
        .map(|(row, row_bounds)| {
            row_bounds
                .iter()
                .enumerate()
                .map(|(col, bounds)| {
                    let cell = ParsedBounds {
                        x0: xs[col] as f32,
                        y0: ys[rows - row - 1] as f32,
                        x1: xs[col + 1] as f32,
                        y1: ys[rows - row] as f32,
                    };
                    bounds
                        .map(|bounds| table_cell_text::infer_cell_alignment(bounds, cell))
                        .unwrap_or((ParagraphAlignment::Left, None))
                })
                .unzip()
        })
        .unzip();
    let cell_padding_pt = cell_text_bounds
        .iter()
        .zip(cell_alignments.iter())
        .flat_map(|(bounds, alignments)| bounds.iter().zip(alignments.iter()).enumerate())
        .filter(|(_, (_, alignment))| **alignment != ParagraphAlignment::Right)
        .filter_map(|(col, (bounds, _))| bounds.map(|bounds| bounds.x0 - xs[col] as f32))
        .filter(|gap| gap.is_finite() && *gap >= 0.0)
        .reduce(f32::min);
    // マスの面積の大半を覆う塗りつぶし矩形を、そのマスの背景色にする。
    let cell_shading = (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| {
                    cell_fill_color(
                        parsed,
                        page,
                        ParsedBounds {
                            x0: xs[col] as f32,
                            y0: ys[rows - row - 1] as f32,
                            x1: xs[col + 1] as f32,
                            y1: ys[rows - row] as f32,
                        },
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // 行は既に正規PDF座標の上から下へ割り当て済みであり、paint順や数値で反転しない。
    let docx_table = if text_augmented_columns {
        None
    } else {
        docx_table_model_from_grid(
            &model_graphics,
            &xs,
            &ys,
            &cells,
            &cell_underlines,
            &cell_text_bounds,
        )
        .filter(|model| model_preserves_grid_cell_texts(model, &cells))
    };
    let table = if let Some(model) = docx_table.as_ref() {
        OcrTable::with_spans(cells, ocr_spans_from_docx_table_model(model, rows, cols))
    } else {
        OcrTable::new(cells)
    };
    let font_name = common_usable_pdf_font_name(assigned_font_names.iter().map(String::as_str));
    let font_embedded = font_name.as_deref().is_some_and(|font_name| {
        parsed
            .embedded_fonts
            .iter()
            .any(|font| font.font_name == font_name)
    });
    Some(PositionedTable {
        page,
        bounds: ParsedBounds {
            x0: *xs.first().expect("xs is not empty") as f32,
            y0: *ys.first().expect("ys is not empty") as f32,
            x1: *xs.last().expect("xs is not empty") as f32,
            y1: *ys.last().expect("ys is not empty") as f32,
        },
        stroke_width: table_rule_stroke_width(&page_graphics, &xs, &ys),
        font_name,
        font_embedded,
        font_size: common_usable_pdf_font_size(assigned_font_sizes),
        column_widths_twips: table_column_widths_twips(&xs),
        table,
        cell_underlines,
        cell_alignments,
        cell_vertical_alignments,
        docx_table,
        nested_tables: Vec::new(),
        row_heights_pt,
        row_line_counts,
        cell_padding_pt,
        cell_font_sizes,
        cell_tab_stops,
        cell_fragments,
        cell_shading,
    })
}

/// text bbox が grid 座標へ直接入るか判定する。
fn text_directly_intersects_grid(
    parsed: &ParsedDocument,
    page: usize,
    xs: &[i32],
    ys: &[i32],
) -> bool {
    parsed.text_elements.iter().any(|text| {
        if text.page != page || text.text.trim().is_empty() {
            return false;
        }
        let center_x = (text.bounds.x0 + text.bounds.x1) / 2.0;
        let center_y = (text.bounds.y0 + text.bounds.y1) / 2.0;
        column_index_for_x(center_x, xs).is_some() && row_index_for_y(center_y, ys).is_some()
    })
}

/// text y 座標から表セルの行 index を返す。
fn row_index_for_text_y(
    parsed: &ParsedDocument,
    page: usize,
    center_y: f32,
    ys: &[i32],
    allow_top_origin_text_y: bool,
) -> Option<(usize, f32)> {
    row_index_for_y(center_y, ys)
        .map(|row| (row, center_y))
        .or_else(|| {
            if !allow_top_origin_text_y {
                return None;
            }
            page_height(parsed, page).and_then(|height| {
                let grid_y = height - center_y;
                row_index_for_y(grid_y, ys).map(|row| (row, grid_y))
            })
        })
}

/// 表罫線 grid が top-origin text y の補正対象か判定する。
fn grid_accepts_top_origin_text_y(parsed: &ParsedDocument, page: usize, ys: &[i32]) -> bool {
    let Some(height) = page_height(parsed, page) else {
        return false;
    };
    ys.first()
        .map(|min_y| *min_y as f32 > height * 0.5)
        .unwrap_or(false)
}

/// 位置付き表が top-origin text y の補正対象か判定する。
fn table_accepts_top_origin_text_y(parsed: &ParsedDocument, table: &PositionedTable) -> bool {
    page_height(parsed, table.page)
        .map(|height| table.bounds.y0 > height * 0.5)
        .unwrap_or(false)
}

/// PDF の列境界を DOCX cell width（twips）へ変換する。
fn table_column_widths_twips(xs: &[i32]) -> Vec<usize> {
    xs.windows(2)
        .filter_map(|pair| pdf_points_to_twips((pair[1] - pair[0]).abs() as f32))
        .collect()
}

/// 推定表を構成する罫線から代表 stroke width を返す。
fn table_rule_stroke_width(page_graphics: &[ParsedGraphicElement], xs: &[i32], ys: &[i32]) -> f32 {
    page_graphics
        .iter()
        .filter(|graphic| {
            let b = normalized_bounds(graphic.bounds);
            let x0 = b.x0.round() as i32;
            let x1 = b.x1.round() as i32;
            let y0 = b.y0.round() as i32;
            let y1 = b.y1.round() as i32;
            (ys.contains(&y0)
                && xs.first().is_some_and(|min_x| x0 <= *min_x + 1)
                && xs.last().is_some_and(|max_x| x1 >= *max_x - 1))
                || (xs.contains(&x0)
                    && ys.first().is_some_and(|min_y| y0 <= *min_y + 1)
                    && ys.last().is_some_and(|max_y| y1 >= *max_y - 1))
        })
        .map(|graphic| graphic.stroke_width)
        .filter(|width| width.is_finite() && *width > 0.0)
        .fold(0.0f32, f32::max)
}

/// 同一セルに分割抽出された文字片を自然な区切りで追加する。
fn append_cell_fragment(cell: &mut String, fragment: &str) {
    if fragment.is_empty() {
        return;
    }
    if !cell.is_empty() && needs_space_between_fragments(cell, fragment) {
        cell.push(' ');
    }
    cell.push_str(fragment);
}

/// PDF text の直下にある短い水平線を文字下線として扱う。
fn parsed_text_has_underline(text: &ParsedTextElement, graphics: &[ParsedGraphicElement]) -> bool {
    let text_bounds = normalized_bounds(text.bounds);
    let text_width = (text_bounds.x1 - text_bounds.x0).abs().max(1.0);
    let text_height = (text_bounds.y1 - text_bounds.y0)
        .abs()
        .max(text.font_size.max(1.0));
    let x_tolerance = text_height * 0.6;
    let min_y = text_bounds.y0 - text_height * 0.7;
    let max_y = text_bounds.y0 + text_height * 0.35;
    graphics.iter().any(|graphic| {
        if graphic.page != text.page {
            return false;
        }
        let bounds = normalized_bounds(graphic.bounds);
        if !is_horizontal_line(&bounds) {
            return false;
        }
        let line_width = bounds.x1 - bounds.x0;
        if line_width < text_width * 0.35 || line_width > (text_width * 8.0).max(180.0) {
            return false;
        }
        let line_y = bounds.y0;
        let text_center_x = (text_bounds.x0 + text_bounds.x1) / 2.0;
        line_y >= min_y
            && line_y <= max_y
            && text_center_x >= bounds.x0 - x_tolerance
            && text_center_x <= bounds.x1 + x_tolerance
            && bounds.x0 <= text_bounds.x1 + x_tolerance
            && bounds.x1 >= text_bounds.x0 - x_tolerance
    })
}

/// マスの面積の 8 割以上を覆う塗りつぶし矩形の色を返す（複数あれば最も小さい矩形）。
fn cell_fill_color(parsed: &ParsedDocument, page: usize, cell: ParsedBounds) -> Option<String> {
    let area = (cell.x1 - cell.x0) * (cell.y1 - cell.y0);
    if area <= 0.0 {
        return None;
    }
    parsed
        .fills
        .iter()
        .filter(|fill| fill.page == page)
        .filter_map(|fill| {
            let bounds = normalized_bounds(fill.bounds);
            let width = bounds.x1.min(cell.x1) - bounds.x0.max(cell.x0);
            let height = bounds.y1.min(cell.y1) - bounds.y0.max(cell.y0);
            (width > 0.0 && height > 0.0 && width * height >= 0.8 * area).then(|| {
                (
                    (bounds.x1 - bounds.x0) * (bounds.y1 - bounds.y0),
                    &fill.color,
                )
            })
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, color)| color.clone())
}

/// 表の行境界に重なる横罫線を除いた graphics を返す（セル罫線を文字の下線と取り違えない）。
fn graphics_without_row_rules(
    graphics: &[ParsedGraphicElement],
    row_edges: &[f32],
) -> Vec<ParsedGraphicElement> {
    graphics
        .iter()
        .filter(|graphic| {
            let bounds = normalized_bounds(graphic.bounds);
            !(is_horizontal_line(&bounds)
                && row_edges.iter().any(|edge| (bounds.y0 - edge).abs() <= 1.5))
        })
        .cloned()
        .collect()
}

/// 2 つの text fragment の間に空白を補うべきか判定する。
fn needs_space_between_fragments(left: &str, right: &str) -> bool {
    let Some(left) = left.chars().last() else {
        return false;
    };
    let Some(right) = right.chars().next() else {
        return false;
    };
    left.is_ascii_alphanumeric() && right.is_ascii_alphanumeric()
}

/// span 適用後の model が元 grid cell の text を保持しているか判定する。
fn model_preserves_grid_cell_texts(model: &DocxTableModel, cells: &[Vec<String>]) -> bool {
    let model_text = model
        .rows
        .iter()
        .flat_map(|row| row.iter())
        .map(|cell| cell.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    cells
        .iter()
        .flatten()
        .map(|cell| cell.trim())
        .filter(|cell| !cell.is_empty())
        .all(|cell| model_text.contains(cell))
}

/// 内部 DOCX 表モデルの結合情報を公開 `OcrTable` の矩形 span grid へ変換する。
fn ocr_spans_from_docx_table_model(
    model: &DocxTableModel,
    rows_len: usize,
    cols_len: usize,
) -> Vec<Vec<OcrCellSpan>> {
    let mut spans = vec![vec![OcrCellSpan::single(); cols_len]; rows_len];
    let mut covered = vec![vec![false; cols_len]; rows_len];
    for (row_index, row) in model.rows.iter().enumerate().take(rows_len) {
        let mut col = 0usize;
        for cell in row {
            if col >= cols_len {
                break;
            }
            let col_span = cell.grid_span.max(1).min(cols_len - col);
            if !matches!(cell.vertical_merge, Some(DocxVerticalMerge::Continue)) {
                let row_span = vertical_merge_row_span(model, row_index, col, rows_len);
                let range_is_uncovered = !covered[row_index][col..col.saturating_add(col_span)]
                    .iter()
                    .any(|covered| *covered);
                if range_is_uncovered {
                    spans[row_index][col] = OcrCellSpan::new(row_span, col_span);
                    for covered_row in covered
                        .iter_mut()
                        .skip(row_index)
                        .take(row_span.min(rows_len - row_index))
                    {
                        for covered_cell in covered_row.iter_mut().skip(col).take(col_span) {
                            *covered_cell = true;
                        }
                    }
                }
            }
            col += col_span;
        }
    }
    spans
}

/// `w:vMerge restart` の開始セルから継続行数を数える。
fn vertical_merge_row_span(
    model: &DocxTableModel,
    row_index: usize,
    col: usize,
    rows_len: usize,
) -> usize {
    let mut row_span = 1usize;
    for next_row in (row_index + 1)..rows_len {
        let Some(cell) = docx_model_cell_at_column(model, next_row, col) else {
            break;
        };
        if matches!(cell.vertical_merge, Some(DocxVerticalMerge::Continue)) {
            row_span += 1;
        } else {
            break;
        }
    }
    row_span
}

/// compact な DOCX 表モデルから grid column 位置のセルを探す。
fn docx_model_cell_at_column(
    model: &DocxTableModel,
    row_index: usize,
    target_col: usize,
) -> Option<&DocxTableCellModel> {
    let row = model.rows.get(row_index)?;
    let mut col = 0usize;
    for cell in row {
        let span = cell.grid_span.max(1);
        if target_col >= col && target_col < col + span {
            return Some(cell);
        }
        col += span;
    }
    None
}

/// 外枠が欠けた表の column 境界を text cluster と部分罫線から合成する。
fn synthesize_xs_from_text_columns_and_graphics(
    parsed: &ParsedDocument,
    page: usize,
    graphics: &[ParsedGraphicElement],
) -> Option<Vec<i32>> {
    let mut centers = parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page && !text.text.trim().is_empty())
        .map(|text| (text.bounds.x0 + text.bounds.x1) / 2.0)
        .collect::<Vec<_>>();
    if centers.len() < 4 {
        return None;
    }
    centers.sort_by(f32::total_cmp);
    let anchors = clustered_axis_anchors(&centers, 18.0, 2);
    if anchors.len() < 2 || anchors.len() > 6 {
        return None;
    }
    let (min_graphic_x, max_graphic_x) = graphics
        .iter()
        .map(|graphic| normalized_bounds(graphic.bounds))
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(min_x, max_x), b| {
            (min_x.min(b.x0), max_x.max(b.x1))
        });
    if !min_graphic_x.is_finite() || !max_graphic_x.is_finite() {
        return None;
    }
    let mut xs = Vec::with_capacity(anchors.len() + 1);
    xs.push(min_graphic_x.round() as i32);
    for pair in anchors.windows(2) {
        xs.push(((pair[0] + pair[1]) / 2.0).round() as i32);
    }
    xs.push(max_graphic_x.round() as i32);
    xs.sort();
    xs.dedup();
    (xs.len() >= 3).then_some(xs)
}

/// 外枠が欠けた表の row 境界を text cluster と部分罫線から合成する。
fn synthesize_ys_from_text_rows_and_graphics(
    parsed: &ParsedDocument,
    page: usize,
    graphics: &[ParsedGraphicElement],
) -> Option<Vec<i32>> {
    let mut centers = parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page && !text.text.trim().is_empty())
        .map(|text| (text.bounds.y0 + text.bounds.y1) / 2.0)
        .collect::<Vec<_>>();
    if centers.len() < 4 {
        return None;
    }
    centers.sort_by(f32::total_cmp);
    let anchors = clustered_axis_anchors(&centers, 8.0, 2);
    if anchors.len() < 2 || anchors.len() > 20 {
        return None;
    }
    let (min_graphic_y, max_graphic_y) = graphics
        .iter()
        .map(|graphic| normalized_bounds(graphic.bounds))
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(min_y, max_y), b| {
            (min_y.min(b.y0), max_y.max(b.y1))
        });
    if !min_graphic_y.is_finite() || !max_graphic_y.is_finite() {
        return None;
    }
    let mut ys = Vec::with_capacity(anchors.len() + 1);
    ys.push(min_graphic_y.round() as i32);
    for pair in anchors.windows(2) {
        ys.push(((pair[0] + pair[1]) / 2.0).round() as i32);
    }
    ys.push(max_graphic_y.round() as i32);
    ys.sort();
    ys.dedup();
    (ys.len() >= 3).then_some(ys)
}

/// 近接する軸座標をクラスタ化して代表アンカーを返す。
fn clustered_axis_anchors(values: &[f32], tolerance: f32, min_cluster_size: usize) -> Vec<f32> {
    let mut clusters: Vec<Vec<f32>> = Vec::new();
    for value in values {
        if let Some(cluster) = clusters
            .iter_mut()
            .find(|cluster| (cluster[0] - *value).abs() <= tolerance)
        {
            cluster.push(*value);
        } else {
            clusters.push(vec![*value]);
        }
    }
    clusters
        .iter()
        .filter(|cluster| cluster.len() >= min_cluster_size)
        .map(|cluster| cluster.iter().sum::<f32>() / cluster.len() as f32)
        .collect()
}

/// 部分的な横罫線を row 境界候補へ補完する。
fn augment_ys_from_partial_horizontals(
    xs: &[i32],
    ys: &[i32],
    horizontal: &[ParsedBounds],
) -> Vec<i32> {
    let mut augmented = ys.to_vec();
    let Some(&min_y) = ys.first() else {
        return augmented;
    };
    let Some(&max_y) = ys.last() else {
        return augmented;
    };
    for bounds in horizontal {
        let y = bounds.y0.round() as i32;
        if y <= min_y + 1 || y >= max_y - 1 {
            continue;
        }
        if xs.windows(2).any(|band| {
            bounds.x0.round() as i32 <= band[0] + 1 && bounds.x1.round() as i32 >= band[1] - 1
        }) {
            augmented.push(y);
        }
    }
    augmented.sort();
    augmented.dedup();
    augmented
}

/// 部分的な縦罫線を column 境界候補へ補完する。
fn augment_xs_from_partial_verticals(
    xs: &[i32],
    ys: &[i32],
    vertical: &[ParsedBounds],
) -> Vec<i32> {
    let mut augmented = xs.to_vec();
    let Some(&min_x) = xs.first() else {
        return augmented;
    };
    let Some(&max_x) = xs.last() else {
        return augmented;
    };
    for bounds in vertical {
        let x = bounds.x0.round() as i32;
        if x <= min_x + 1 || x >= max_x - 1 {
            continue;
        }
        if ys.windows(2).any(|band| {
            bounds.y0.round() as i32 <= band[0] + 1 && bounds.y1.round() as i32 >= band[1] - 1
        }) {
            augmented.push(x);
        }
    }
    augmented.sort();
    augmented.dedup();
    augmented
}

/// 罫線の欠損から縦横の結合とセルごとの罫線を反映した DOCX 表モデルを作る。
///
/// 結合も欠けた罫線もない場合は `None` を返し、呼び出し側の全罫線の表モデルに任せる。
fn docx_table_model_from_grid(
    graphics: &[ParsedGraphicElement],
    xs: &[i32],
    ys: &[i32],
    cells: &[Vec<String>],
    cell_underlines: &[Vec<bool>],
    cell_text_bounds: &[Vec<Option<ParsedBounds>>],
) -> Option<DocxTableModel> {
    if cells.is_empty() || cells.iter().any(|row| row.len() + 1 != xs.len()) {
        return None;
    }
    // 結合は線幅 0 の見えない境界も区切りとして求め、罫線は実際に描かれた線だけから求める。
    let (merge_horizontal, merge_vertical) = grid_rules_from_graphics(graphics);
    let drawn = graphics
        .iter()
        .filter(|graphic| graphic.stroke_width > 0.0)
        .cloned()
        .collect::<Vec<_>>();
    let (horizontal, vertical) = grid_rules_from_graphics(&drawn);
    let (dashed_graphics, solid_graphics): (Vec<_>, Vec<_>) =
        drawn.iter().cloned().partition(|graphic| graphic.dashed);
    let (dashed_horizontal, dashed_vertical) = grid_rules_from_graphics(&dashed_graphics);
    let (solid_horizontal, solid_vertical) = grid_rules_from_graphics(&solid_graphics);
    let xs = xs.iter().map(|x| *x as f32).collect::<Vec<_>>();
    let ys = ys.iter().map(|y| *y as f32).collect::<Vec<_>>();
    let has_text = cells
        .iter()
        .map(|row| row.iter().map(|cell| !cell.trim().is_empty()).collect())
        .collect::<Vec<Vec<bool>>>();
    let rects = table_grid::merged_cells(&merge_horizontal, &merge_vertical, &xs, &ys, &has_text);
    let has_span = rects
        .iter()
        .any(|rect| rect.row0 != rect.row1 || rect.col0 != rect.col1);
    let row_count = cells.len();
    let col_count = xs.len() - 1;
    let mut owner = vec![vec![None::<usize>; col_count]; row_count];
    for (index, rect) in rects.iter().enumerate() {
        for row in owner.iter_mut().take(rect.row1 + 1).skip(rect.row0) {
            for cell in row.iter_mut().take(rect.col1 + 1).skip(rect.col0) {
                *cell = Some(index);
            }
        }
    }
    let border_size = table_border_size_for_pdf_rule(table_rule_stroke_width(
        graphics,
        &xs.iter().map(|x| *x as i32).collect::<Vec<_>>(),
        &ys.iter().map(|y| *y as i32).collect::<Vec<_>>(),
    ));
    let top_of = |row: usize| ys[row_count - row];
    let bottom_of = |row: usize| ys[row_count - row - 1];
    let mut rows = Vec::with_capacity(row_count);
    for row in 0..row_count {
        let mut modeled_row = Vec::new();
        let mut col = 0usize;
        while col < col_count {
            let rect = rects[owner[row][col]?];
            let span = rect.col1 - rect.col0 + 1;
            let vertical_merge = (rect.row1 > rect.row0).then_some(if row == rect.row0 {
                DocxVerticalMerge::Restart
            } else {
                DocxVerticalMerge::Continue
            });
            let text = if row == rect.row0 {
                (rect.row0..=rect.row1)
                    .flat_map(|r| (rect.col0..=rect.col1).map(move |c| (r, c)))
                    .map(|(r, c)| cells[r][c].trim())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                String::new()
            };
            let underlined = (rect.col0..=rect.col1).any(|c| {
                cell_underlines
                    .get(row)
                    .and_then(|row| row.get(c))
                    .copied()
                    .unwrap_or(false)
            });
            let (left, right) = (xs[rect.col0], xs[rect.col1 + 1]);
            // 結合セルは、結合した範囲全体に対する文字の位置で配置を決める。
            let rect_text = (rect.row0..=rect.row1)
                .flat_map(|r| (rect.col0..=rect.col1).map(move |c| (r, c)))
                .filter_map(|(r, c)| {
                    cell_text_bounds
                        .get(r)
                        .and_then(|row| row.get(c))
                        .copied()
                        .flatten()
                })
                .reduce(|a, b| ParsedBounds {
                    x0: a.x0.min(b.x0),
                    y0: a.y0.min(b.y0),
                    x1: a.x1.max(b.x1),
                    y1: a.y1.max(b.y1),
                });
            let rect_bounds = ParsedBounds {
                x0: left,
                y0: bottom_of(rect.row1),
                x1: right,
                y1: top_of(rect.row0),
            };
            let (alignment, vertical_alignment) = rect_text
                .map(|text| table_cell_text::infer_cell_alignment(text, rect_bounds))
                .unwrap_or((ParagraphAlignment::Left, Some(CellVerticalAlignment::Top)));
            modeled_row.push(DocxTableCellModel {
                text,
                underlined,
                alignment,
                vertical_alignment,
                grid_span: span,
                vertical_merge,
                width_twips: None,
                borders: Some(DocxCellBorders {
                    top: table_grid::is_ruled(&horizontal, top_of(row), left, right),
                    left: table_grid::is_ruled(&vertical, left, bottom_of(row), top_of(row)),
                    bottom: table_grid::is_ruled(&horizontal, bottom_of(row), left, right),
                    right: table_grid::is_ruled(&vertical, right, bottom_of(row), top_of(row)),
                    size_eighths_pt: border_size,
                    // 辺が破線だけで描かれていれば破線にする（実線と重なる辺は実線）。
                    dashed: [
                        table_grid::is_ruled(&dashed_horizontal, top_of(row), left, right)
                            && !table_grid::is_ruled(&solid_horizontal, top_of(row), left, right),
                        table_grid::is_ruled(&dashed_vertical, left, bottom_of(row), top_of(row))
                            && !table_grid::is_ruled(
                                &solid_vertical,
                                left,
                                bottom_of(row),
                                top_of(row),
                            ),
                        table_grid::is_ruled(&dashed_horizontal, bottom_of(row), left, right)
                            && !table_grid::is_ruled(
                                &solid_horizontal,
                                bottom_of(row),
                                left,
                                right,
                            ),
                        table_grid::is_ruled(&dashed_vertical, right, bottom_of(row), top_of(row))
                            && !table_grid::is_ruled(
                                &solid_vertical,
                                right,
                                bottom_of(row),
                                top_of(row),
                            ),
                    ],
                }),
            });
            col += span;
        }
        rows.push(modeled_row);
    }
    let has_missing_rule = rows.iter().flatten().any(|cell| {
        cell.borders.is_some_and(|borders| {
            !(borders.top && borders.left && borders.bottom && borders.right)
        })
    });
    let has_dashed_rule = rows.iter().flatten().any(|cell| {
        cell.borders
            .is_some_and(|borders| borders.dashed.iter().any(|dashed| *dashed))
    });
    (has_span || has_missing_rule || has_dashed_rule).then_some(DocxTableModel { rows })
}

/// ページ graphics を水平・垂直の罫線区間へ分ける。
fn grid_rules_from_graphics(
    graphics: &[ParsedGraphicElement],
) -> (Vec<table_grid::Rule>, Vec<table_grid::Rule>) {
    let mut horizontal = Vec::new();
    let mut vertical = Vec::new();
    for graphic in graphics {
        let bounds = normalized_bounds(graphic.bounds);
        if is_horizontal_line(&bounds) {
            horizontal.push(table_grid::Rule {
                at: bounds.y0,
                start: bounds.x0,
                end: bounds.x1,
            });
        } else if is_vertical_line(&bounds) {
            vertical.push(table_grid::Rule {
                at: bounds.x0,
                start: bounds.y0,
                end: bounds.y1,
            });
        }
    }
    (horizontal, vertical)
}

/// 指定 row band を縦罫線が分割しているか判定する。
fn has_vertical_boundary_in_band(
    graphics: &[ParsedGraphicElement],
    x: i32,
    bottom: i32,
    top: i32,
) -> bool {
    graphics.iter().any(|graphic| {
        let b = normalized_bounds(graphic.bounds);
        is_vertical_line(&b)
            && (b.x0.round() as i32 - x).abs() <= 1
            && b.y0.round() as i32 <= bottom + TABLE_RULE_JOIN_TOLERANCE_PT
            && b.y1.round() as i32 >= top - TABLE_RULE_JOIN_TOLERANCE_PT
    })
}

/// 指定 column band を横罫線が分割しているか判定する。
fn has_horizontal_boundary_in_column_band(
    graphics: &[ParsedGraphicElement],
    y: i32,
    left: i32,
    right: i32,
) -> bool {
    graphics.iter().any(|graphic| {
        let b = normalized_bounds(graphic.bounds);
        is_horizontal_line(&b)
            && (b.y0.round() as i32 - y).abs() <= 1
            && b.x0.round() as i32 <= left + TABLE_RULE_JOIN_TOLERANCE_PT
            && b.x1.round() as i32 >= right - TABLE_RULE_JOIN_TOLERANCE_PT
    })
}

/// 欠損した内側縦罫線を text column の中心から補完する。
fn augment_xs_from_text_columns(
    parsed: &ParsedDocument,
    page: usize,
    xs: &[i32],
    ys: &[i32],
) -> Option<Vec<i32>> {
    let min_x = *xs.first()? as f32;
    let max_x = *xs.last()? as f32;
    let min_y = *ys.first()? as f32;
    let max_y = *ys.last()? as f32;
    let mut centers = parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page && !text.text.trim().is_empty())
        .filter_map(|text| {
            let center_x = (text.bounds.x0 + text.bounds.x1) / 2.0;
            let center_y = (text.bounds.y0 + text.bounds.y1) / 2.0;
            (center_x >= min_x - 1.0
                && center_x <= max_x + 1.0
                && center_y >= min_y - 1.0
                && center_y <= max_y + 1.0)
                .then_some(center_x)
        })
        .collect::<Vec<_>>();
    if centers.len() < 4 {
        return None;
    }
    centers.sort_by(f32::total_cmp);
    let mut clusters: Vec<Vec<f32>> = Vec::new();
    for center in centers {
        if let Some(cluster) = clusters
            .iter_mut()
            .find(|cluster| (cluster[0] - center).abs() <= 18.0)
        {
            cluster.push(center);
        } else {
            clusters.push(vec![center]);
        }
    }
    let anchors = clusters
        .iter()
        .filter(|cluster| cluster.len() >= 2)
        .map(|cluster| cluster.iter().sum::<f32>() / cluster.len() as f32)
        .collect::<Vec<_>>();
    if anchors.len() < 2 || anchors.len() > 6 || anchors.len() + 1 <= xs.len() {
        return None;
    }
    if !text_columns_repeat_across_row_bands(parsed, page, &anchors, xs, ys) {
        return None;
    }
    let mut augmented = Vec::with_capacity(anchors.len() + 1);
    augmented.push(xs[0]);
    for pair in anchors.windows(2) {
        augmented.push(((pair[0] + pair[1]) / 2.0).round() as i32);
    }
    augmented.push(*xs.last()?);
    augmented.sort();
    augmented.dedup();
    Some(augmented)
}

/// 推定した text column が各 row band で繰り返し現れるか判定する。
fn text_columns_repeat_across_row_bands(
    parsed: &ParsedDocument,
    page: usize,
    anchors: &[f32],
    xs: &[i32],
    ys: &[i32],
) -> bool {
    let Some(&min_x) = xs.first() else {
        return false;
    };
    let Some(&max_x) = xs.last() else {
        return false;
    };
    let row_bands = ys.windows(2).collect::<Vec<_>>();
    if row_bands.len() < 2 {
        return false;
    }
    row_bands.iter().all(|band| {
        let bottom = band[0] as f32;
        let top = band[1] as f32;
        let mut occupied = vec![false; anchors.len()];
        for text in parsed
            .text_elements
            .iter()
            .filter(|text| text.page == page && !text.text.trim().is_empty())
        {
            let center_x = (text.bounds.x0 + text.bounds.x1) / 2.0;
            let center_y = (text.bounds.y0 + text.bounds.y1) / 2.0;
            if center_x < min_x as f32 - 1.0
                || center_x > max_x as f32 + 1.0
                || center_y < bottom - 1.0
                || center_y > top + 1.0
            {
                continue;
            }
            if let Some(index) = nearest_text_column_anchor(center_x, anchors)
                && (center_x - anchors[index]).abs() <= 24.0
            {
                occupied[index] = true;
            }
        }
        occupied.iter().filter(|occupied| **occupied).count() >= 2
    })
}

/// text center に最も近い column anchor を返す。
fn nearest_text_column_anchor(x: f32, anchors: &[f32]) -> Option<usize> {
    anchors
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (x - **a).abs().total_cmp(&(x - **b).abs()))
        .map(|(index, _)| index)
}

/// PDF の座標付き text element を位置付き `OcrTable` 群へ変換する。
fn infer_positioned_tables_from_text_grid(parsed: &ParsedDocument) -> Vec<PositionedTable> {
    parsed
        .text_elements
        .iter()
        .map(|text| text.page)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|page| infer_positioned_table_from_text_grid_page(parsed, page))
        .collect()
}

/// 文字の並びだけから作った表を、罫線なし・数値は右揃えの表モデルにする。
fn borderless_text_table(mut table: PositionedTable) -> PositionedTable {
    if table.docx_table.is_some() {
        return table;
    }
    let mut model = docx_table_model_from_ocr_table(&table.table);
    for cell in model.rows.iter_mut().flatten() {
        cell.borders = Some(DocxCellBorders {
            top: false,
            left: false,
            bottom: false,
            right: false,
            size_eighths_pt: 0,
            dashed: [false; 4],
        });
        let body = cell.text.trim();
        let digits = body.chars().filter(|ch| ch.is_ascii_digit()).count();
        if !body.is_empty() && digits * 2 >= body.chars().filter(|ch| !ch.is_whitespace()).count() {
            cell.alignment = ParagraphAlignment::Right;
        }
    }
    table.docx_table = Some(model);
    // 文字の並びから作った行は、代替フォントの行の高さで文字が切れないよう伸ばせるようにする。
    let rows = table.table.row_count();
    if rows > 0 && table.row_heights_pt.len() != rows {
        let height = (table.bounds.y1 - table.bounds.y0).abs() / rows as f32;
        table.row_heights_pt = vec![height; rows];
    }
    table.row_line_counts = vec![2; rows];
    table
}

/// 指定 page の座標付き text element を反復列の表へ変換する。
fn infer_positioned_table_from_text_grid_page(
    parsed: &ParsedDocument,
    page: usize,
) -> Option<PositionedTable> {
    let mut page_texts = parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page && !text.text.trim().is_empty())
        .collect::<Vec<_>>();
    let has_page_graphics = parsed.graphics.iter().any(|graphic| graphic.page == page);
    let min_texts = if has_page_graphics { 4 } else { 6 };
    let min_rows = if has_page_graphics { 2 } else { 3 };
    if page_texts.len() < min_texts {
        return None;
    }
    let page_text_count = page_texts.len();
    let y_increases_in_content_order = page_texts
        .first()
        .zip(page_texts.last())
        .is_some_and(|(first, last)| last.bounds.y0 > first.bounds.y0 + 1.0);
    if y_increases_in_content_order {
        page_texts.sort_by(|a, b| {
            a.bounds
                .y0
                .total_cmp(&b.bounds.y0)
                .then(a.bounds.x0.total_cmp(&b.bounds.x0))
        });
    } else {
        page_texts.sort_by(|a, b| {
            b.bounds
                .y0
                .total_cmp(&a.bounds.y0)
                .then(a.bounds.x0.total_cmp(&b.bounds.x0))
        });
    }

    let mut rows: Vec<Vec<&ParsedTextElement>> = Vec::new();
    for text in page_texts {
        if let Some(row) = rows
            .iter_mut()
            .find(|row| (row[0].bounds.y0 - text.bounds.y0).abs() <= 4.0)
        {
            row.push(text);
        } else {
            rows.push(vec![text]);
        }
    }

    let row_cells = rows
        .iter_mut()
        .filter_map(|row| {
            row.sort_by(|a, b| a.bounds.x0.total_cmp(&b.bounds.x0));
            merge_text_row_into_cells(row)
        })
        .collect::<Vec<_>>();
    if row_cells.len() < min_rows {
        return None;
    }
    if text_grid_page_is_dense_article_like(page_text_count, row_cells.len()) {
        return None;
    }

    let anchors = repeated_column_anchors(&row_cells);
    if anchors.len() < 2 {
        return None;
    }
    let bounds = text_grid_cells_bounds(&row_cells)?;
    // 段を区切る縦罫線をまたぐ字の並びは、左右の段の行が同じ高さに並んだだけなので表にしない。
    let area = normalized_bounds(bounds);
    let crosses_column_rule = column_bands::partial_band_rules(parsed, page, &[])
        .iter()
        .any(|rule| {
            rule.x0 > area.x0 + 2.0
                && rule.x0 < area.x1 - 2.0
                && rule.y0 < area.y1
                && rule.y1 > area.y0
        })
        || column_bands::column_band_edges(parsed, page, &[])
            .iter()
            .any(|edge| *edge > area.x0 + 2.0 && *edge < area.x1 - 2.0);
    if crosses_column_rule {
        return None;
    }

    let mut table_rows = Vec::new();
    for cells in row_cells {
        let mut row = vec![String::new(); anchors.len()];
        for cell in cells {
            // どの列の近くにもない字（金額の前の印「－」「△」など、使う行の少ない列の字）も
            // 落とさず、いちばん近い列に入れる。
            if let Some(index) = nearest_column_anchor(cell.x0, &anchors)
                .or_else(|| closest_column_anchor(cell.x0, &anchors))
            {
                if !row[index].is_empty() {
                    row[index].push(' ');
                }
                row[index].push_str(&cell.text);
            }
        }
        if row.iter().filter(|value| !value.trim().is_empty()).count() >= 2 {
            table_rows.push(row);
        }
    }

    if table_rows.len() < min_rows {
        return None;
    }
    Some(PositionedTable {
        page,
        bounds,
        stroke_width: 0.0,
        font_name: None,
        font_embedded: false,
        font_size: None,
        column_widths_twips: Vec::new(),
        table: OcrTable::new(table_rows),
        cell_underlines: Vec::new(),
        cell_alignments: Vec::new(),
        cell_vertical_alignments: Vec::new(),
        docx_table: None,
        nested_tables: Vec::new(),
        row_heights_pt: Vec::new(),
        row_line_counts: Vec::new(),
        cell_padding_pt: None,
        cell_font_sizes: Vec::new(),
        cell_tab_stops: Vec::new(),
        cell_fragments: Vec::new(),
        cell_shading: Vec::new(),
    })
}

/// 装飾線の有無によらず長文ページを反復列だけで巨大表に誤分類しないガード。
fn text_grid_page_is_dense_article_like(text_count: usize, row_count: usize) -> bool {
    text_count >= 120 || row_count >= 25
}

/// 1 行分の text element を大きな x gap でセル候補にまとめる。
fn merge_text_row_into_cells(row: &[&ParsedTextElement]) -> Option<Vec<TextGridCell>> {
    let first = row.first()?;
    let mut cells = Vec::new();
    let mut current_text = first.text.trim().to_string();
    let mut current_x0 = first.bounds.x0;
    let mut current_x1 = first.bounds.x1;

    for text in row.iter().skip(1) {
        let gap = text.bounds.x0 - current_x1;
        if gap > 24.0 {
            cells.push(TextGridCell {
                x0: current_x0,
                y0: row
                    .iter()
                    .map(|text| text.bounds.y0)
                    .fold(f32::INFINITY, f32::min),
                x1: current_x1,
                y1: row
                    .iter()
                    .map(|text| text.bounds.y1)
                    .fold(f32::NEG_INFINITY, f32::max),
                text: current_text,
            });
            current_text = text.text.trim().to_string();
            current_x0 = text.bounds.x0;
            current_x1 = text.bounds.x1;
        } else {
            append_cell_fragment(&mut current_text, text.text.trim());
            current_x1 = current_x1.max(text.bounds.x1);
        }
    }
    cells.push(TextGridCell {
        x0: current_x0,
        y0: row
            .iter()
            .map(|text| text.bounds.y0)
            .fold(f32::INFINITY, f32::min),
        x1: current_x1,
        y1: row
            .iter()
            .map(|text| text.bounds.y1)
            .fold(f32::NEG_INFINITY, f32::max),
        text: current_text,
    });

    (cells.len() >= 2).then_some(cells)
}

/// text grid cell 群の外接 bounds を返す。
fn text_grid_cells_bounds(rows: &[Vec<TextGridCell>]) -> Option<ParsedBounds> {
    let mut cells = rows.iter().flatten();
    let first = cells.next()?;
    let mut bounds = ParsedBounds {
        x0: first.x0,
        y0: first.y0,
        x1: first.x1,
        y1: first.y1,
    };
    for cell in cells {
        bounds.x0 = bounds.x0.min(cell.x0);
        bounds.y0 = bounds.y0.min(cell.y0);
        bounds.x1 = bounds.x1.max(cell.x1);
        bounds.y1 = bounds.y1.max(cell.y1);
    }
    Some(bounds)
}

/// 複数行に繰り返し現れる列 x 座標を抽出する。
fn repeated_column_anchors(rows: &[Vec<TextGridCell>]) -> Vec<f32> {
    let mut anchors: Vec<(f32, usize)> = Vec::new();
    for cell in rows.iter().flatten() {
        if let Some((anchor, count)) = anchors
            .iter_mut()
            .find(|(anchor, _)| (*anchor - cell.x0).abs() <= 18.0)
        {
            *anchor = (*anchor * *count as f32 + cell.x0) / (*count as f32 + 1.0);
            *count += 1;
        } else {
            anchors.push((cell.x0, 1));
        }
    }
    let min_count = 3usize.min(rows.len()).max(rows.len() / 5);
    let mut anchors = anchors
        .into_iter()
        .filter(|(_, count)| *count >= min_count)
        .map(|(x, _)| x)
        .collect::<Vec<_>>();
    anchors.sort_by(|a, b| a.total_cmp(b));
    anchors
}

/// cell の x 座標に最も近い列 index を返す。
fn nearest_column_anchor(x: f32, anchors: &[f32]) -> Option<usize> {
    anchors
        .iter()
        .enumerate()
        .filter(|(_, anchor)| (x - **anchor).abs() <= 28.0)
        .min_by(|(_, a), (_, b)| (x - **a).abs().total_cmp(&(x - **b).abs()))
        .map(|(index, _)| index)
}

/// cell の x 座標に、距離を問わず最も近い列 index を返す。
fn closest_column_anchor(x: f32, anchors: &[f32]) -> Option<usize> {
    anchors
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (x - **a).abs().total_cmp(&(x - **b).abs()))
        .map(|(index, _)| index)
}

/// text grid から推定したセル候補。
#[derive(Debug, Clone)]
struct TextGridCell {
    /// セル左端の PDF x 座標。
    x0: f32,
    /// セル下端の PDF y 座標。
    y0: f32,
    /// セル右端の PDF x 座標。
    x1: f32,
    /// セル上端の PDF y 座標。
    y1: f32,
    /// セル内テキスト。
    text: String,
}

/// x 座標から表セルの列 index を返す。
fn column_index_for_x(x: f32, xs: &[i32]) -> Option<usize> {
    xs.windows(2)
        .position(|pair| x >= pair[0] as f32 - 1.0 && x <= pair[1] as f32 + 1.0)
}

/// y 座標から表セルの行 index（上から下）を返す。
fn row_index_for_y(y: f32, ys: &[i32]) -> Option<usize> {
    ys.windows(2)
        .position(|pair| y >= pair[0] as f32 - 1.0 && y <= pair[1] as f32 + 1.0)
        .map(|bottom_interval| ys.len().saturating_sub(2).saturating_sub(bottom_interval))
}

/// 外枠を欠く表の辺を、揃った罫線の端から補った座標列を返す。
///
/// `lines` は辺に直交する罫線（上下辺なら縦罫線）。2 本以上の罫線の端が同じ位置で揃い、
/// その間を `cross` の罫線が実際に結んでいる場合（罫線が 1 本だけなら `cross` と交わる場合）だけ、
/// 既存の座標列より外側の端を追加する。
fn open_table_edges(
    lines: &[&ParsedBounds],
    cross: &[&ParsedBounds],
    existing: &[i32],
    vertical: bool,
) -> Vec<i32> {
    let mut edges = existing.to_vec();
    if lines.is_empty() || cross.is_empty() {
        return edges;
    }
    let (starts, ends): (Vec<f32>, Vec<f32>) = lines
        .iter()
        .map(|bounds| {
            if vertical {
                (bounds.y0, bounds.y1)
            } else {
                (bounds.x0, bounds.x1)
            }
        })
        .unzip();
    let positions = lines
        .iter()
        .map(|bounds| if vertical { bounds.x0 } else { bounds.y0 })
        .collect::<Vec<_>>();
    // 交差する罫線が、端の揃った罫線どうしを少なくとも 1 か所で結んでいること。
    // 罫線が 1 本だけの場合は、交差する罫線と少なくとも 1 か所で交わっていること。
    let required = lines.len().min(2);
    let joined = cross.iter().any(|bounds| {
        let (from, to, at) = if vertical {
            (bounds.x0, bounds.x1, bounds.y0)
        } else {
            (bounds.y0, bounds.y1, bounds.x0)
        };
        positions
            .iter()
            .zip(starts.iter().zip(ends.iter()))
            .filter(|(position, (start, end))| {
                **position >= from - 2.0
                    && **position <= to + 2.0
                    && at >= **start - 2.0
                    && at <= **end + 2.0
            })
            .count()
            >= required
    });
    if !joined {
        return edges;
    }
    for candidates in [starts, ends] {
        let mut sorted = candidates.clone();
        sorted.sort_by(f32::total_cmp);
        let median = sorted[sorted.len() / 2];
        let aligned = candidates
            .iter()
            .filter(|value| (**value - median).abs() <= 2.0)
            .count();
        let edge = median.round() as i32;
        if aligned >= required && !edges.iter().any(|existing| (existing - edge).abs() <= 2) {
            edges.push(edge);
        }
    }
    edges.sort();
    edges.dedup();
    edges
}

/// 表の範囲にある文字の水平方向の占有区間から、全行で空いている隙間を列境界にする。
///
/// 文字単位の要素でも語単位の要素でも同じ結果になる。隙間が語間（約 0.25 em）より広い
/// 文字サイズの 0.5 倍以上あるところだけを列の切れ目とし、2 列以上に分かれる場合に外枠を含む列境界を返す。
fn text_projection_columns(
    parsed: &ParsedDocument,
    page: usize,
    xs: &[i32],
    ys: &[i32],
) -> Option<Vec<i32>> {
    let (left, right) = (*xs.first()? as f32, *xs.last()? as f32);
    let (bottom, top) = (*ys.first()? as f32, *ys.last()? as f32);
    let mut intervals = Vec::new();
    let mut sizes = Vec::new();
    for text in parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page && !text.text.trim().is_empty())
    {
        let bounds = normalized_bounds(text.bounds);
        let center_y = (bounds.y0 + bounds.y1) / 2.0;
        if bounds.x0 < left - 1.0 || bounds.x1 > right + 1.0 || center_y < bottom || center_y > top
        {
            continue;
        }
        intervals.push((bounds.x0, bounds.x1));
        sizes.push(text.font_size.max(bounds.y1 - bounds.y0));
    }
    if intervals.len() < 2 {
        return None;
    }
    sizes.sort_by(f32::total_cmp);
    let min_gap = (sizes[sizes.len() / 2] * 0.5).max(3.0);
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut boundaries = vec![xs[0]];
    let mut reach = intervals[0].1;
    for &(start, end) in &intervals[1..] {
        if start - reach >= min_gap {
            boundaries.push(((start + reach) / 2.0).round() as i32);
        }
        reach = reach.max(end);
    }
    boundaries.push(*xs.last()?);
    (boundaries.len() >= 3).then_some(boundaries)
}

/// 左右の端が揃った（2pt 以内）最長の横罫線の本数を返す。
fn aligned_full_width_rules(horizontal: &[ParsedBounds]) -> usize {
    let Some(longest) = horizontal
        .iter()
        .max_by(|a, b| (a.x1 - a.x0).total_cmp(&(b.x1 - b.x0)))
    else {
        return 0;
    };
    horizontal
        .iter()
        .filter(|bounds| {
            (bounds.x0 - longest.x0).abs() <= 2.0 && (bounds.x1 - longest.x1).abs() <= 2.0
        })
        .count()
}

/// 間隔が狭い（二重線・セルごとの枠）境界座標を、その平均の 1 本へまとめる。
fn merge_close_edges(edges: &[i32]) -> Vec<i32> {
    const DOUBLE_RULE_GAP_PT: i32 = 3;
    let mut groups: Vec<Vec<i32>> = Vec::new();
    for &edge in edges {
        match groups.last_mut() {
            Some(group)
                if edge - *group.last().expect("group is not empty") <= DOUBLE_RULE_GAP_PT =>
            {
                group.push(edge)
            }
            _ => groups.push(vec![edge]),
        }
    }
    groups
        .into_iter()
        .map(|group| {
            let sum = group.iter().map(|edge| *edge as f32).sum::<f32>();
            (sum / group.len() as f32).round() as i32
        })
        .collect()
}

/// 全ての格子線が、分割描画も含めた罫線区間でほぼ覆われているか判定する。
fn has_covered_grid_lines(graphics: &[ParsedGraphicElement], xs: &[i32], ys: &[i32]) -> bool {
    let (horizontal, vertical) = grid_rules_from_graphics(graphics);
    let (Some(&min_y), Some(&max_y)) = (ys.first(), ys.last()) else {
        return false;
    };
    let (Some(&min_x), Some(&max_x)) = (xs.first(), xs.last()) else {
        return false;
    };
    xs.len() >= 2
        && ys.len() >= 2
        && (xs.len() >= 3 || ys.len() >= 3)
        && xs
            .iter()
            .all(|x| table_grid::coverage(&vertical, *x as f32, min_y as f32, max_y as f32) >= 0.9)
        && ys.iter().all(|y| {
            table_grid::coverage(&horizontal, *y as f32, min_x as f32, max_x as f32) >= 0.9
        })
}

/// 外枠を補った表で、全ての列境界が縦罫線で区切られているか判定する。
fn has_open_grid_lines(graphics: &[ParsedGraphicElement], xs: &[i32], ys: &[i32]) -> bool {
    let (horizontal, vertical) = grid_rules_from_graphics(graphics);
    let (Some(&min_y), Some(&max_y)) = (ys.first(), ys.last()) else {
        return false;
    };
    let (Some(&min_x), Some(&max_x)) = (xs.first(), xs.last()) else {
        return false;
    };
    let columns_ruled = xs
        .iter()
        .all(|x| table_grid::coverage(&vertical, *x as f32, min_y as f32, max_y as f32) >= 0.9);
    let rows_ruled = ys
        .iter()
        .all(|y| table_grid::coverage(&horizontal, *y as f32, min_x as f32, max_x as f32) >= 0.9);
    xs.len() >= 3 && (columns_ruled || rows_ruled)
}

/// 抽出座標が表格子として使えるだけの水平/垂直線を持つか判定する。
fn has_grid_lines(graphics: &[ParsedGraphicElement], xs: &[i32], ys: &[i32]) -> bool {
    let min_x = *xs.first().expect("xs must not be empty");
    let max_x = *xs.last().expect("xs must not be empty");
    let min_y = *ys.first().expect("ys must not be empty");
    let max_y = *ys.last().expect("ys must not be empty");

    let horizontal_count = ys
        .iter()
        .filter(|&&y| {
            graphics.iter().any(|graphic| {
                let b = normalized_bounds(graphic.bounds);
                is_horizontal_line(&b)
                    && (b.y0.round() as i32 - y).abs() <= 1
                    && b.x0.round() as i32 <= min_x + 1
                    && b.x1.round() as i32 >= max_x - 1
            })
        })
        .count();
    let vertical_count = xs
        .iter()
        .filter(|&&x| {
            graphics.iter().any(|graphic| {
                let b = normalized_bounds(graphic.bounds);
                is_vertical_line(&b)
                    && (b.x0.round() as i32 - x).abs() <= 1
                    && b.y0.round() as i32 <= min_y + 1
                    && b.y1.round() as i32 >= max_y - 1
            })
        })
        .count();

    horizontal_count == ys.len() && vertical_count == xs.len()
}

/// 内側罫線欠損を許容できる外枠と横罫線を持つか判定する。
fn has_partial_grid_lines(graphics: &[ParsedGraphicElement], xs: &[i32], ys: &[i32]) -> bool {
    let min_x = *xs.first().expect("xs must not be empty");
    let max_x = *xs.last().expect("xs must not be empty");
    let min_y = *ys.first().expect("ys must not be empty");
    let max_y = *ys.last().expect("ys must not be empty");

    let horizontal_count = ys
        .iter()
        .filter(|&&y| {
            graphics.iter().any(|graphic| {
                let b = normalized_bounds(graphic.bounds);
                is_horizontal_line(&b)
                    && (b.y0.round() as i32 - y).abs() <= 1
                    && b.x0.round() as i32 <= min_x + 1
                    && b.x1.round() as i32 >= max_x - 1
            })
        })
        .count();
    let has_left = graphics.iter().any(|graphic| {
        let b = normalized_bounds(graphic.bounds);
        is_vertical_line(&b)
            && (b.x0.round() as i32 - min_x).abs() <= 1
            && b.y0.round() as i32 <= min_y + 1
            && b.y1.round() as i32 >= max_y - 1
    });
    let has_right = graphics.iter().any(|graphic| {
        let b = normalized_bounds(graphic.bounds);
        is_vertical_line(&b)
            && (b.x0.round() as i32 - max_x).abs() <= 1
            && b.y0.round() as i32 <= min_y + 1
            && b.y1.round() as i32 >= max_y - 1
    });

    horizontal_count == ys.len() && has_left && has_right
}

/// 結合セルを含む表として扱える外枠/部分罫線を持つか判定する。
fn has_span_grid_lines(graphics: &[ParsedGraphicElement], xs: &[i32], ys: &[i32]) -> bool {
    let min_x = *xs.first().expect("xs must not be empty");
    let max_x = *xs.last().expect("xs must not be empty");
    let min_y = *ys.first().expect("ys must not be empty");
    let max_y = *ys.last().expect("ys must not be empty");

    let has_top = has_horizontal_boundary_in_column_band(graphics, max_y, min_x, max_x);
    let has_bottom = has_horizontal_boundary_in_column_band(graphics, min_y, min_x, max_x);
    let vertical_count = xs
        .iter()
        .filter(|&&x| {
            graphics.iter().any(|graphic| {
                let b = normalized_bounds(graphic.bounds);
                is_vertical_line(&b)
                    && (b.x0.round() as i32 - x).abs() <= 1
                    && b.y0.round() as i32 <= min_y + 1
                    && b.y1.round() as i32 >= max_y - 1
            })
        })
        .count();
    let partial_horizontal_count = ys
        .iter()
        .filter(|&&y| y != min_y && y != max_y)
        .filter(|&&y| {
            xs.windows(2)
                .any(|band| has_horizontal_boundary_in_column_band(graphics, y, band[0], band[1]))
        })
        .count();

    has_top && has_bottom && vertical_count == xs.len() && partial_horizontal_count + 2 == ys.len()
}

/// top-origin text 補正時だけ許容する結合セル向けの部分罫線判定。
fn has_relaxed_span_grid_lines(graphics: &[ParsedGraphicElement], xs: &[i32], ys: &[i32]) -> bool {
    let min_x = *xs.first().expect("xs must not be empty");
    let max_x = *xs.last().expect("xs must not be empty");
    let min_y = *ys.first().expect("ys must not be empty");
    let max_y = *ys.last().expect("ys must not be empty");
    let has_top = has_horizontal_boundary_in_column_band(graphics, max_y, min_x, max_x);
    let has_bottom = has_horizontal_boundary_in_column_band(graphics, min_y, min_x, max_x);
    let vertical_count = xs
        .iter()
        .filter(|&&x| {
            ys.windows(2)
                .any(|band| has_vertical_boundary_in_band(graphics, x, band[0], band[1]))
        })
        .count();
    let partial_horizontal_count = ys
        .iter()
        .filter(|&&y| y != min_y && y != max_y)
        .filter(|&&y| {
            xs.windows(2)
                .any(|band| has_horizontal_boundary_in_column_band(graphics, y, band[0], band[1]))
        })
        .count();
    has_top && has_bottom && vertical_count == xs.len() && partial_horizontal_count + 2 == ys.len()
}

/// 外枠が無くても内側罫線だけで表格子とみなせるか判定する。
fn has_borderless_grid_lines(graphics: &[ParsedGraphicElement], xs: &[i32], ys: &[i32]) -> bool {
    if xs.len() < 3 || ys.len() < 3 {
        return false;
    }
    let min_y = *ys.first().expect("ys must not be empty");
    let max_y = *ys.last().expect("ys must not be empty");
    let min_x = *xs.first().expect("xs must not be empty");
    let max_x = *xs.last().expect("xs must not be empty");

    let internal_vertical_count = xs[1..xs.len() - 1]
        .iter()
        .filter(|&&x| {
            graphics.iter().any(|graphic| {
                let b = normalized_bounds(graphic.bounds);
                is_vertical_line(&b)
                    && (b.x0.round() as i32 - x).abs() <= 3
                    && b.y0.round() as i32 <= min_y + 3
                    && b.y1.round() as i32 >= max_y - 3
            })
        })
        .count();
    let internal_horizontal_count = ys[1..ys.len() - 1]
        .iter()
        .filter(|&&y| {
            graphics.iter().any(|graphic| {
                let b = normalized_bounds(graphic.bounds);
                is_horizontal_line(&b)
                    && (b.y0.round() as i32 - y).abs() <= 3
                    && b.x0.round() as i32 <= min_x + 3
                    && b.x1.round() as i32 >= max_x - 3
            })
        })
        .count();

    internal_vertical_count == xs.len() - 2 && internal_horizontal_count == ys.len() - 2
}

/// bounds を左下/右上順へ正規化する。
fn normalized_bounds(bounds: ParsedBounds) -> ParsedBounds {
    ParsedBounds {
        x0: bounds.x0.min(bounds.x1),
        y0: bounds.y0.min(bounds.y1),
        x1: bounds.x0.max(bounds.x1),
        y1: bounds.y0.max(bounds.y1),
    }
}

/// 水平線か判定する。
fn is_horizontal_line(bounds: &ParsedBounds) -> bool {
    (bounds.y0 - bounds.y1).abs() < f32::EPSILON && (bounds.x1 - bounds.x0).abs() > 1.0
}

/// 垂直線か判定する。
fn is_vertical_line(bounds: &ParsedBounds) -> bool {
    (bounds.x0 - bounds.x1).abs() < f32::EPSILON && (bounds.y1 - bounds.y0).abs() > 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 数字に挟まれた小さな私用領域の点は小数点に戻し、離れた字はそのまま残す。
    #[test]
    fn private_use_dot_between_digits_becomes_decimal_point() {
        let text = |x0: f32, y0: f32, x1: f32, y1: f32, body: &str| ParsedTextElement {
            baseline_y: None,
            page: 0,
            bounds: ParsedBounds { x0, y0, x1, y1 },
            font_size: 8.0,
            font_name: None,
            fill_color: None,
            text: body.to_string(),
        };
        let mut parsed = ParsedDocument {
            quality: Default::default(),
            path: "decimal.pdf".into(),
            byte_len: 0,
            header: "%PDF-1.7".to_string(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
            page_count: 1,
            pages: Vec::new(),
            graphics: Vec::new(),
            fills: Vec::new(),
            text_elements: vec![
                text(400.0, 526.9, 402.5, 532.8, "1"),
                text(403.8, 526.5, 404.8, 527.5, "\u{EA74}"),
                text(405.6, 526.8, 416.5, 532.8, "061"),
                text(100.0, 526.5, 101.0, 527.5, "\u{EA74}"),
            ],
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
        };
        recover_decimal_points(&mut parsed);
        let texts = parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["1", ".", "061", "\u{EA74}"]);
    }

    /// 座標の並びから度分秒の記号を学習し、文書中の同じ私用領域の字を置き換える。
    #[test]
    fn private_use_angle_marks_become_degree_minute_second() {
        let text = |x0: f32, x1: f32, y0: f32, body: &str| ParsedTextElement {
            baseline_y: None,
            page: 0,
            bounds: ParsedBounds {
                x0,
                y0,
                x1,
                y1: y0 + 8.0,
            },
            font_size: 8.0,
            font_name: None,
            fill_color: None,
            text: body.to_string(),
        };
        let mut parsed = ParsedDocument {
            quality: Default::default(),
            path: "dms.pdf".into(),
            byte_len: 0,
            header: "%PDF-1.7".to_string(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
            page_count: 1,
            pages: Vec::new(),
            graphics: Vec::new(),
            fills: Vec::new(),
            text_elements: vec![
                text(
                    100.0,
                    140.0,
                    300.0,
                    "36 \u{EA6F} 47\u{EA6D} 11.6908\u{EA6E}",
                ),
                text(100.0, 112.0, 200.0, "138"),
                text(112.0, 120.0, 200.0, "\u{EA6F}"),
            ],
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
        };
        recover_angle_marks(&mut parsed);
        let texts = parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["36 ° 47′ 11.6908″", "138", "°"]);
    }

    /// 上付きの記号を別の文字片で置いた座標（官報の実配置）からも学習する。
    #[test]
    fn raised_angle_mark_fragments_are_learned() {
        let text = |x0: f32, y0: f32, x1: f32, y1: f32, body: &str| ParsedTextElement {
            baseline_y: None,
            page: 0,
            bounds: ParsedBounds { x0, y0, x1, y1 },
            font_size: 8.0,
            font_name: None,
            fill_color: None,
            text: body.to_string(),
        };
        let mut parsed = ParsedDocument {
            quality: Default::default(),
            path: "dms-real.pdf".into(),
            byte_len: 0,
            header: "%PDF-1.7".to_string(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
            page_count: 1,
            pages: Vec::new(),
            graphics: Vec::new(),
            fills: Vec::new(),
            text_elements: vec![
                text(263.6, 87.5, 290.7, 93.5, "１ 31"),
                text(291.9, 91.8, 293.9, 93.8, "\u{EA6F}"),
                text(295.8, 87.5, 303.2, 93.5, "53"),
                text(303.7, 91.8, 305.3, 93.9, "\u{EA6D}"),
                text(307.8, 87.5, 315.2, 93.5, "30"),
                text(316.0, 87.2, 317.0, 88.2, "\u{EA74}"),
                text(317.8, 87.5, 333.2, 93.5, "6145"),
                text(333.9, 91.8, 337.1, 93.9, "\u{EA6E}"),
            ],
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
        };
        recover_decimal_points(&mut parsed);
        recover_angle_marks(&mut parsed);
        let joined = parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<String>();
        assert_eq!(joined, "１ 31°53′30.6145″");
    }

    /// 官報の DynaFont の既知の私用領域の字は置き換え、罫線用フォントの字は除く。
    #[test]
    fn known_dynafont_private_use_glyphs_are_mapped() {
        let text = |font: &str, body: &str| ParsedTextElement {
            baseline_y: None,
            page: 0,
            bounds: ParsedBounds {
                x0: 0.0,
                y0: 0.0,
                x1: 10.0,
                y1: 8.0,
            },
            font_size: 8.0,
            font_name: Some(font.to_string()),
            fill_color: None,
            text: body.to_string(),
        };
        let mut parsed = ParsedDocument {
            quality: Default::default(),
            path: "dynafont.pdf".into(),
            byte_len: 0,
            header: "%PDF-1.7".to_string(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
            page_count: 1,
            pages: Vec::new(),
            graphics: Vec::new(),
            fills: Vec::new(),
            text_elements: vec![
                text(
                    "ABCDEF+DFHSMinchoRPro6N-W3",
                    "号外第\u{E88B}\u{E890}\u{E88F}号",
                ),
                text("ABCDEF+DFHSMinchoRPro6N-W3", "\u{E888}12"),
                text("ABCDEF+DFHSMinchoRPro6N-W3", "\u{E7B4}\u{E7D2}"),
                text("ABCDEF+MFHSLine-W3", "\u{E007}\u{E007}"),
                text("Other", "\u{E88B}"),
            ],
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
        };
        map_known_private_use_glyphs(&mut parsed);
        let texts = parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["号外第165号", "△12", "⑶②", "\u{E88B}"]);
    }

    /// 罫線用フォントの細長い私用領域の字は除き、点のような字は文字のまま残す。
    #[test]
    fn rule_glyphs_are_dropped() {
        let text = |x0: f32, y0: f32, x1: f32, y1: f32, body: &str| ParsedTextElement {
            baseline_y: None,
            page: 0,
            bounds: ParsedBounds { x0, y0, x1, y1 },
            font_size: 8.0,
            font_name: None,
            fill_color: None,
            text: body.to_string(),
        };
        let mut parsed = ParsedDocument {
            quality: Default::default(),
            path: "rule-glyphs.pdf".into(),
            byte_len: 0,
            header: "%PDF-1.7".to_string(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
            page_count: 1,
            pages: Vec::new(),
            graphics: Vec::new(),
            fills: Vec::new(),
            text_elements: vec![
                text(10.0, 99.5, 40.0, 100.0, "\u{E005}\u{E005}"),
                text(20.0, 100.0, 21.0, 101.0, "\u{EA74}"),
                text(10.0, 102.0, 40.0, 110.0, "1.061"),
                text(50.0, 96.0, 90.0, 104.0, "\u{E02D}\u{E02D}\u{E02D}\u{E02D}"),
            ],
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
        };
        drop_rule_glyphs(&mut parsed);
        let texts = parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["\u{EA74}", "1.061"]);
        assert!(
            parsed.graphics.is_empty(),
            "rule glyphs must not become table rules"
        );
    }

    #[test]
    fn ocr_spans_from_docx_model_drops_span_that_crosses_existing_vertical_merge() {
        let model = DocxTableModel {
            rows: vec![
                vec![
                    DocxTableCellModel {
                        text: "merged".to_string(),
                        underlined: false,
                        alignment: ParagraphAlignment::Left,
                        vertical_alignment: None,
                        grid_span: 2,
                        vertical_merge: Some(DocxVerticalMerge::Restart),
                        width_twips: None,
                        borders: Some(DocxCellBorders::all()),
                    },
                    DocxTableCellModel {
                        text: "right".to_string(),
                        underlined: false,
                        alignment: ParagraphAlignment::Left,
                        vertical_alignment: None,
                        grid_span: 1,
                        vertical_merge: None,
                        width_twips: None,
                        borders: Some(DocxCellBorders::all()),
                    },
                ],
                vec![
                    DocxTableCellModel {
                        text: String::new(),
                        underlined: false,
                        alignment: ParagraphAlignment::Left,
                        vertical_alignment: None,
                        grid_span: 1,
                        vertical_merge: Some(DocxVerticalMerge::Continue),
                        width_twips: None,
                        borders: Some(DocxCellBorders::all()),
                    },
                    DocxTableCellModel {
                        text: "wide".to_string(),
                        underlined: false,
                        alignment: ParagraphAlignment::Left,
                        vertical_alignment: None,
                        grid_span: 2,
                        vertical_merge: None,
                        width_twips: None,
                        borders: Some(DocxCellBorders::all()),
                    },
                ],
            ],
        };

        let spans = ocr_spans_from_docx_table_model(&model, 2, 3);
        let table = OcrTable::with_spans(
            vec![
                vec!["merged".to_string(), String::new(), "right".to_string()],
                vec![String::new(), "wide".to_string(), String::new()],
            ],
            spans,
        );

        table
            .validate()
            .expect("public OcrTable spans must not overlap");
        assert_eq!(table.spans[0][0], OcrCellSpan::new(2, 2));
        assert_eq!(table.spans[1][1], OcrCellSpan::single());
    }

    #[test]
    fn nested_candidate_allows_small_pdf_rule_rounding_overlap() {
        let parent = ParsedBounds {
            x0: 0.0,
            y0: 0.0,
            x1: 280.0,
            y1: 160.0,
        };
        let child = ParsedBounds {
            x0: 10.0,
            y0: -0.5,
            x1: 130.0,
            y1: 54.0,
        };

        assert!(table_bounds_contain_nested_candidate(parent, child));
    }

    #[test]
    fn nested_candidate_rejects_peer_tables_with_similar_area() {
        let parent = ParsedBounds {
            x0: 0.0,
            y0: 0.0,
            x1: 140.0,
            y1: 160.0,
        };
        let peer = ParsedBounds {
            x0: 139.0,
            y0: 0.0,
            x1: 280.0,
            y1: 160.0,
        };

        assert!(!table_bounds_contain_nested_candidate(parent, peer));
    }

    #[test]
    fn pdf_font_names_are_normalized_for_docx_renderers() {
        assert_eq!(
            docx_font_name_from_pdf_font_name("LTJAPV+HiraKakuProN-W3"),
            "Noto Sans CJK JP"
        );
        assert_eq!(
            docx_font_name_from_pdf_font_name("LTJAPV+KozGoPr6N-Medium"),
            "Noto Sans CJK JP"
        );
        assert_eq!(
            docx_font_name_from_pdf_font_name("AAAAAA+MS-PGothic"),
            "Arial Unicode MS"
        );
        assert_eq!(
            docx_font_name_from_pdf_font_name("AAAAAC+YuMincho-Regular"),
            "Hiragino Mincho ProN"
        );
        assert_eq!(docx_font_name_from_pdf_font_name("ArialMT"), "Arial");
        assert_eq!(docx_font_name_from_pdf_font_name("Helvetica-Bold"), "Arial");
        assert_eq!(
            docx_font_name_from_pdf_font_name("Helvetica-Oblique"),
            "Arial"
        );
        assert_eq!(
            docx_font_name_from_pdf_font_name("TimesNewRomanPSMT"),
            "Times New Roman"
        );
        assert_eq!(
            docx_font_name_from_pdf_font_name("Times-BoldItalic"),
            "Times New Roman"
        );
        assert_eq!(
            docx_font_name_from_pdf_font_name("Courier-BoldOblique"),
            "Courier New"
        );
        assert_eq!(
            docx_font_stretch_from_pdf_font_name("AAAAAA+MS-PGothic"),
            Some(92)
        );
        assert_eq!(docx_font_stretch_from_pdf_font_name("YuMincho"), None);
    }

    #[test]
    fn table_font_selection_ignores_resource_handles_and_rejects_mixed_families() {
        assert_eq!(common_usable_pdf_font_name(["F4"]), None);
        assert_eq!(
            common_usable_pdf_font_name(["AAAAAC+YuMincho-Regular", "BBBBBB+YuMincho-Regular",]),
            Some("AAAAAC+YuMincho-Regular".to_string())
        );
        assert_eq!(
            common_usable_pdf_font_name(["YuMincho-Regular", "YuGothic-Regular"]),
            None
        );
    }

    #[test]
    fn text_grid_page_guard_rejects_dense_article_like_pages() {
        assert!(text_grid_page_is_dense_article_like(120, 3));
        assert!(text_grid_page_is_dense_article_like(10, 25));
        assert!(!text_grid_page_is_dense_article_like(7, 3));
    }
}
