//! 構造を見て比較する PDF の差分ライブラリ。
//!
//! ページとテキストブロックを先に対応付けてから中身を比べるので、途中に
//! 1 ページ挟まっても以降が全部差分になることはない。粒度は
//! ページ → テキストブロック → 文字 の 3 段。
//!
//! テキスト抽出のバックエンドは Pdfium。既定の実体はハッシュを固定して取得する
//! (`bundled-pdfium` feature でビルド時に実行ファイルへ埋め込める)。座標は PDF ユーザ空間
//! のまま持ち回るので、UI 側はページ画像の上に変更箇所をぴたりと重ねられる。
//!
//! ```no_run
//! use std::path::Path;
//! use pdf_diff::{diff_documents, load_document_blocking, DiffOptions, Limits};
//!
//! let old = load_document_blocking(Path::new("old.pdf"), &Limits::default())?;
//! let new = load_document_blocking(Path::new("new.pdf"), &Limits::default())?;
//! let diff = diff_documents("old.pdf", "new.pdf", &old, &new, &DiffOptions::default());
//! println!("{} pages changed", diff.summary.pages_modified);
//! # Ok::<(), anyhow::Error>(())
//! ```
//!
//! ブラウザ (WASM) では [`diff_bytes`] / [`render_page_from_bytes`] /
//! [`text_layer_from_bytes`] を使う。Pdfium は JS 側で読み込んだ Emscripten
//! モジュールにつなぐ。
//!
//! 表示に使うページ画像とテキストレイヤも同じ Pdfium から取れる
//! ([`render_page`] / [`text_layer`])。抽出と描画を同じエンジンに揃えることで、
//! [`RenderedPage::viewport`] の変換でハイライトを画像のピクセル座標へ投影できる。
//!
//! ## 画像・フォームと比較範囲
//!
//! [`diff_files`] / [`diff_bytes`] は既定で144DPIの描画も照合し、画像・線図・
//! 文字色・字体の見た目の変更を返す。Widgetの保存値も表示とは別に比較する。
//! [`diff_documents`] / [`try_diff_documents`] はIRによるテキスト比較のみ。
//! [`DocumentDiff::is_comparison_complete`] で描画上限や未対応の対話要素を確認する。
//! これは指定解像度での照合であり、任意のPDFオブジェクトの完全同値判定ではない。

#[cfg(test)]
#[allow(dead_code)]
#[path = "../tests/support/appearance_fixture.rs"]
mod appearance_fixture;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../tests/support/report_fixture.rs"]
mod report_fixture;

mod actual_text;
mod canonical_forms;
mod comparison;
mod diff;
mod geometry;
mod input;
mod ir;
#[cfg(not(target_arch = "wasm32"))]
pub mod isolation;
#[cfg(not(target_arch = "wasm32"))]
mod native_runtime;
mod page_units;
#[cfg(not(target_arch = "wasm32"))]
mod parse;
#[cfg(not(target_arch = "wasm32"))]
#[cfg_attr(feature = "bundled-pdfium", allow(dead_code))]
mod pdfium_assets;
mod physical_text;
mod raster_limits;
mod render;
mod scan;
mod sdk_probes;
mod source_text;
mod tagged_table;
mod text;
mod unicode_facts;
mod widget_values;

pub use canonical_forms::{CanonicalFormChange, CanonicalFormField};
pub use comparison::{
    ComparisonCoverage, FormFieldChange, FormFieldSnapshot, VisualDiffOptions, VisualPageDiff,
    VisualStatus,
};
pub use diff::{
    diff_documents, BlockChange, ChangeKind, Diagnostics, DiffOptions, DiffSummary, DocumentDiff,
    PageDiff, PageSize, Segment, SegmentKind, Status, DIFF_SCHEMA_VERSION,
};
pub use geometry::{AffineTransform, PageViewport};
pub use ir::{BlockIr, Bounds, DocumentIr, PageIr};
#[cfg(not(target_arch = "wasm32"))]
pub use native_runtime::pdfium_library_path;
#[cfg(not(target_arch = "wasm32"))]
pub use pdfium_assets::PDFIUM_VERSION;
/// License notices distributed with the default Pdfium SDK.
pub const PDFIUM_LICENSE_NOTICES: &str = include_str!("../licenses/pdfium-8111.txt");

/// License notice for the pinned Adobe Glyph List used by source diagnostics.
pub const ADOBE_GLYPH_LIST_LICENSE: &str = include_str!("../licenses/adobe-glyphlist.txt");
#[cfg(not(target_arch = "wasm32"))]
pub use parse::{load_document, load_document_blocking};
pub use scan::{load_document_from_bytes, Limits};

#[cfg(not(target_arch = "wasm32"))]
pub use render::{render_page, render_page_at_dpi, text_layer};
pub use render::{
    render_page_at_dpi_from_bytes, render_page_from_bytes, text_layer_from_bytes, RenderedPage,
    SourceTextRegion, TextRun,
};

/// Validate external/OCR IR, resource limits and options before comparison.
/// Prefer this over `diff_documents` when the input is not already validated.
pub fn try_diff_documents(
    old_name: &str,
    new_name: &str,
    old: &DocumentIr,
    new: &DocumentIr,
    limits: &Limits,
    options: &DiffOptions,
) -> anyhow::Result<DocumentDiff> {
    options.validate()?;
    old.validate(limits)?;
    new.validate(limits)?;
    Ok(diff_documents(old_name, new_name, old, new, options))
}

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

/// 2 ファイルを読み込んで比較する (同期)。
#[cfg(not(target_arch = "wasm32"))]
pub fn diff_files(
    old_path: &Path,
    new_path: &Path,
    limits: &Limits,
    options: &DiffOptions,
) -> anyhow::Result<DocumentDiff> {
    options.validate()?;
    limits.validate()?;
    crate::scan::ensure_size(std::fs::metadata(old_path)?.len(), limits)?;
    crate::scan::ensure_size(std::fs::metadata(new_path)?.len(), limits)?;
    let pdfium = crate::render::bind()?;
    let (old, (old_unicode, old_units)) =
        crate::input::load_file_with(&pdfium, old_path, limits, |handle| {
            let count = pdfium.bindings().FPDF_GetPageCount(handle.cast()) as usize;
            let units = crate::page_units::from_file_for_text(old_path, count, 0..count);
            let unicode = crate::unicode_facts::read(&pdfium, handle, limits, None, &units)?;
            Ok((unicode, units))
        })?;
    let (new, (new_unicode, new_units)) =
        crate::input::load_file_with(&pdfium, new_path, limits, |handle| {
            let count = pdfium.bindings().FPDF_GetPageCount(handle.cast()) as usize;
            let units = crate::page_units::from_file_for_text(new_path, count, 0..count);
            let unicode = crate::unicode_facts::read(&pdfium, handle, limits, None, &units)?;
            Ok((unicode, units))
        })?;
    let same_source = crate::comparison::equal_files(old_path, new_path)?;
    let old_requires_forms = old.form().is_some() || old_units.requires_form_analysis();
    let new_requires_forms = new.form().is_some() || new_units.requires_form_analysis();
    let source = crate::comparison::SourceFacts {
        same_source,
        old_units,
        new_units,
        old_unicode,
        new_unicode,
        old_widgets: if !same_source && old.form().is_some() {
            crate::widget_values::from_file(&pdfium, old_path, limits)?
        } else {
            Default::default()
        },
        new_widgets: if !same_source && new.form().is_some() {
            crate::widget_values::from_file(&pdfium, new_path, limits)?
        } else {
            Default::default()
        },
        old_canonical: if !same_source && old_requires_forms {
            crate::canonical_forms::from_file(old_path, limits).map_err(|e| format!("{e:#}"))
        } else {
            Ok(crate::canonical_forms::FormSnapshot::default())
        },
        new_canonical: if !same_source && new_requires_forms {
            crate::canonical_forms::from_file(new_path, limits).map_err(|e| format!("{e:#}"))
        } else {
            Ok(crate::canonical_forms::FormSnapshot::default())
        },
    };
    let mut diff = compare_loaded(
        &old_path.display().to_string(),
        &new_path.display().to_string(),
        &old,
        &new,
        limits,
        options,
        &source,
    )?;
    drop((old, new));
    let old = crate::input::load_file(&pdfium, old_path, limits)?;
    let new = crate::input::load_file(&pdfium, new_path, limits)?;
    crate::comparison::supplement(&mut diff, &old, &new, limits, options, &source)?;
    Ok(diff)
}

/// メモリ上の 2 ファイルを比較する (ブラウザ用)。抽出・描画は同じ Pdfium を使う。
pub fn diff_bytes(
    old_name: &str,
    new_name: &str,
    old_bytes: &[u8],
    new_bytes: &[u8],
    limits: &Limits,
    options: &DiffOptions,
) -> anyhow::Result<DocumentDiff> {
    options.validate()?;
    limits.validate()?;
    crate::scan::ensure_size(old_bytes.len() as u64, limits)?;
    crate::scan::ensure_size(new_bytes.len() as u64, limits)?;
    let pdfium = crate::render::bind()?;
    let (old, (old_unicode, old_units)) =
        crate::input::load_bytes_with(&pdfium, old_bytes, limits, |handle| {
            let count = pdfium.bindings().FPDF_GetPageCount(handle.cast()) as usize;
            let units = crate::page_units::from_bytes_for_text(old_bytes, count, 0..count);
            let unicode = crate::unicode_facts::read(&pdfium, handle, limits, None, &units)?;
            Ok((unicode, units))
        })?;
    let (new, (new_unicode, new_units)) =
        crate::input::load_bytes_with(&pdfium, new_bytes, limits, |handle| {
            let count = pdfium.bindings().FPDF_GetPageCount(handle.cast()) as usize;
            let units = crate::page_units::from_bytes_for_text(new_bytes, count, 0..count);
            let unicode = crate::unicode_facts::read(&pdfium, handle, limits, None, &units)?;
            Ok((unicode, units))
        })?;
    let same_source = old_bytes == new_bytes;
    let old_requires_forms = old.form().is_some() || old_units.requires_form_analysis();
    let new_requires_forms = new.form().is_some() || new_units.requires_form_analysis();
    let source = crate::comparison::SourceFacts {
        same_source,
        old_units,
        new_units,
        old_unicode,
        new_unicode,
        old_widgets: if !same_source && old.form().is_some() {
            crate::widget_values::from_bytes(&pdfium, old_bytes, limits)?
        } else {
            Default::default()
        },
        new_widgets: if !same_source && new.form().is_some() {
            crate::widget_values::from_bytes(&pdfium, new_bytes, limits)?
        } else {
            Default::default()
        },
        old_canonical: if !same_source && old_requires_forms {
            crate::canonical_forms::from_bytes(old_bytes, limits).map_err(|e| format!("{e:#}"))
        } else {
            Ok(crate::canonical_forms::FormSnapshot::default())
        },
        new_canonical: if !same_source && new_requires_forms {
            crate::canonical_forms::from_bytes(new_bytes, limits).map_err(|e| format!("{e:#}"))
        } else {
            Ok(crate::canonical_forms::FormSnapshot::default())
        },
    };
    let mut diff = compare_loaded(old_name, new_name, &old, &new, limits, options, &source)?;
    drop((old, new));
    let old = crate::input::load_bytes(&pdfium, old_bytes, limits)?;
    let new = crate::input::load_bytes(&pdfium, new_bytes, limits)?;
    crate::comparison::supplement(&mut diff, &old, &new, limits, options, &source)?;
    Ok(diff)
}

fn compare_loaded(
    old_name: &str,
    new_name: &str,
    old: &pdfium_render::prelude::PdfDocument<'_>,
    new: &pdfium_render::prelude::PdfDocument<'_>,
    limits: &Limits,
    options: &DiffOptions,
    source: &crate::comparison::SourceFacts,
) -> anyhow::Result<DocumentDiff> {
    let old_ir = crate::scan::scan_document(old, limits, &source.old_unicode, &source.old_units)?
        .ok_or_else(|| anyhow::anyhow!("Old PDF has no pages"))?;
    let new_ir = crate::scan::scan_document(new, limits, &source.new_unicode, &source.new_units)?
        .ok_or_else(|| anyhow::anyhow!("New PDF has no pages"))?;
    try_diff_documents(old_name, new_name, &old_ir, &new_ir, limits, options)
}

#[cfg(target_arch = "wasm32")]
mod wasm_runtime;
