//! ページ画像と、その上に重ねるテキストレイヤ。
//!
//! 描画も抽出も同じ Pdfium から取る。別々のエンジンを使うと、ハイライトの
//! 座標をもう一方の座標系へ変換し直すことになり、ズレの温床になる。
//!
//! テキストレイヤは pdf.js がやっているのと同じ考え方で、画像の上に透明な
//! 文字を重ねる。見た目は画像のまま、選択・コピー・ページ内検索は効く。

use std::io::Cursor;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use image::ImageFormat;
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{ir::Bounds, PageViewport};

/// 描画したページ 1 枚。
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    /// 実際の画像サイズに合わせた PDF→ピクセル変換。
    pub viewport: PageViewport,
    /// PNG バイト列。
    pub png: Vec<u8>,
}

/// A source-declared replacement within one text run. Indices count Unicode
/// scalars, with an exclusive end, matching `TextRun::chars` and `Array.from`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTextRegion {
    pub start: usize,
    pub end: usize,
    pub bounds: Bounds,
    /// Only source anchors/lines or containers were available. The bounds may
    /// enclose several anchors; they do not supply a painted glyph rectangle.
    #[serde(default, skip_serializing_if = "crate::ir::is_false")]
    pub anchor: bool,
}

/// テキストレイヤの 1 まとまり。
///
/// Pdfium の文字ストリームを行と文字サイズでまとめる。文字単位まで刻むと
/// DOM が膨れるうえ、選択の挙動もかえって不自然になる。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    /// Some spans own an entire declared ActualText source region; they do not
    /// claim individual painted glyph positions inside that region.
    #[serde(default, skip_serializing_if = "crate::ir::is_false")]
    pub source_text_region: bool,
    /// At least one recovered source scope supplies only a zero-width or
    /// zero-height anchor, not a painted glyph rectangle.
    #[serde(default, skip_serializing_if = "crate::ir::is_false")]
    pub source_text_anchor: bool,
    /// Exact source ownership ranges, retained when runs merge. Anchor bounds
    /// here must be used instead of inferring a rectangle from the row height.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_regions: Vec<SourceTextRegion>,
    /// Reported SDK mapping errors; printable fallback text can still be present.
    pub sdk_unicode_mapping_errors: usize,
    /// Page-level count, repeated on every run. An empty layer with such objects
    /// returns an error because an empty Vec cannot carry this diagnostic.
    #[serde(default, skip_serializing_if = "crate::ir::is_zero")]
    pub sdk_text_objects_without_characters: usize,
    /// Lower bound on encoded source glyphs absent from nongenerated SDK records.
    /// Zero is not proof of source exhaustiveness: Unicode expansion can mask loss.
    #[serde(default, skip_serializing_if = "crate::ir::is_zero")]
    pub source_text_characters_missing: usize,
    /// Source text accounting could not finish within supported syntax/budgets.
    #[serde(default, skip_serializing_if = "crate::ir::is_false")]
    pub source_text_accounting_unavailable: bool,
    /// Non-whitespace Unicode multiset discrepancy against verified source mappings.
    /// Includes excess SDK scalars only when every source mapping was available.
    #[serde(default, skip_serializing_if = "crate::ir::is_zero")]
    pub source_unicode_scalars_unmatched: usize,
    /// Source font/ActualText Unicode mapping could not be fully verified.
    #[serde(default, skip_serializing_if = "crate::ir::is_false")]
    pub source_unicode_accounting_unavailable: bool,

    /// Page-level tagged reading-order uncertainty, including inferred cells.
    #[serde(default, skip_serializing_if = "crate::ir::is_false")]
    pub reading_order_requires_review: bool,
    /// 回転前の PDF ユーザ空間の外接矩形。表示領域の原点は (0, 0) とは限らない。
    pub bounds: Bounds,
    /// 文字から取得した有効な文字サイズ (PDFユーザ単位)。取得できなければ字面の高さの目安。
    pub size: f64,
    /// Unicode scalar ごとの横位置。Rust `text.chars()` と 1 対 1 で並ぶ。
    /// JS の UTF-16 `text.length` とは異なるため `Array.from(text)` を使う。
    ///
    /// 「変わったのはこの語」と塗り分けるのに要る。文字幅を推し量ると 1 文字
    /// ぶんずれることがあり、帯として出すと目に付く。
    pub chars: Vec<CharSpan>,
}

/// 文字 1 つの横の広がり (PDF ユーザ空間の単位)。
///
/// 縦は行で共通なので持たない。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CharSpan {
    pub x0: f64,
    pub x1: f64,
}

/// ページ 1 枚を PNG にする。
///
/// `target_width` は出力画像の横幅 (px)。表示倍率と devicePixelRatio を
/// 掛けた値を呼び出し側が渡す。
#[cfg(not(target_arch = "wasm32"))]
pub fn render_page(path: &Path, page_number: usize, target_width: u32) -> Result<RenderedPage> {
    check_render_args(page_number, target_width)?;
    let pdfium = bind()?;
    let document = crate::input::load_file(&pdfium, path, &crate::input::single_page_limits())?;
    let unit = crate::page_units::from_file(path, usize::from(document.pages().len()))
        .get(page_number - 1)
        .ok();
    render_loaded(&document, page_number, target_width, unit)
}

/// メモリ上の PDF からページ 1 枚を PNG にする (ブラウザ用)。
pub fn render_page_from_bytes(
    bytes: &[u8],
    page_number: usize,
    target_width: u32,
) -> Result<RenderedPage> {
    check_render_args(page_number, target_width)?;
    let pdfium = bind()?;
    let document = crate::input::load_bytes(&pdfium, bytes, &crate::input::single_page_limits())?;
    let unit = crate::page_units::from_bytes(bytes, usize::from(document.pages().len()))
        .get(page_number - 1)
        .ok();
    render_loaded(&document, page_number, target_width, unit)
}

/// Render at the requested DPI, with each output dimension rounded to a pixel.
/// Use the returned viewport for projection after rounding.
#[cfg(not(target_arch = "wasm32"))]
pub fn render_page_at_dpi(path: &Path, page_number: usize, dpi: u32) -> Result<RenderedPage> {
    check_dpi_args(page_number, dpi)?;
    let pdfium = bind()?;
    let document = crate::input::load_file(&pdfium, path, &crate::input::single_page_limits())?;
    let unit = crate::page_units::from_file(path, usize::from(document.pages().len()))
        .get(page_number - 1)?;
    render_loaded_at_dpi(&document, page_number, dpi, unit)
}

pub fn render_page_at_dpi_from_bytes(
    bytes: &[u8],
    page_number: usize,
    dpi: u32,
) -> Result<RenderedPage> {
    check_dpi_args(page_number, dpi)?;
    let pdfium = bind()?;
    let document = crate::input::load_bytes(&pdfium, bytes, &crate::input::single_page_limits())?;
    let unit = crate::page_units::from_bytes(bytes, usize::from(document.pages().len()))
        .get(page_number - 1)?;
    render_loaded_at_dpi(&document, page_number, dpi, unit)
}

pub(crate) fn check_page_number(page_number: usize) -> Result<()> {
    anyhow::ensure!(
        (1..=65_536).contains(&page_number),
        "page number must be between 1 and 65536"
    );
    Ok(())
}

pub(crate) fn check_dpi_args(page_number: usize, dpi: u32) -> Result<()> {
    check_page_number(page_number)?;
    anyhow::ensure!((1..=600).contains(&dpi), "dpi {dpi} is out of range");
    Ok(())
}

fn render_loaded_at_dpi(
    document: &PdfDocument<'_>,
    page_number: usize,
    dpi: u32,
    user_unit: f64,
) -> Result<RenderedPage> {
    let page = document
        .pages()
        .get((page_number - 1) as u16)
        .map_err(|e| anyhow!("page {page_number} is not available: {e}"))?;
    let scale = physical_scale(dpi, user_unit)?;
    let dimensions = checked_render_dimensions(&page, scale)?;
    encode_page(
        &page,
        page_number,
        &PdfRenderConfig::new().scale_page_by_factor(scale),
        dimensions,
        Some(user_unit),
    )
}

pub(crate) fn physical_scale(dpi: u32, user_unit: f64) -> Result<f32> {
    crate::page_units::validate(user_unit)?;
    let scale = (f64::from(dpi) / 72.0 * user_unit) as f32;
    anyhow::ensure!(
        scale.is_finite() && scale > 0.0,
        "Invalid physical rendering scale"
    );
    Ok(scale)
}

pub(crate) fn checked_render_dimensions(page: &PdfPage<'_>, scale: f32) -> Result<(u32, u32)> {
    let source_width = page.width().value;
    let source_height = page.height().value;
    let width = (source_width * scale).round();
    let height = (source_height * scale).round();
    anyhow::ensure!(
        source_width.is_finite()
            && source_height.is_finite()
            && source_width > 0.0 && source_height > 0.0
            && width.is_finite()
            && height.is_finite()
            && (1.0..=8192.0).contains(&width)
            && (1.0..=8192.0).contains(&height)
            && f64::from(width) * f64::from(height) <= 16_000_000.0,
        "page exceeds the rendering limit or has invalid dimensions (8192px per axis, 16000000 pixels)"
    );
    Ok((width as u32, height as u32))
}

pub(crate) fn check_render_args(page_number: usize, target_width: u32) -> Result<()> {
    check_page_number(page_number)?;
    anyhow::ensure!(
        (16..=8192).contains(&target_width),
        "target width {target_width} is out of range"
    );
    Ok(())
}

fn render_loaded(
    document: &PdfDocument<'_>,
    page_number: usize,
    target_width: u32,
    user_unit: Option<f64>,
) -> Result<RenderedPage> {
    let page = document
        .pages()
        .get((page_number - 1) as u16)
        .map_err(|e| anyhow!("page {page_number} is not available: {e}"))?;

    let dimensions = checked_render_dimensions(&page, target_width as f32 / page.width().value)?;
    encode_page(
        &page,
        page_number,
        &PdfRenderConfig::new().set_target_width(target_width as i32),
        dimensions,
        user_unit,
    )
}

fn encode_page(
    page: &PdfPage<'_>,
    page_number: usize,
    config: &PdfRenderConfig,
    dimensions: (u32, u32),
    user_unit: Option<f64>,
) -> Result<RenderedPage> {
    crate::raster_limits::check_page(page, crate::raster_limits::DEFAULT_SOURCE_IMAGE_PIXELS)?;
    let (view_box, rotation) = crate::geometry::read_page_geometry(page)?;
    let mut viewport = PageViewport::new(view_box, rotation, dimensions.0, dimensions.1)?;
    viewport.user_unit = user_unit;
    let bitmap = page
        .render_with_config(config)
        .map_err(|e| anyhow!("failed to render page {page_number}: {e}"))?;

    let image = bitmap.as_image();
    let (width, height) = (image.width(), image.height());
    anyhow::ensure!(
        (width, height) == dimensions,
        "Unexpected Pdfium bitmap dimensions"
    );
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .context("failed to encode the page as png")?;

    Ok(RenderedPage {
        width,
        height,
        viewport,
        png,
    })
}

/// ページ 1 枚ぶんのテキストレイヤ。
#[cfg(not(target_arch = "wasm32"))]
pub fn text_layer(path: &Path, page_number: usize) -> Result<Vec<TextRun>> {
    check_page_number(page_number)?;
    let pdfium = bind()?;
    let limits = crate::input::single_page_limits();
    let (document, (unicode, units)) =
        crate::input::load_file_with(&pdfium, path, &limits, |handle| {
            let count = pdfium.bindings().FPDF_GetPageCount(handle.cast()) as usize;
            let units =
                crate::page_units::from_file_for_text(path, count, page_number - 1..page_number);
            let unicode =
                crate::unicode_facts::read(&pdfium, handle, &limits, Some(page_number), &units)?;
            Ok((unicode, units))
        })?;
    text_layer_loaded(
        &document,
        page_number,
        &unicode[0],
        units.get(page_number - 1).ok(),
    )
}

/// メモリ上の PDF からページ 1 枚ぶんのテキストレイヤを取る (ブラウザ用)。
pub fn text_layer_from_bytes(bytes: &[u8], page_number: usize) -> Result<Vec<TextRun>> {
    check_page_number(page_number)?;
    let pdfium = bind()?;
    let limits = crate::input::single_page_limits();
    let (document, (unicode, units)) =
        crate::input::load_bytes_with(&pdfium, bytes, &limits, |handle| {
            let count = pdfium.bindings().FPDF_GetPageCount(handle.cast()) as usize;
            let units =
                crate::page_units::from_bytes_for_text(bytes, count, page_number - 1..page_number);
            let unicode =
                crate::unicode_facts::read(&pdfium, handle, &limits, Some(page_number), &units)?;
            Ok((unicode, units))
        })?;
    text_layer_loaded(
        &document,
        page_number,
        &unicode[0],
        units.get(page_number - 1).ok(),
    )
}

fn text_layer_loaded(
    document: &PdfDocument<'_>,
    page_number: usize,
    unicode: &crate::unicode_facts::PageFacts,
    user_unit: Option<f64>,
) -> Result<Vec<TextRun>> {
    let mut page = document
        .pages()
        .get((page_number - 1) as u16)
        .map_err(|e| anyhow!("page {page_number} is not available: {e}"))?;
    let (bounds, _) = crate::geometry::read_page_geometry(&page)?;
    crate::physical_text::prepare(&mut page, unicode, bounds)?;

    let limits = crate::Limits::default();
    let runs = crate::text::extract_runs(
        &mut page,
        unicode,
        user_unit,
        limits.max_blocks_per_page,
        limits.max_input_mb.saturating_mul(1024 * 1024),
    )?;
    anyhow::ensure!(
        !runs.runs.is_empty() || unicode.sdk_text_objects_without_characters == 0,
        "SDK text objects have no character records; an empty text layer cannot represent this uncertainty"
    );
    anyhow::ensure!(
        !runs.runs.is_empty() || (unicode.source_text_characters_missing == 0 && !unicode.source_text_accounting_unavailable && unicode.source_unicode_scalars_unmatched == 0 && !unicode.source_unicode_accounting_unavailable),
        "Source text accounting is incomplete; an empty text layer cannot represent this uncertainty"
    );
    let reading_order_requires_review = runs.reading_order_requires_review;
    Ok(runs
        .runs
        .into_iter()
        .map(|run| TextRun {
            size: run
                .size
                .unwrap_or_else(|| run.bounds.height().max(1.0 / user_unit.unwrap_or(1.0))),
            text: run.text,
            source_text_region: run.source_text_region,
            source_text_anchor: run.source_text_anchor,
            source_regions: run.source_regions,
            sdk_unicode_mapping_errors: run.sdk_unicode_mapping_errors,
            sdk_text_objects_without_characters: unicode.sdk_text_objects_without_characters,
            source_text_characters_missing: unicode.source_text_characters_missing,
            source_text_accounting_unavailable: unicode.source_text_accounting_unavailable,
            source_unicode_scalars_unmatched: unicode.source_unicode_scalars_unmatched,
            source_unicode_accounting_unavailable: unicode.source_unicode_accounting_unavailable,
            reading_order_requires_review,
            bounds: run.bounds,
            chars: run
                .spans
                .into_iter()
                .map(|(x0, x1)| CharSpan { x0, x1 })
                .collect(),
        })
        .collect())
}

/// Pdfium を読み込む。
///
/// `bundled-pdfium` を付けてビルドしたときは実行ファイルに埋め込んだものを使う。
/// 付けていないときは実行時に取りに行く (手元での開発用)。
///
/// WASM では JS 側で読み込んだ Pdfium の Emscripten モジュールにつなぐ。
/// 先に JS から `initialize_pdfium_render()` を呼んでおく必要がある。
pub(crate) fn bind() -> Result<Pdfium> {
    #[cfg(not(target_arch = "wasm32"))]
    let result = Pdfium::bind_to_library(crate::pdfium_library_path()?).map(Pdfium::new);
    #[cfg(target_arch = "wasm32")]
    let result = crate::wasm_runtime::bind();

    result.map_err(|e| anyhow!("PDF の描画に必要な Pdfium を読み込めませんでした: {e}"))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        crate::report_fixture::path(name)
    }

    fn blank_pdf(width: f32, height: f32, rotation: u16) -> Vec<u8> {
        // Minimal independent PDF: no fonts or external fixture generator.
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Rotate {rotation} /Resources << >> >>"),
        ];
        let mut source = String::from("%PDF-1.7\n");
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(source.len());
            source.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
        }
        let xref = source.len();
        source.push_str("xref\n0 4\n0000000000 65535 f \n");
        for offset in offsets {
            source.push_str(&format!("{offset:010} 00000 n \n"));
        }
        source.push_str(&format!(
            "trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
        ));
        source.into_bytes()
    }

    #[test]
    fn pixel_rendering_uses_fixed_dpi_for_fractional_and_small_pages() {
        for (width, height, rotation, expected) in [
            (600.24, 800.26, 0, (1200, 1601)),
            (600.24, 800.26, 90, (1601, 1200)),
            (4.0, 6.0, 0, (8, 12)),
        ] {
            let bytes = blank_pdf(width, height, rotation);
            let rendered = render_page_at_dpi_from_bytes(&bytes, 1, 144).unwrap();
            assert_eq!((rendered.width, rendered.height), expected);
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("small.pdf");
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(render_page_at_dpi(&path, 1, 144).unwrap().png, rendered.png);
        }
    }

    #[test]
    fn pixel_rendering_rejects_invalid_dpi_and_oversized_pages() {
        let bytes = blank_pdf(600.0, 800.0, 0);
        for dpi in [0, 601, u32::MAX] {
            assert!(render_page_at_dpi_from_bytes(&bytes, 1, dpi).is_err());
        }
        for page in [0, 65_537, usize::MAX] {
            assert!(render_page_at_dpi_from_bytes(&bytes, page, 144).is_err());
            assert!(render_page_from_bytes(&bytes, page, 400).is_err());
            assert!(text_layer_from_bytes(&bytes, page).is_err());
        }
        // Check actual dimensions before allocating a bitmap.
        let bytes = blank_pdf(600.0, 100_000.0, 0);
        assert!(render_page_at_dpi_from_bytes(&bytes, 1, 144).is_err());
        let bytes = blank_pdf(2000.0, 2001.0, 0);
        assert!(render_page_at_dpi_from_bytes(&bytes, 1, 144).is_err());
    }

    #[test]
    fn renders_a_page_at_the_requested_width() {
        let page = render_page(&fixture("report-old.pdf"), 1, 400).unwrap();
        assert_eq!(page.width, 400);
        // A4 縦なので、高さは幅のおよそ 1.41 倍。
        assert!(
            (page.height as f64 / 400.0 - 1.414).abs() < 0.05,
            "height: {}",
            page.height
        );
        assert_eq!(&page.png[1..4], b"PNG", "PNG シグネチャがない");
    }

    #[test]
    fn rendering_rejects_a_page_out_of_range() {
        assert!(render_page(&fixture("report-old.pdf"), 99, 400).is_err());
    }

    #[test]
    fn rendering_rejects_an_absurd_width() {
        assert!(render_page(&fixture("report-old.pdf"), 1, 100_000).is_err());
    }

    #[test]
    fn text_layer_covers_the_page_text() {
        let runs = text_layer(&fixture("report-old.pdf"), 1).unwrap();
        assert!(!runs.is_empty());

        let joined: String = runs.iter().map(|run| run.text.as_str()).collect();
        assert!(joined.contains("業務委託契約書"), "text: {joined}");

        // 選択のために、どの行も大きさを持っていること。
        for run in &runs {
            assert!(
                run.bounds.width() > 0.0 && run.bounds.height() > 0.0,
                "{run:?}"
            );
            assert!(run.size > 0.0);
        }
    }

    #[test]
    fn 文字ごとの位置が本文と一対一で並ぶ() {
        let runs = text_layer(&fixture("report-old.pdf"), 1).unwrap();
        let with_spans: Vec<_> = runs.iter().filter(|run| !run.chars.is_empty()).collect();
        assert!(!with_spans.is_empty(), "文字の位置が 1 つも取れていない");

        for run in with_spans {
            assert_eq!(
                run.chars.len(),
                run.text.chars().count(),
                "文字と位置の数が合わない: {:?}",
                run.text
            );
            // 左から右へ並び、行の外へはみ出さないこと。塗る帯の位置はこれに
            // 乗るので、順番が崩れていると見当違いの所を塗る。
            let mut previous = run.bounds.x0 - 1.0;
            for span in &run.chars {
                assert!(span.x0 <= span.x1, "{span:?}");
                assert!(span.x0 >= previous - 1.0, "左へ戻っている: {span:?}");
                previous = span.x0;
            }
        }
    }

    #[test]
    fn bytes_and_path_give_the_same_page() {
        // ブラウザはバイト列で、ネイティブはパスで読む。同じ結果でないと
        // Web 版とデスクトップ版でハイライトの位置が変わる。
        let path = fixture("report-old.pdf");
        let bytes = std::fs::read(&path).unwrap();

        let from_path = text_layer(&path, 1).unwrap();
        let from_bytes = text_layer_from_bytes(&bytes, 1).unwrap();
        assert_eq!(
            serde_json::to_value(&from_path).unwrap(),
            serde_json::to_value(&from_bytes).unwrap()
        );

        let image = render_page_from_bytes(&bytes, 1, 400).unwrap();
        assert_eq!(image.png, render_page(&path, 1, 400).unwrap().png);
    }

    #[test]
    fn text_layer_is_empty_for_a_page_without_text() {
        // 存在しないページではなく、テキストの無いページとして扱えること。
        let runs = text_layer(&fixture("report-old.pdf"), 3).unwrap();
        assert!(runs.iter().all(|run| !run.text.trim().is_empty()));
    }
}
