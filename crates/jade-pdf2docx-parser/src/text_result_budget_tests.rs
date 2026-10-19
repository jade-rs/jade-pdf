//! 文字結果slot・属性コピー・結合の累積保存容量を固定する。
use super::*;

/// 最初の4個のCTM変更点を保存する要求容量。
const MATRIX_BYTES: u64 = 4 * std::mem::size_of::<(usize, [f32; 6])>() as u64;

/// 指定容量で、ページ本文と空のFont情報から文字結果を抽出する。
fn extract(
    limit: u64,
    source: &[u8],
    names: &BTreeMap<String, String>,
) -> (DecodeBudget, Vec<ParsedTextElement>) {
    let target = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    });
    let results = extract_text_elements_from_content_stream(
        &target,
        0,
        source,
        names,
        &BTreeMap::new(),
        &BTreeMap::new(),
        &ToUnicodeCMap::default(),
        &BTreeMap::new(),
        |_| Default::default(),
    );
    (target, results)
}

/// 最初の4slot分とraw/output byteを確保前に予約する。
#[test]
fn text_results_require_full_slot_capacity() {
    let total = MATRIX_BYTES + 2 + 4 * std::mem::size_of::<ParsedTextElement>() as u64;
    for limit in [total - 1, total] {
        let (target, results) = extract(limit, b"BT (x) Tj ET", &BTreeMap::new());
        assert_eq!(results.len(), usize::from(limit == total));
        assert_eq!(
            matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == total),
            limit < total
        );
    }
}

/// 同一描画の結合で、新しい全文字列の容量を累積へ数える。
#[test]
fn merged_text_storage_is_inclusive() {
    let total = MATRIX_BYTES + 6 + 4 * std::mem::size_of::<ParsedTextElement>() as u64;
    for limit in [total - 1, total] {
        let (target, results) = extract(limit, b"BT [(x) (y)] TJ ET", &BTreeMap::new());
        if limit == total {
            assert_eq!(results[0].text, "xy");
        } else {
            assert!(results.is_empty());
        }
        assert_eq!(
            matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == total),
            limit < total
        );
    }
}

/// Font/色は結果へ保存するコピー容量も累積予約する。
#[test]
fn result_font_and_color_copies_require_capacity() {
    let names = BTreeMap::from([("F".into(), "LongFont".into())]);
    let total = MATRIX_BYTES + 23 + 4 * std::mem::size_of::<ParsedTextElement>() as u64;
    for (limit, actual) in [
        (MATRIX_BYTES + 16, MATRIX_BYTES + 17),
        (MATRIX_BYTES + 22, MATRIX_BYTES + 23),
        (total - 1, total),
        (total, 0),
    ] {
        let (target, results) = extract(limit, b"BT /F 12 Tf 1 0 0 rg (x) Tj ET", &names);
        if actual == 0 {
            assert_eq!(results[0].font_name.as_deref(), Some("LongFont"));
            assert_eq!(results[0].fill_color.as_deref(), Some("FF0000"));
            assert!(target.check().is_ok());
        } else {
            assert!(results.is_empty());
            assert!(
                matches!(target.check(), Err(ParseError::ResourceLimit { actual: requested, .. }) if requested == actual)
            );
        }
    }
}

/// 5要素目は倍増後の全slotを予約し、先行する4要素も部分公開しない。
#[test]
fn text_result_growth_discards_partial_results() {
    let total = 3 * MATRIX_BYTES + 10 + 12 * std::mem::size_of::<ParsedTextElement>() as u64;
    let source = b"BT (x) Tj ET BT (x) Tj ET BT (x) Tj ET BT (x) Tj ET BT (x) Tj ET";
    for limit in [total - 1, total] {
        let (target, results) = extract(limit, source, &BTreeMap::new());
        assert_eq!(results.len(), if limit == total { 5 } else { 0 });
        assert_eq!(
            matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == total),
            limit < total
        );
    }
}

/// 小さいPDFのCMap出力増幅も、ページ/Formの公開APIで容量エラーとして返す。
#[test]
fn public_page_and_form_reject_amplified_text_output() {
    let mapping = format!(
        "1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<41> <{}>\nendbfchar\n",
        "0041".repeat(8192)
    );
    let source = format!("BT /F1 12 Tf <{}> Tj ET", "41".repeat(2048));
    let limits = ParseLimits {
        max_total_metadata_workspace_bytes: 1024 * 1024,
        ..Default::default()
    };
    for form in [false, true] {
        let pdf = super::content_bytes_tests::fixture(source.as_bytes(), mapping.as_bytes(), form);
        assert!(pdf.len() < 64 * 1024);
        assert!(
            matches!(parse_pdf_bytes_with_limits("amplified.pdf", &pdf, &limits), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit: 1048576, actual }) if actual >= 16 * 1024 * 1024)
        );
    }
}

/// 幅計算の後続code復号で停止すると、先に保存した文字も部分公開しない。
#[test]
fn later_font_code_failure_discards_prior_text_results() {
    for base_font in ["Courier", "Unknown"] {
        let prepared = crate::font_widths::Metrics::read(
            &crate::page_attributes::Objects::new(),
            format!("<< /Subtype /Type1 /BaseFont /{base_font} >>").as_bytes(),
            &DecodeBudget::default(),
            &[],
        );
        let target = DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: MATRIX_BYTES
                + 16
                + 4 * std::mem::size_of::<ParsedTextElement>() as u64,
            ..Default::default()
        });
        let names = BTreeMap::from([("F".into(), base_font.into())]);
        let metrics = BTreeMap::from([("F".into(), std::sync::Arc::new(prepared))]);
        let values = extract_text_elements_from_content_stream(
            &target,
            0,
            b"BT /F 12 Tf (x) Tj ET BT (AB) Tj ET",
            &names,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &ToUnicodeCMap::default(),
            &metrics,
            |_| Default::default(),
        );
        assert!(values.is_empty());
        assert!(
            matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == MATRIX_BYTES + 17 + 4 * std::mem::size_of::<ParsedTextElement>() as u64)
        );
        assert!(target.is_stopped());
    }
}
