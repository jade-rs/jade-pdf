//! native文字対応の容量・失敗伝播と幾何/style保持の回帰。
use super::{apply_pdfium_text, reconcile_pdfium_characters};
use crate::{ParseError, ParseLimits, ParsedTextElement, decode::DecodeBudget};

/// 指定したmetadata容量で独立した予算を作る。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    })
}

/// 既存の幾何/style回帰と同じ要素を作る。
fn element(text: &str, page: usize) -> ParsedTextElement {
    ParsedTextElement {
        page,
        bounds: crate::ParsedBounds {
            x0: 10.,
            y0: 10.,
            x1: 50.,
            y1: 20.,
        },
        font_size: 10.,
        font_name: None,
        fill_color: None,
        text: text.into(),
    }
}

/// 公開動作から容量ちょうどの境界を探し、非ゼロ予約を要求する。
fn minimum_budget(pure: &[ParsedTextElement], native: &[ParsedTextElement]) -> u64 {
    let mut lower = 0;
    let mut upper = 16384;
    assert!(
        reconcile_pdfium_characters(&budget(upper), pure, native, &Default::default()).is_some()
    );
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        if reconcile_pdfium_characters(&budget(middle), pure, native, &Default::default()).is_some()
        {
            upper = middle;
        } else {
            lower = middle + 1;
        }
    }
    assert!(lower > 0, "native reconciliation must reserve workspace");
    lower
}

/// 文字対応の全配列・本文・属性は同じ累積予算を使う。
#[test]
fn reconciliation_reserves_unicode_and_attribute_bytes() {
    let mut pure = vec![element("a b", 0)];
    pure[0].font_name = None;
    pure[0].fill_color = None;
    let native = vec![element("日🙂", 0)];
    let baseline = minimum_budget(&pure, &native);
    pure[0].font_name = Some("日本語Font".into());
    pure[0].fill_color = Some("123ABC".into());
    let required = minimum_budget(&pure, &native);
    assert_eq!(required - baseline, "日本語Font".len() as u64 + 6);
    let allowed = budget(required);
    let actual =
        reconcile_pdfium_characters(&allowed, &pure, &native, &Default::default()).unwrap();
    let mut expected = pure.clone();
    expected[0].text = "日 🙂".into();
    assert_eq!(actual, expected);
    assert!(allowed.check().is_ok());
    let stopped = budget(required - 1);
    assert!(reconcile_pdfium_characters(&stopped, &pure, &native, &Default::default()).is_none());
    assert!(matches!(
        stopped.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// 対応中の容量不足はfallbackで隠さず、元の本文/preview/診断を変更しない。
#[test]
fn reconciliation_failure_is_public_and_does_not_apply_partial_text() {
    let mut parsed = crate::parser::parse_pdf_bytes_pure(
        "native-reconcile-budget.pdf".into(),
        b"%PDF-1.4\n1 0 obj << /Type /Page /MediaBox [0 0 200 200] >> endobj\n",
    )
    .unwrap();
    parsed.text_elements = vec![element("a b", 0)];
    parsed.text_preview = "original preview".into();
    let native = vec![element("日🙂", 0)];
    let required = minimum_budget(&parsed.text_elements, &native);
    let original = parsed.clone();
    for capacity in [0, required - 1] {
        let stopped = budget(capacity);
        assert!(matches!(
            apply_pdfium_text(&stopped, &mut parsed, "replacement".into(), native.clone()),
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                ..
            })
        ));
        assert_eq!(parsed, original);
    }
}

/// 消費済みの停止状態でも再開せず、空入力にも成功を返さない。
#[test]
fn reconciliation_preserves_consumed_stop() {
    let stopped = budget(0);
    stopped.charge_metadata_workspace_bytes(1);
    assert!(stopped.check().is_err());
    let pure = vec![element("a", 0)];
    let native = vec![element("日", 0)];
    assert!(reconcile_pdfium_characters(&stopped, &pure, &native, &Default::default()).is_none());
    assert!(reconcile_pdfium_characters(&stopped, &[], &[], &Default::default()).is_none());
}

/// ページ数/文字数の不一致とFormの抽出順違いは、容量失敗と区別する。
#[test]
fn reconciliation_mismatch_is_not_resource_failure() {
    let target = budget(16384);
    let pure = vec![element("ab", usize::MAX)];
    let native = vec![element("日", usize::MAX)];
    assert!(reconcile_pdfium_characters(&target, &pure, &native, &Default::default()).is_none());
    assert!(target.check().is_ok());
    let native = vec![element("日本", 0)];
    assert!(reconcile_pdfium_characters(&target, &pure, &native, &Default::default()).is_none());
    assert!(target.check().is_ok());
    let quality = crate::QualityReport {
        issues: vec![crate::QualityIssue::UnsupportedXObject {
            page: usize::MAX,
            object_number: 5,
            reason: crate::UnsupportedXObjectReason::Form,
        }],
    };
    let native = vec![element("ba", usize::MAX)];
    assert!(reconcile_pdfium_characters(&target, &pure, &native, &quality).is_none());
    assert!(target.check().is_ok());
}

proptest::proptest! {
    /// ページが交互でもUnicode増幅とpureの空白・幾何/styleを保持する。
    #[test]
    fn reconciliation_preserves_page_grouping_and_attributes(
        lengths in proptest::collection::vec(1usize..12, 1..16),
    ) {
        let pure: Vec<_> = lengths.iter().enumerate().map(|(index, &length)| {
            let mut value = element(&"a ".repeat(length), index % 3);
            value.font_name = Some(format!("Font{index}"));
            value.fill_color = Some("123ABC".into());
            value.font_size += index as f32;
            value
        }).collect();
        let native: Vec<_> = lengths.iter().enumerate().map(|(index, &length)|
            element(&"🙂".repeat(length), index % 3)).collect();
        let mut expected = pure.clone();
        for (item, &length) in expected.iter_mut().zip(&lengths) {
            item.text = "🙂 ".repeat(length);
        }
        let target = budget(128 * 1024);
        proptest::prop_assert_eq!(reconcile_pdfium_characters(&target, &pure, &native, &Default::default()), Some(expected));
        proptest::prop_assert!(target.check().is_ok());
    }
}
