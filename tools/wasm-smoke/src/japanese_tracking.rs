//! 字間・宣言空白・正準分解・補助文字を、公開APIの実際のWASM出力で検査する。
use super::appearance_fixture;
use wasm_bindgen::JsValue;

/// 座標系と入力順を変え、本文・公開セルと原文に書かれた空白を独立した期待値と照合する。
pub(super) fn check(mode: &str) -> Result<(), JsValue> {
    let fixture_mode = if matches!(mode, "edit" | "gap") {
        "split"
    } else {
        mode
    };
    let expected = match mode {
        "coded" | "actual" | "gap" => "申 請 書 の",
        "latin" => "A B C D",
        "hangul" => "한 글 신 청",
        "edit" => "申請類の",
        "supplementary" => "\u{20000}\u{20001}\u{33479}の",
        "nfd" | "nfc" => "が請書の",
        _ => "申請書の",
    };
    let expected_layer = if mode == "nfd" {
        "か\u{3099}請書の"
    } else {
        expected
    };
    for unit in [1. / 64., 1., 3., 64.] {
        for rotation in [0, 90, 180, 270] {
            for reverse in [false, true] {
                let bytes = appearance_fixture::japanese_tracking(
                    fixture_mode,
                    unit,
                    rotation,
                    reverse,
                    if mode == "gap" { 0.35 } else { 0.17 },
                    mode == "edit",
                );
                let ir = pdf_diff::load_document_from_bytes(&bytes, &pdf_diff::Limits::default())
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                let layer = pdf_diff::text_layer_from_bytes(&bytes, 1)
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                if ir.pages[0].text() != expected
                    || layer.iter().map(|r| r.text.as_str()).collect::<String>() != expected_layer
                    || layer.iter().map(|r| r.chars.len()).sum::<usize>()
                        != expected_layer.chars().count()
                    || ir.pages[0].sdk_unicode_mapping_errors != 0
                    || ir.pages[0].source_unicode_scalars_unmatched != 0
                    || mode == "actual"
                        && !layer
                            .iter()
                            .all(|r| r.source_text_region && !r.source_regions.is_empty())
                {
                    return Err(JsValue::from_str("Japanese tracking changed text, literal spaces, scalar spans or source ownership under geometry"));
                }
            }
        }
    }
    if mode == "nfd" {
        let old = appearance_fixture::japanese_tracking("nfd", 1., 0, false, 0.17, false);
        let new = appearance_fixture::japanese_tracking("nfc", 1., 0, false, 0.17, false);
        let diff = pdf_diff::diff_bytes(
            "nfd",
            "nfc",
            &old,
            &new,
            &pdf_diff::Limits::default(),
            &pdf_diff::DiffOptions::default(),
        )
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
        if !diff.is_comparison_complete() || diff.has_changes() {
            return Err(JsValue::from_str(
                "Canonical voicing-mark decomposition introduced a false tracking difference",
            ));
        }
    }
    Ok(())
}
