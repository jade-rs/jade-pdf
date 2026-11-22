//! 実際のWASM SDKで本文・行・幾何の独立した差分契約を確認する。
use wasm_bindgen::JsValue;

/// 折り返し・重複語・実際の挿入と順序変更を、単位・回転・描画順を跨いで検査する。
pub(crate) fn check(mode: &str) -> Result<(), JsValue> {
    for unit in [1. / 64., 1., 3., 64.] {
        for rotation in [0, 90, 180, 270] {
            for reverse in [false, true] {
                let old = crate::appearance_fixture::page_text_layout(
                    mode, false, unit, rotation, reverse,
                );
                let new = crate::appearance_fixture::page_text_layout(
                    mode, true, unit, rotation, reverse,
                );
                let load = |bytes: &[u8]| {
                    pdf_diff::load_document_from_bytes(bytes, &pdf_diff::Limits::default())
                        .map_err(|e| JsValue::from_str(&e.to_string()))
                };
                let before = load(&old)?;
                let after = load(&new)?;
                let diff = pdf_diff::diff_documents(
                    "a",
                    "b",
                    &before,
                    &after,
                    &pdf_diff::DiffOptions::default(),
                );
                let view = diff
                    .pages
                    .first()
                    .and_then(|p| p.text_diff.as_ref())
                    .ok_or_else(|| JsValue::from_str("Page text was not computed"))?;
                if diff.pages.len() != 1
                    || view.alignment_limited
                    || view.groups.is_empty() != matches!(mode, "wrap" | "repeat")
                    || !diff.is_text_comparison_complete()
                    || ((unit >= 1. || matches!(mode, "edit" | "order")) && !diff.has_changes())
                {
                    return Err(JsValue::from_str(
                        "Page text lost reflow, real edits, or source order",
                    ));
                }
            }
        }
    }
    Ok(())
}
