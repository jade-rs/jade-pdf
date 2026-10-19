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
use serde::Serialize;

use crate::ir::Bounds;

/// 描画したページ 1 枚。
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    /// PNG バイト列。
    pub png: Vec<u8>,
}

/// テキストレイヤの 1 まとまり。
///
/// Pdfium が返す segment (おおむね 1 行) をそのまま使う。文字単位まで刻むと
/// DOM が膨れるうえ、選択の挙動もかえって不自然になる。
#[derive(Debug, Clone, Serialize)]
pub struct TextRun {
    pub text: String,
    /// PDF ユーザ空間の外接矩形 (原点は左下)。
    pub bounds: Bounds,
    /// 字面の高さから求めたフォントサイズの目安 (pt)。
    pub size: f64,
    /// 文字ごとの横位置。`text` の文字と 1 対 1 で並ぶ。
    ///
    /// 「変わったのはこの語」と塗り分けるのに要る。文字幅を推し量ると 1 文字
    /// ぶんずれることがあり、帯として出すと目に付く。
    pub chars: Vec<CharSpan>,
}

/// 文字 1 つの横の広がり (PDF ユーザ空間、pt)。
///
/// 縦は行で共通なので持たない。
#[derive(Debug, Clone, Copy, Serialize)]
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
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| anyhow!("failed to open {}: {e}", path.display()))?;
    render_loaded(&document, page_number, target_width)
}

/// メモリ上の PDF からページ 1 枚を PNG にする (ブラウザ用)。
pub fn render_page_from_bytes(
    bytes: &[u8],
    page_number: usize,
    target_width: u32,
) -> Result<RenderedPage> {
    check_render_args(page_number, target_width)?;
    let pdfium = bind()?;
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|e| anyhow!("failed to open the pdf: {e}"))?;
    render_loaded(&document, page_number, target_width)
}

/// Render at a fixed scale in both axes. Width-only scaling changes the
/// effective DPI when a fractional page width is rounded to a pixel.
#[cfg(not(target_arch = "wasm32"))]
pub fn render_page_at_dpi(path: &Path, page_number: usize, dpi: u32) -> Result<RenderedPage> {
    check_dpi_args(page_number, dpi)?;
    let pdfium = bind()?;
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| anyhow!("failed to open {}: {e}", path.display()))?;
    render_loaded_at_dpi(&document, page_number, dpi)
}

pub fn render_page_at_dpi_from_bytes(
    bytes: &[u8],
    page_number: usize,
    dpi: u32,
) -> Result<RenderedPage> {
    check_dpi_args(page_number, dpi)?;
    let pdfium = bind()?;
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|e| anyhow!("failed to open the pdf: {e}"))?;
    render_loaded_at_dpi(&document, page_number, dpi)
}

fn check_dpi_args(page_number: usize, dpi: u32) -> Result<()> {
    anyhow::ensure!(
        (1..=65_536).contains(&page_number),
        "page number must be between 1 and 65536"
    );
    anyhow::ensure!((1..=600).contains(&dpi), "dpi {dpi} is out of range");
    Ok(())
}

fn render_loaded_at_dpi(
    document: &PdfDocument<'_>,
    page_number: usize,
    dpi: u32,
) -> Result<RenderedPage> {
    let page = document
        .pages()
        .get((page_number - 1) as u16)
        .map_err(|e| anyhow!("page {page_number} is not available: {e}"))?;
    let scale = dpi as f32 / 72.0;
    let width = (page.width().value * scale).round();
    let height = (page.height().value * scale).round();
    anyhow::ensure!(
        width.is_finite()
            && height.is_finite()
            && (1.0..=8192.0).contains(&width)
            && (1.0..=8192.0).contains(&height)
            && f64::from(width) * f64::from(height) <= 16_000_000.0,
        "fixed-DPI page exceeds the rendering limit (8192px per axis, 16000000 pixels)"
    );
    encode_page(
        &page,
        page_number,
        &PdfRenderConfig::new().scale_page_by_factor(scale),
    )
}

fn check_render_args(page_number: usize, target_width: u32) -> Result<()> {
    anyhow::ensure!(
        (1..=65_536).contains(&page_number),
        "page number must be between 1 and 65536"
    );
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
) -> Result<RenderedPage> {
    let page = document
        .pages()
        .get((page_number - 1) as u16)
        .map_err(|e| anyhow!("page {page_number} is not available: {e}"))?;

    encode_page(
        &page,
        page_number,
        &PdfRenderConfig::new().set_target_width(target_width as i32),
    )
}

fn encode_page(
    page: &PdfPage<'_>,
    page_number: usize,
    config: &PdfRenderConfig,
) -> Result<RenderedPage> {
    let bitmap = page
        .render_with_config(config)
        .map_err(|e| anyhow!("failed to render page {page_number}: {e}"))?;

    let image = bitmap.as_image();
    let (width, height) = (image.width(), image.height());
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .context("failed to encode the page as png")?;

    Ok(RenderedPage { width, height, png })
}

/// ページ 1 枚ぶんのテキストレイヤ。
#[cfg(not(target_arch = "wasm32"))]
pub fn text_layer(path: &Path, page_number: usize) -> Result<Vec<TextRun>> {
    anyhow::ensure!(
        (1..=65_536).contains(&page_number),
        "page number must be between 1 and 65536"
    );
    let pdfium = bind()?;
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| anyhow!("failed to open {}: {e}", path.display()))?;
    text_layer_loaded(&document, page_number)
}

/// メモリ上の PDF からページ 1 枚ぶんのテキストレイヤを取る (ブラウザ用)。
pub fn text_layer_from_bytes(bytes: &[u8], page_number: usize) -> Result<Vec<TextRun>> {
    anyhow::ensure!(
        (1..=65_536).contains(&page_number),
        "page number must be between 1 and 65536"
    );
    let pdfium = bind()?;
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|e| anyhow!("failed to open the pdf: {e}"))?;
    text_layer_loaded(&document, page_number)
}

fn text_layer_loaded(document: &PdfDocument<'_>, page_number: usize) -> Result<Vec<TextRun>> {
    let page = document
        .pages()
        .get((page_number - 1) as u16)
        .map_err(|e| anyhow!("page {page_number} is not available: {e}"))?;

    let Ok(text) = page.text() else {
        return Ok(Vec::new());
    };

    let mut runs = Vec::new();
    for segment in text.segments().iter() {
        let rect = segment.bounds();
        let y0 = rect.bottom().value.min(rect.top().value) as f64;
        let y1 = rect.bottom().value.max(rect.top().value) as f64;
        let x0 = rect.left().value.min(rect.right().value) as f64;

        // 本文は文字の並びから組み立てる。segment.text() をそのまま使うと、
        // 行末の改行が落とされているぶん位置の数と合わなくなる。
        let (content, chars) = match char_spans(&segment, x0) {
            Some(pair) => pair,
            None => (segment.text(), Vec::new()),
        };
        if content.trim().is_empty() {
            continue;
        }

        runs.push(TextRun {
            text: content,
            bounds: Bounds {
                x0,
                y0,
                x1: rect.left().value.max(rect.right().value) as f64,
                y1,
            },
            size: (y1 - y0).max(1.0),
            chars,
        });
    }
    Ok(runs)
}

/// 行の本文と、文字ごとの横位置。**必ず 1 対 1 で並ぶ**。
///
/// 位置が取れないときは `None`。半端に返すと途中から 1 文字ずつずれた場所を
/// 塗ることになり、行全体を塗るより悪い。
fn char_spans(segment: &PdfPageTextSegment<'_>, start_x: f64) -> Option<(String, Vec<CharSpan>)> {
    let chars = segment.chars().ok()?;

    let mut text = String::new();
    let mut spans = Vec::new();
    let mut last = start_x;
    for ch in chars.iter() {
        let Some(unicode) = ch.unicode_char() else {
            continue;
        };
        let span = match ch.loose_bounds() {
            Ok(rect) => {
                let x0 = rect.left().value.min(rect.right().value) as f64;
                let x1 = rect.left().value.max(rect.right().value) as f64;
                last = x1;
                CharSpan { x0, x1 }
            }
            // 位置の取れない文字 (合字の続きなど) は幅ゼロで置く。数を合わせて
            // おかないと、以降の文字がまとめてずれる。
            Err(_) => CharSpan { x0: last, x1: last },
        };
        text.push(unicode);
        spans.push(span);
    }

    // 行末の改行は描かない。位置と一緒に落とす。
    while text.ends_with(['\r', '\n']) {
        text.pop();
        spans.pop();
    }
    (!spans.is_empty()).then_some((text, spans))
}

/// Pdfium を読み込む。
///
/// `bundled-pdfium` を付けてビルドしたときは実行ファイルに埋め込んだものを使う。
/// 付けていないときは実行時に取りに行く (手元での開発用)。
///
/// WASM では JS 側で読み込んだ Pdfium の Emscripten モジュールにつなぐ。
/// 先に JS から `initialize_pdfium_render()` を呼んでおく必要がある。
pub(crate) fn bind() -> Result<Pdfium> {
    #[cfg(all(not(target_arch = "wasm32"), feature = "bundled-pdfium"))]
    let result = pdfium_auto::bind_bundled();
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "bundled-pdfium")))]
    let result = pdfium_auto::bind_pdfium_silent();
    #[cfg(target_arch = "wasm32")]
    let result = Pdfium::bind_to_system_library().map(Pdfium::new);

    result.map_err(|e| anyhow!("PDF の描画に必要な Pdfium を読み込めませんでした: {e}"))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

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
}
