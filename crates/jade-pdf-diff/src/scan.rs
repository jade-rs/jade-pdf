//! Pdfium で開いた PDF から [`DocumentIr`] を組む。
//!
//! ネイティブと WASM の両方で使う部分。ファイルパスやスレッドには触らないので、
//! ブラウザ内でも同じ抽出結果になる。

use anyhow::{anyhow, Context, Result};
use pdfium_render::prelude::*;

use crate::ir::{normalize, BlockIr, DocumentIr, PageIr};

/// 読み込み時の上限。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Limits {
    /// 読み込むファイルサイズ、および抽出テキスト総量の上限 (MB)。
    /// 文字インデックスの確保前に、1 ページ 200 万文字の上限も適用する。
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

impl Limits {
    /// Resource budgets must be positive. Pdfium's 65535-page backend limit
    /// applies even if max_pages is larger.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.max_input_mb > 0 && self.max_pages > 0 && self.max_blocks_per_page > 0,
            "PDF resource limits must be positive"
        );
        Ok(())
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
    limits.validate()?;
    ensure_size(bytes.len() as u64, limits)?;
    let pdfium = crate::render::bind()?;
    let (document, (unicode, units)) =
        crate::input::load_bytes_with(&pdfium, bytes, limits, |handle| {
            let count = pdfium.bindings().FPDF_GetPageCount(handle.cast()) as usize;
            let units = crate::page_units::from_bytes_for_text(bytes, count, 0..count);
            let unicode = crate::unicode_facts::read(&pdfium, handle, limits, None, &units)?;
            Ok((unicode, units))
        })?;
    scan_document(&document, limits, &unicode, &units)?
        .ok_or_else(|| anyhow!("PDF にページがありません"))
}

/// Pdfium でページの枠とテキストを取り出す。
///
/// ページが 1 枚も無いときだけ `None`。テキストが 1 つも無くても
/// (スキャンした紙など)、ページの枠は入った [`DocumentIr`] を返す。
pub(crate) fn scan_document(
    document: &PdfDocument<'_>,
    limits: &Limits,
    unicode: &[crate::unicode_facts::PageFacts],
    units: &crate::page_units::PageUnits,
) -> Result<Option<DocumentIr>> {
    anyhow::ensure!(
        unicode.len() == usize::from(document.pages().len()),
        "Unicode diagnostic page count changed while opening"
    );
    anyhow::ensure!(
        usize::from(document.pages().len()) <= limits.max_pages,
        "PDF exceeds max_pages {}. No partial comparison was performed.",
        limits.max_pages
    );
    let mut pages: Vec<PageIr> = Vec::new();
    let mut text_bytes = 0u64;
    let mut source_characters = 0u64;
    // PdfPagesIterator はページの読み込み失敗を None に変換する。失敗した
    // ページ以降を黙って切り捨てないよう、番号で全ページを取得する。
    for number in 0..document.pages().len() {
        let mut page = document
            .pages()
            .get(number)
            .with_context(|| format!("Failed to load PDF page {}", number + 1))?;
        let height = page.height().value as f64;
        let width = page.width().value as f64;
        let (view_box, rotation) = crate::geometry::read_page_geometry(&page)
            .with_context(|| format!("Invalid geometry on PDF page {}", number + 1))?;
        let mut blocks: Vec<BlockIr> = Vec::new();

        let max_text_bytes = limits.max_input_mb.saturating_mul(1024 * 1024);
        let user_unit = units.get(usize::from(number)).ok();
        crate::physical_text::prepare(&mut page, &unicode[usize::from(number)], view_box)?;
        let extracted = crate::text::extract_runs(
            &mut page,
            &unicode[usize::from(number)],
            user_unit,
            limits.max_blocks_per_page,
            max_text_bytes
                .saturating_sub(text_bytes)
                .min(max_text_bytes.saturating_sub(source_characters)),
        )
        .with_context(|| format!("Failed to extract PDF page {}", number + 1))?;
        // Count whitespace and control characters too: normalization must not let
        // a compressed whitespace-only document bypass the work budget.
        source_characters = source_characters
            .checked_add(extracted.source_characters)
            .ok_or_else(|| anyhow!("PDF source text size overflow"))?;
        for run in extracted.runs {
            let content = normalize(&run.text);
            if content.is_empty() {
                continue;
            }
            text_bytes = text_bytes
                .checked_add(content.len() as u64)
                .ok_or_else(|| anyhow!("PDF extracted text size overflow"))?;
            ensure_size(text_bytes, limits)?;
            blocks.push(BlockIr {
                index: blocks.len() + 1,
                text: content,
                bounds: run.bounds,
                size: run.size,
            });
        }

        pages.push(PageIr {
            index: usize::from(number) + 1,
            width,
            height,
            user_unit,
            rotation,
            view_box: Some(view_box),
            sdk_unicode_mapping_errors: extracted.sdk_unicode_mapping_errors,
            sdk_text_objects_without_characters: unicode[usize::from(number)]
                .sdk_text_objects_without_characters,
            source_text_characters_missing: unicode[usize::from(number)]
                .source_text_characters_missing,
            source_text_accounting_unavailable: unicode[usize::from(number)]
                .source_text_accounting_unavailable,
            source_unicode_scalars_unmatched: unicode[usize::from(number)]
                .source_unicode_scalars_unmatched,
            source_unicode_accounting_unavailable: unicode[usize::from(number)]
                .source_unicode_accounting_unavailable,
            reading_order_requires_review: extracted.reading_order_requires_review,
            blocks,
        });
    }

    Ok((!pages.is_empty()).then_some(DocumentIr { pages }))
}
