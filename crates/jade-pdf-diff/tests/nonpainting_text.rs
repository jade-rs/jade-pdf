use pdf_diff::{diff_bytes, load_document_from_bytes, render_page_from_bytes, DiffOptions, Limits};

#[path = "support/nonpainting_fixture.rs"]
mod fixture;
use fixture::source;

/// 描画されない文字の内容だけを変えても、可視本文の描画は全pixel同じである。
#[test]
fn hidden_quantity_edits_do_not_change_visible_pixels() {
    for mode in [3, 7] {
        let old = source(mode, "Recorded 1/4");
        let new = source(mode, "Recorded 1/8");
        let a = render_page_from_bytes(&old, 1, 600).unwrap();
        let b = render_page_from_bytes(&new, 1, 600).unwrap();
        assert_eq!((a.width, a.height), (b.width, b.height));
        assert_eq!(
            a.png, b.png,
            "nonpainting mode {mode} altered visible content"
        );
    }
}

/// 通常の可視文字は追加の非描画診断を持たず、従来どおり本文を比較できる。
#[test]
fn painted_text_keeps_text_comparison_complete() {
    let bytes = source(0, "Painted");
    let result = diff_bytes(
        "old",
        "new",
        &bytes,
        &bytes,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(
        result.is_text_comparison_complete(),
        "{:?}",
        result.diagnostics
    );
}

/// 非描画文字を消さずraw本文に保持し、SDKから観測した件数を別の事実として公開する。
#[test]
fn nonpainting_records_are_retained_and_explicitly_counted() {
    for mode in [3, 7] {
        let bytes = source(mode, "Hidden");
        let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
        assert!(ir.pages[0].text().contains("Visible 1/8"));
        assert!(ir.pages[0].text().contains("Hidden"));
        let value = serde_json::to_value(&ir.pages[0]).unwrap();
        assert_eq!(
            value
                .get("nonpainting_text_characters")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            6,
            "mode {mode}: {value}"
        );
    }
}

/// 非表示層の比較成功を可視OCR本文の確認済みとして扱わず、同一sourceの外観同一性は維持する。
#[test]
fn nonpainting_text_requires_visible_review_without_discarding_source_proof() {
    for mode in [3, 7] {
        let bytes = source(mode, "Hidden");
        let result = diff_bytes(
            "old",
            "new",
            &bytes,
            &bytes,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(
            !result.is_text_comparison_complete(),
            "mode {mode} incorrectly claims verified text: {:?}",
            result.diagnostics
        );
        assert!(
            result.is_comparison_complete(),
            "identical source-byte proof must remain sufficient for appearance identity"
        );
        assert!(!result.has_changes());
    }
}

/// file・bytes・文字レイヤー・JSONで同じページ診断を返し、原本も保持する。
#[test]
fn file_and_text_layer_routes_preserve_visibility_facts() {
    let bytes = source(3, "Hidden");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.pdf");
    std::fs::write(&path, &bytes).unwrap();
    let limits = Limits::default();
    let a = load_document_from_bytes(&bytes, &limits).unwrap();
    let b = pdf_diff::load_document(&path, &limits).unwrap();
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(&b).unwrap()
    );
    let layer = pdf_diff::text_layer_from_bytes(&bytes, 1).unwrap();
    let file_layer = pdf_diff::text_layer(&path, 1).unwrap();
    assert_eq!(
        serde_json::to_value(&layer).unwrap(),
        serde_json::to_value(&file_layer).unwrap()
    );
    assert!(!layer.is_empty());
    assert!(layer.iter().all(|run| run.nonpainting_text_characters == 6
        && !run.text_visibility_accounting_unavailable
        && !run.source_unicode_accounting_unavailable));
    let result = pdf_diff::diff_files(&path, &path, &limits, &DiffOptions::default()).unwrap();
    let restored: pdf_diff::DocumentDiff =
        serde_json::from_value(serde_json::to_value(&result).unwrap()).unwrap();
    assert!(restored.diagnostics.has_unverified_text_visibility());
    assert!(!restored.is_text_comparison_complete());
    assert!(restored.is_comparison_complete());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

/// 非表示の原文変更は消さずに検出し、同じ描画でも可視本文の確認済みとはしない。
#[test]
fn hidden_edits_keep_raw_source_changes_and_review() {
    let old = source(3, "Recorded 1/4");
    let new = source(3, "Recorded 1/8");
    let result = diff_bytes(
        "old",
        "new",
        &old,
        &new,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert!(result.has_changes());
    assert_eq!(result.summary.pages_visual_changed, 0);
    assert!(!result.is_text_comparison_complete());
    assert!(!result.is_comparison_complete());
    assert!(result.diagnostics.has_unverified_text_visibility());
}

/// 取得不能と旧JSONの省略を区別し、未知の診断が往復後にも要確認を保つ。
#[test]
fn unavailable_visibility_survives_external_ir_and_legacy_defaults() {
    let ir = load_document_from_bytes(&source(0, "Painted"), &Limits::default()).unwrap();
    let mut legacy = serde_json::to_value(&ir).unwrap();
    let page = legacy["pages"][0].as_object_mut().unwrap();
    page.remove("nonpainting_text_characters");
    page.remove("text_visibility_accounting_unavailable");
    let restored: pdf_diff::DocumentIr = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(restored.pages[0].nonpainting_text_characters, 0);
    assert!(!restored.pages[0].text_visibility_accounting_unavailable);
    legacy["pages"][0]["text_visibility_accounting_unavailable"] = serde_json::Value::Bool(true);
    let uncertain: pdf_diff::DocumentIr = serde_json::from_value(legacy).unwrap();
    let result = pdf_diff::try_diff_documents(
        "old",
        "new",
        &uncertain,
        &restored,
        &Limits::default(),
        &DiffOptions::default(),
    )
    .unwrap();
    assert_eq!(
        result
            .diagnostics
            .old_pages_with_unavailable_text_visibility,
        1
    );
    assert_eq!(
        result
            .diagnostics
            .new_pages_with_unavailable_text_visibility,
        0
    );
    assert!(result.diagnostics.has_unverified_text_visibility());
    assert!(!result.is_text_comparison_complete());
}

/// 全文を空にする確認済みActualTextは、非表示のSDKアンカーを出力本文へ戻さない。
#[test]
fn verified_empty_source_replacement_keeps_an_empty_text_layer() {
    let bytes = fixture::empty_actualtext_source();
    let ir = load_document_from_bytes(&bytes, &Limits::default()).unwrap();
    assert!(ir.pages[0].text().is_empty());
    assert!(ir.pages[0].nonpainting_text_characters > 0);
    assert!(!ir.pages[0].source_unicode_accounting_unavailable);
    assert!(pdf_diff::text_layer_from_bytes(&bytes, 1)
        .unwrap()
        .is_empty());
}
