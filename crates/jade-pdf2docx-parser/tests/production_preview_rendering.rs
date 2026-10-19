//! ページのない互換入力でもpreviewの文字描画モードを反映する。
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;

/// ページを作らず、正確なLengthを持つ独立した本文streamを保存する。
fn source(contents: &[&str]) -> Vec<u8> {
    let mut bytes = String::from("%PDF-1.4\n");
    for (index, content) in contents.iter().enumerate() {
        bytes.push_str(&format!(
            "{} 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
            index + 1,
            content.len()
        ));
    }
    bytes.push_str("%%EOF\n");
    bytes.into_bytes()
}

/// ページ本文がないことも確認して互換previewだけを返す。
fn preview(contents: &[&str]) -> String {
    let parsed = parse_pdf_bytes("compat-preview.pdf", &source(contents)).unwrap();
    assert_eq!(parsed.page_count, 0);
    assert!(parsed.text_elements.is_empty());
    parsed.text_preview
}

/// 三つの候補passで不可視モード3/7を復活させない。
#[test]
fn modes_filter_complete_text_objects_and_fallback_operands() {
    for mode in 0..8 {
        for content in [
            format!("{mode} Tr BT (MARK) Tj ET"),
            format!("{mode} Tr (MARK) Tj"),
            format!("{mode} Tr [(MA) 10 (RK)] TJ"),
        ] {
            let expected = if matches!(mode, 3 | 7) { "" } else { "MARK" };
            assert_eq!(preview(&[&content]), expected, "{content}");
        }
    }
}

/// BTを越えてモードを維持し、BT外側と内側のq/Qからも復帰する。
#[test]
fn graphics_restore_and_text_object_boundaries_preserve_mode() {
    assert_eq!(
        preview(&[
            "3 Tr q 0 Tr BT (LOCAL) Tj q 7 Tr (SECRET) Tj Q (VISIBLE) Tj ET Q BT (HIDDEN) Tj ET"
        ]),
        "LOCAL\nVISIBLE"
    );
}

/// 配列・辞書・文字列・コメント内の偽命令と不正Trを評価しない。
#[test]
fn containers_and_invalid_modes_do_not_change_rendering() {
    assert_eq!(
        preview(&[
            "7 Tr [0 Tr q] << /Fake 0 Tr /Other (Q) >> (0 Tr Q) % 0 Tr Q\n 8 Tr -1 Tr 0.5 Tr BT (SECRET) Tj ET 0 Tr BT (VISIBLE) Tj ET"
        ]),
        "VISIBLE"
    );
}

/// 別streamは独立し、不可視候補を重複索引へ保存しない。
#[test]
fn streams_reset_mode_and_visible_duplicates_survive() {
    assert_eq!(
        preview(&["3 Tr BT (SHARED) Tj ET", "BT (SHARED) Tj ET"]),
        "SHARED"
    );
}

/// BT内の優先順位を維持し、閉じないBTの候補はfallbackで回復する。
#[test]
fn candidate_priority_and_unclosed_text_recovery_are_preserved() {
    assert_eq!(
        preview(&["(OUTSIDE) Tj BT (INNER) Tj ET BT (RECOVERED) Tj"]),
        "INNER\nOUTSIDE\nRECOVERED"
    );
    assert_eq!(preview(&["3 Tr BT (SECRET) Tj"]), "");
}

/// container内の表示命令を除き、未完TJから後続BTの可視候補へ復帰する。
#[test]
fn containers_do_not_create_candidates_and_malformed_arrays_recover() {
    assert_eq!(
        preview(&["[BT (ARRAY) Tj ET] << /Fake (DICT) Tj >> (FALLBACK) Tj BT (INNER) Tj ET"]),
        "INNER\nFALLBACK"
    );
    assert_eq!(
        preview(&["BT [(BROKEN) Tj ET 3 Tr BT [(HIDDEN) Tj ET 0 Tr BT (RECOVERED) Tj ET"]),
        "RECOVERED"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 任意の描画モード列を独立したpaint判定と候補順序モデルで比較する。
    #[test]
    fn preview_matches_painted_mode_sequence(modes in prop::collection::vec(0u8..8, 1..25)) {
        let mut content = String::new();
        let mut expected = Vec::new();
        for (index, mode) in modes.into_iter().enumerate() {
            let text = format!("WORD{index}");
            content.push_str(&format!("{mode} Tr BT ({text}) Tj ET\n"));
            if !matches!(mode, 3 | 7) { expected.push(text); }
        }
        prop_assert_eq!(preview(&[&content]), expected.join("\n"));
    }
}
