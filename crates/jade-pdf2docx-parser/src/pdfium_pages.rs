//! nativeページ情報の保存容量とpure/native寸法合成を扱う。
use crate::{ParseError, ParsedPage, decode::DecodeBudget};

/// 件数が確定したnativeページの全slotを予約し、fallibleに保存配列を作る。
pub(super) fn reserve(budget: &DecodeBudget, count: usize) -> Result<Vec<ParsedPage>, ParseError> {
    crate::metadata_workspace::vector(budget, count)
}

/// pureの表示矩形・継承回転を優先し、欠損ページをnative情報で補う。
/// 保存済み配列を直接更新し、合成用の配列を追加確保しない。
pub(super) fn merge(
    budget: &DecodeBudget,
    pure: &[ParsedPage],
    mut native: Vec<ParsedPage>,
) -> Result<Vec<ParsedPage>, ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let sorted_unique = pure
        .windows(2)
        .all(|pair| pair[0].page_number < pair[1].page_number);
    for page in &mut native {
        crate::metadata_workspace::ensure_active(budget)?;
        let replacement = if sorted_unique {
            pure.get(page.page_number)
                .filter(|pure| pure.page_number == page.page_number)
                .or_else(|| {
                    pure.binary_search_by_key(&page.page_number, |pure| pure.page_number)
                        .ok()
                        .map(|index| &pure[index])
                })
        } else {
            pure.iter()
                .find(|pure| pure.page_number == page.page_number)
        };
        if let Some(replacement) = replacement {
            *page = replacement.clone();
        } else if page.rotation.rem_euclid(180) == 90 {
            std::mem::swap(&mut page.width, &mut page.height);
        }
    }
    Ok(native)
}

#[cfg(test)]
#[path = "pdfium_pages_budget_tests.rs"]
mod tests;
