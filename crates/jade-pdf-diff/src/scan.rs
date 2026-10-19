//! Pdfium で開いた PDF から [`DocumentIr`] を組む。
//!
//! ネイティブと WASM の両方で使う部分。ファイルパスやスレッドには触らないので、
//! ブラウザ内でも同じ抽出結果になる。

use anyhow::{anyhow, Result};
use pdfium_render::prelude::*;

use crate::ir::{normalize, BlockIr, Bounds, DocumentIr, PageIr};

/// 読み込み時の上限。
#[derive(Debug, Clone)]
pub struct Limits {
    /// 読み込むファイルサイズの上限 (MB)。
    pub max_input_mb: u64,
    pub max_pages: usize,
    pub max_blocks_per_page: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_mb: 512,
            max_pages: 5_000,
            max_blocks_per_page: 20_000,
        }
    }
}

pub(crate) fn ensure_size(size: u64, limits: &Limits) -> Result<()> {
    let max_bytes = limits.max_input_mb.saturating_mul(1024 * 1024);
    if size > max_bytes {
        return Err(anyhow!(
            "pdf is {size} bytes, exceeding max_input_mb {}",
            limits.max_input_mb
        ));
    }
    Ok(())
}

/// メモリ上の PDF を Pdfium だけで読む。
///
/// ブラウザではファイルパスが無いので、こちらが本線になる。
/// ネイティブの [`crate::load_document`] も同じ抽出を使う。
pub fn load_document_from_bytes(bytes: &[u8], limits: &Limits) -> Result<DocumentIr> {
    ensure_size(bytes.len() as u64, limits)?;
    let pdfium = crate::render::bind()?;
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|e| anyhow!("Pdfium が PDF を開けませんでした: {e}"))?;
    scan_document(&document, limits)?.ok_or_else(|| anyhow!("PDF にページがありません"))
}

/// Pdfium でページの枠とテキストを取り出す。
///
/// ページが 1 枚も無いときだけ `None`。テキストが 1 つも無くても
/// (スキャンした紙など)、ページの枠は入った [`DocumentIr`] を返す。
pub(crate) fn scan_document(
    document: &PdfDocument<'_>,
    limits: &Limits,
) -> Result<Option<DocumentIr>> {
    anyhow::ensure!(
        usize::from(document.pages().len()) <= limits.max_pages,
        "PDF exceeds max_pages {}. No partial comparison was performed.",
        limits.max_pages
    );
    let mut pages: Vec<PageIr> = Vec::new();
    for (number, page) in document.pages().iter().enumerate() {
        let height = page.height().value as f64;
        let mut blocks: Vec<BlockIr> = Vec::new();

        if let Ok(text) = page.text() {
            for segment in text.segments().iter() {
                let content = normalize(&segment.text());
                if content.is_empty() {
                    continue;
                }
                anyhow::ensure!(
                    blocks.len() < limits.max_blocks_per_page,
                    "PDF page {} exceeds max_blocks_per_page {}. No partial comparison was performed.", number + 1, limits.max_blocks_per_page
                );
                // Pdfium の rect は上端 / 下端が PDF ユーザ空間のまま入っている。
                let rect = segment.bounds();
                let y0 = rect.bottom().value.min(rect.top().value) as f64;
                let y1 = rect.bottom().value.max(rect.top().value) as f64;
                blocks.push(BlockIr {
                    index: blocks.len() + 1,
                    text: content,
                    bounds: Bounds {
                        x0: rect.left().value as f64,
                        y0,
                        x1: rect.right().value as f64,
                        y1,
                    },
                    // Pdfium の segment は font size を持たないので、行の高さで
                    // 代用する。表示用の目安にしか使っていない。
                    size: (y1 - y0 > 0.0).then_some(y1 - y0),
                });
            }
        }

        pages.push(PageIr {
            index: number + 1,
            width: page.width().value as f64,
            height,
            rotation: page
                .rotation()
                .map(|rotation| rotation.as_degrees() as i64)
                .unwrap_or(0)
                .rem_euclid(360) as u64,
            blocks,
        });
    }

    Ok((!pages.is_empty()).then_some(DocumentIr { pages }))
}
