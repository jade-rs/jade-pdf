//! 他objectをSDK内だけで不可視化し、選択元objectの一文字だけを独立に確認する。
use super::{Tagged, Target};
use crate::{ParseError, ParsedTextElement, decode::DecodeBudget};
use pdfium_render::prelude::*;

/// 元入力を変更せず、sourceの識別子・weight・文字を確認した場合だけ一文字を返す。
pub(crate) fn extract(
    _budget: &DecodeBudget,
    _sdk: &Pdfium,
    _bytes: &[u8],
    _page: usize,
    _target: &Target,
    _tags: &Tagged,
    _path: &std::path::Path,
) -> Result<Option<ParsedTextElement>, ParseError> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParseLimits;

    /// 同じfamily・位置のobjectを別々に読み、確認済みsource weightとAAを保持する。
    #[test]
    fn isolated_source_objects_recover_both_owned_unicode_and_weights() {
        super::super::tests::with_page(|sdk, page, tags| {
            let groups =
                super::super::collect(&DecodeBudget::default(), page.objects(), tags).unwrap();
            let mut elements = Vec::new();
            for target in &groups[0].targets {
                elements.push(
                    extract(
                        &DecodeBudget::default(),
                        sdk,
                        &tags.bytes,
                        0,
                        target,
                        tags,
                        std::path::Path::new("overprint.pdf"),
                    )
                    .unwrap()
                    .unwrap(),
                );
            }
            assert_eq!(
                elements.iter().map(|e| e.text.as_str()).collect::<String>(),
                "AA"
            );
            assert_eq!(
                elements.iter().map(|e| e.font_weight).collect::<Vec<_>>(),
                vec![Some(400), Some(700)]
            );
            assert!(
                elements
                    .iter()
                    .all(|e| e.font_name.as_deref() == Some("OwnedSubset"))
            );
            assert_eq!(elements[0].bounds, elements[1].bounds);
        });
    }

    /// SDKの再読込みも入力長を処理量の代理値として予約し、上限超過のまま進めない。
    #[test]
    fn isolated_source_reload_obeys_original_workspace_failure() {
        super::super::tests::with_page(|sdk, page, tags| {
            let groups =
                super::super::collect(&DecodeBudget::default(), page.objects(), tags).unwrap();
            let budget = DecodeBudget::new(&ParseLimits {
                max_total_metadata_workspace_bytes: 0,
                ..Default::default()
            });
            assert!(matches!(
                extract(
                    &budget,
                    sdk,
                    &tags.bytes,
                    0,
                    &groups[0].targets[0],
                    tags,
                    std::path::Path::new("overprint.pdf")
                ),
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    limit: 0,
                    ..
                })
            ));
        });
    }
}
