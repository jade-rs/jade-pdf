//! PDF ファイルを読んで [`DocumentIr`] にする。
//!
//! 抽出と描画は同じ Pdfium を使う。ハイライトは [`crate::PageViewport`] で
//! 画像のピクセル座標へ投影する。Pdfium の実体はハッシュを固定して取得する (`bundled-pdfium` で
//! 実行ファイルへ埋め込める)。
//!
//! ファイルパスを使うのでネイティブ専用。ブラウザでは
//! [`crate::scan::load_document_from_bytes`] を使う。

use std::path::Path;

use anyhow::{anyhow, Result};

use crate::ir::DocumentIr;
use crate::scan::{scan_document, Limits};

/// PDF を読み込む。CPU を長く使うので、呼び出し側ではブロッキングスレッドへ。
///
/// テキストが 1 つも無い PDF (スキャンした紙など) は、ページの枠だけ入った
/// [`DocumentIr`] を返す。
pub fn load_document(path: &Path, limits: &Limits) -> Result<DocumentIr> {
    limits.validate()?;
    let size = std::fs::metadata(path)
        .map_err(|e| anyhow!("failed to read {}: {e}", path.display()))?
        .len();
    crate::scan::ensure_size(size, limits)?;
    let pdfium = crate::render::bind()?;
    let (document, (unicode, units)) =
        crate::input::load_file_with(&pdfium, path, limits, |handle| {
            let count = pdfium.bindings().FPDF_GetPageCount(handle.cast()) as usize;
            let units = crate::page_units::from_file_for_text(path, count, 0..count);
            let unicode = crate::unicode_facts::read(&pdfium, handle, limits, None, &units)?;
            Ok((unicode, units))
        })
        .map_err(|e| anyhow!("{}", readable_error(&format!("pdfium parse error: {e}"))))?;
    scan_document(&document, limits, &unicode, &units)?
        .ok_or_else(|| anyhow!("PDF にページがありません"))
}

/// 以前は内部で非同期ランタイムを立てていたときの名前。呼び出し側の互換のため残す。
pub fn load_document_blocking(path: &Path, limits: &Limits) -> Result<DocumentIr> {
    load_document(path, limits)
}

/// 解析器のエラーを人が読める長さに整える。
///
/// Pdfium が見つからないときの dlopen エラーは原因が分かりにくいので言い換える。
fn readable_error(message: &str) -> String {
    const LIMIT: usize = 200;

    if message.to_lowercase().contains("pdfium") {
        if message.contains("dlopen") || message.contains("no such file") {
            return "PDF の解析に必要な Pdfium を読み込めませんでした".to_string();
        }
        return format!(
            "Pdfium が PDF を開けませんでした: {}",
            message.chars().take(120).collect::<String>()
        );
    }

    let head: String = message
        .lines()
        .next()
        .unwrap_or(message)
        .chars()
        .take(LIMIT)
        .collect();
    if head.chars().count() < message.chars().count() {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_error_shortens_dumps() {
        let long = format!("failed to parse. {}", "x".repeat(500));
        let out = readable_error(&long);
        assert!(
            out.chars().count() <= 201,
            "length: {}",
            out.chars().count()
        );
        assert!(out.ends_with('…'));
    }

    #[test]
    fn readable_error_explains_a_missing_pdfium() {
        let out = readable_error(
            "pdfium parse error: /tmp/a.pdf (dlopen(libpdfium.dylib): no such file)",
        );
        assert!(out.contains("Pdfium"), "unexpected message: {out}");
        assert!(out.contains("読み込めません"));
    }

    #[test]
    fn readable_error_keeps_short_messages() {
        let out = readable_error("file not found: /tmp/none.pdf");
        assert_eq!(out, "file not found: /tmp/none.pdf");
    }
}
