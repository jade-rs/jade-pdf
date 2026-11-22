use pdf_diff::{diff_bytes, load_document_from_bytes, render_page_from_bytes, DiffOptions, Limits};

/// 保存fixtureを使わず、独立した可視本文と非描画文字を持つ正式xref付きPDFを生成する。
fn source(mode: u8, recorded: &str) -> Vec<u8> {
    let content = format!("BT /F1 18 Tf 20 130 Td (Visible 1/8) Tj ET\nq BT /F1 18 Tf {mode} Tr 20 70 Td ({recorded}) Tj ET Q");
    let objects=["<< /Type /Catalog /Pages 2 0 R >>".into(),"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),format!("<< /Length {} >>\nstream\n{content}\nendstream",content.len()),"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into()];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

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
