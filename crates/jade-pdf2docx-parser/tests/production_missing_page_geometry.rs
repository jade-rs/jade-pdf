//! 寸法欠落ページの品質診断と後続ページの幾何対応を公開APIで固定する。
use jade_pdf2docx_parser::{
    ParseError, ParseOptions, QualityIssue, parse_pdf_bytes, parse_pdf_bytes_with_options,
};
use proptest::prelude::*;

/// strictはページ幾何の欠落を含む部分復元結果を成功として返さない。
#[test]
fn strict_rejects_missing_geometry() {
    let source = b"%PDF-1.7\n1 0 obj << /Type /Page >> endobj\n%%EOF\n";
    assert!(matches!(
        parse_pdf_bytes_with_options(
            "strict.pdf",
            source,
            &ParseOptions {
                strict: true,
                ..Default::default()
            }
        ),
        Err(ParseError::QualityRejected { .. })
    ));
}

/// 直接・間接nullと親のMediaBox欠落も寸法不明として同じ診断を返す。
#[test]
fn null_and_inherited_missing_geometry_are_reported() {
    for value in ["", "/MediaBox null", "/MediaBox 9 0 R"] {
        let source = format!(
            "%PDF-1.7\n1 0 obj << /Type /Page /Parent 2 0 R {value} >> endobj\n2 0 obj << /Type /Pages >> endobj\n9 0 obj null endobj\n%%EOF\n"
        );
        let parsed = parse_pdf_bytes("null.pdf", source.as_bytes()).unwrap();
        assert!(parsed.pages.is_empty());
        assert_eq!(
            parsed.quality.issues,
            vec![QualityIssue::MissingPageGeometry { page: 0 }]
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 欠落位置を変えてもページIDと欠落診断は元ページ順の独立モデルと一致する。
    #[test]
    fn sparse_page_numbers_follow_original_order(known in prop::collection::vec(any::<bool>(), 1..12)) {
        let mut source = String::from("%PDF-1.7\n");
        for (page, present) in known.iter().enumerate() {
            let geometry = if *present { "/MediaBox [10 20 110 220]" } else { "" };
            source.push_str(&format!("{} 0 obj << /Type /Page {geometry} >> endobj\n", page + 1));
        }
        source.push_str("%%EOF\n");
        let parsed = parse_pdf_bytes("model.pdf", source.as_bytes()).unwrap();
        let expected = known.iter().enumerate().filter_map(|(page, present)| present.then_some(page)).collect::<Vec<_>>();
        prop_assert_eq!(parsed.pages.iter().map(|page| page.page_number).collect::<Vec<_>>(), expected);
        let warnings = parsed.quality.issues.iter().map(ToString::to_string).collect::<Vec<_>>();
        prop_assert_eq!(warnings.len(), known.iter().filter(|present| !**present).count());
        for (page, present) in known.iter().enumerate() {
            if !present {
                let marker = format!("page {} ", page + 1);
                prop_assert!(warnings.iter().any(|issue| issue.contains("missing_page_geometry") && issue.contains(&marker)));
            }
        }
    }
}
