//! 構造を見て比較する PDF の差分ライブラリ。
//!
//! ページとテキストブロックを先に対応付けてから中身を比べるので、途中に
//! 1 ページ挟まっても以降が全部差分になることはない。粒度は
//! ページ → テキストブロック → 文字 の 3 段。
//!
//! テキスト抽出のバックエンドは Pdfium。実体は `pdfium-auto` が用意する
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
//! ハイライトの座標を変換し直す必要が無くなる。
//!
//! ## 比較の対象外
//!
//! 図・画像・罫線・文字色・字体は比較しない。文字として取り出せるものと、
//! 抽出できた文字サイズを比べる。スキャンしただけの PDF (文字が入っていない
//! もの) は、テキストが空なので差分も出ない。

mod diff;
mod ir;
#[cfg(not(target_arch = "wasm32"))]
mod parse;
mod render;
mod scan;

pub use diff::{
    diff_documents, BlockChange, ChangeKind, Diagnostics, DiffOptions, DiffSummary, DocumentDiff,
    PageDiff, PageSize, Segment, SegmentKind, Status, DIFF_SCHEMA_VERSION,
};
pub use ir::{BlockIr, Bounds, DocumentIr, PageIr};
#[cfg(not(target_arch = "wasm32"))]
pub use parse::{load_document, load_document_blocking};
pub use scan::{load_document_from_bytes, Limits};

#[cfg(not(target_arch = "wasm32"))]
pub use render::{render_page, render_page_at_dpi, text_layer};
pub use render::{
    render_page_at_dpi_from_bytes, render_page_from_bytes, text_layer_from_bytes, RenderedPage,
    TextRun,
};

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
    let old = load_document_blocking(old_path, limits)?;
    let new = load_document_blocking(new_path, limits)?;
    Ok(diff_documents(
        &old_path.display().to_string(),
        &new_path.display().to_string(),
        &old,
        &new,
        options,
    ))
}

/// メモリ上の 2 ファイルを比較する (ブラウザ用)。抽出は Pdfium だけを使う。
pub fn diff_bytes(
    old_name: &str,
    new_name: &str,
    old_bytes: &[u8],
    new_bytes: &[u8],
    limits: &Limits,
    options: &DiffOptions,
) -> anyhow::Result<DocumentDiff> {
    let old = load_document_from_bytes(old_bytes, limits)?;
    let new = load_document_from_bytes(new_bytes, limits)?;
    Ok(diff_documents(old_name, new_name, &old, &new, options))
}
