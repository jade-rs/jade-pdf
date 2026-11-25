//! 同じForm内で完全に重なる、確認済みfontの描画object候補を保存する。
use crate::{ParseError, decode::DecodeBudget, pdfium_font_tags::Tagged};
use pdfium_render::prelude::*;

/// 個別に読み直す元object。fontのfamily名を識別子にしない。
pub(crate) struct Target {
    /// ページ内を深さ優先で数えたtext objectの順番。
    pub(crate) ordinal: usize,
    /// 解析ローカルの確認済みfont tag index。
    pub(crate) source_key: usize,
    /// 元FontDescriptorが宣言した整数weight。
    pub(crate) weight: u16,
}

/// 同じ局所矩形とfont sizeを持つ、まだ回復採用前のobject候補群。
pub(crate) struct Group {
    /// 元のpaint順に保持する描画object。
    pub(crate) targets: Vec<Target>,
}

/// 候補の収集だけを行い、Unicodeや幾何の回復成功とは扱わない。
pub(crate) fn collect<'a>(
    _budget: &DecodeBudget,
    _objects: &'a impl PdfPageObjectsCommon<'a>,
    _tags: &Tagged,
) -> Result<Vec<Group>, ParseError> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParseLimits;

    /// 所有CDF対照を同じSDK文書へ渡し、確認済みtagと借用ページを組み合わせる。
    fn with_page(check: impl FnOnce(&PdfPage<'_>, &Tagged)) {
        let bytes = crate::pdfium_subset_native_tests::identical_overprint_source();
        let budget = DecodeBudget::default();
        crate::parser::parse_pdf_bytes_pure_with_budget(
            "overprint.pdf".into(),
            &bytes,
            &ParseLimits::default(),
            &budget,
        )
        .unwrap();
        let mut tags = Tagged::prepare_weights(&budget, &bytes, false).unwrap();
        let outcome = {
            let sdk = pdfium_auto::bind_pdfium_silent().unwrap();
            let confirmed = tags.confirm_embedded(&budget, &sdk, 1000).unwrap();
            assert!(confirmed);
            let doc = sdk.load_pdf_from_byte_slice(&tags.bytes, None).unwrap();
            let page = doc.pages().get(0).unwrap();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&page, &tags)))
        };
        if let Err(error) = outcome {
            std::panic::resume_unwind(error);
        }
    }

    /// 同じfamily・位置でも別source resourceであることを保ち、元paint順を固定する。
    #[test]
    fn exact_overprint_group_preserves_distinct_source_resources_and_order() {
        with_page(|page, tags| {
            let groups = collect(&DecodeBudget::default(), page.objects(), tags).unwrap();
            assert_eq!(groups.len(), 1);
            assert_eq!(
                groups[0]
                    .targets
                    .iter()
                    .map(|t| (t.ordinal, t.weight))
                    .collect::<Vec<_>>(),
                vec![(0, 400), (1, 700)]
            );
            assert_ne!(
                groups[0].targets[0].source_key,
                groups[0].targets[1].source_key
            );
        });
    }

    /// 新しい候補配列も容量を確保前に予約し、元の上限エラーを返す。
    #[test]
    fn overprint_group_storage_preserves_original_budget_failure() {
        with_page(|page, tags| {
            let budget = DecodeBudget::new(&ParseLimits {
                max_total_metadata_workspace_bytes: 0,
                ..Default::default()
            });
            assert!(matches!(
                collect(&budget, page.objects(), tags),
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    limit: 0,
                    ..
                })
            ));
        });
    }
}
