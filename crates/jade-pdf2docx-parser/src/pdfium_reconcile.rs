//! pure/native文字対応の索引・文字配列・結果コピーを累積予算へ予約する。
use crate::{ParsedTextElement, QualityReport, decode::DecodeBudget, metadata_map::Map};

/// 一つのページでnative文字とpure文字数・対応位置を共有する。
struct Page {
    /// 非空白native文字の必要件数。
    native_count: usize,
    /// pure要素に含まれる非空白文字数。
    pure_count: usize,
    /// 空白だけのpureページも存在を区別する。
    pure_seen: bool,
    /// 件数検証後に確保するnative文字配列。
    characters: Vec<char>,
    /// 次のpure要素へ対応させる非空白文字位置。
    offset: usize,
    /// Formのあるページでは異なる抽出順への置換を拒否する。
    form_page: bool,
}

/// 件数一致時だけpureの幾何/style/空白を保持し、native文字を順に割り当てる。
/// 不一致はNone、容量不足もNoneで返し、共有予算へ保存した失敗を呼び出し元が検査する。
pub(super) fn reconcile(
    budget: &DecodeBudget,
    pure_elements: &[ParsedTextElement],
    native_elements: &[ParsedTextElement],
    quality: &QualityReport,
) -> Option<Vec<ParsedTextElement>> {
    if budget.is_stopped() || pure_elements.is_empty() {
        return None;
    }
    let mut index = Map::<usize, usize>::default();
    let mut pages: Vec<Page> = Vec::new();
    for element in native_elements {
        if budget.is_stopped() {
            return None;
        }
        let slot = if let Some(&slot) = index.get(&element.page) {
            slot
        } else {
            let slot = pages.len();
            index.insert(budget, element.page, slot)?;
            crate::metadata_workspace::try_push(budget, &mut pages, Page {
                native_count: 0,
                pure_count: 0,
                pure_seen: false,
                characters: Vec::new(),
                offset: 0,
                form_page: quality.issues.iter().any(|issue| matches!(issue,
                    crate::QualityIssue::UnsupportedXObject { page, reason: crate::UnsupportedXObjectReason::Form, .. }
                        if *page == element.page)),
            })?;
            slot
        };
        pages[slot].native_count = pages[slot]
            .native_count
            .checked_add(nonspace_count(&element.text))?;
    }
    for element in pure_elements {
        if budget.is_stopped() {
            return None;
        }
        let slot = *index.get(&element.page)?;
        pages[slot].pure_seen = true;
        pages[slot].pure_count = pages[slot]
            .pure_count
            .checked_add(nonspace_count(&element.text))?;
    }
    if pages
        .iter()
        .any(|page| !page.pure_seen || page.native_count != page.pure_count)
    {
        return None;
    }
    for page in &mut pages {
        page.characters = budget.metadata_vector(page.native_count)?;
    }
    for element in native_elements {
        if budget.is_stopped() {
            return None;
        }
        let slot = *index.get(&element.page)?;
        pages[slot]
            .characters
            .extend(element.text.chars().filter(|ch| !ch.is_whitespace()));
    }
    let mut output = budget.metadata_vector(pure_elements.len())?;
    for element in pure_elements {
        let slot = *index.get(&element.page)?;
        let page = &mut pages[slot];
        let remaining = &page.characters[page.offset..];
        let text = replacement(budget, &element.text, remaining, page.form_page)?;
        let font_name = crate::text_result_storage::copy(budget, element.font_name.as_deref())?;
        let fill_color = crate::text_result_storage::copy(budget, element.fill_color.as_deref())?;
        output.push(ParsedTextElement {
            page: element.page,
            bounds: element.bounds,
            font_size: element.font_size,
            font_name,
            fill_color,
            text,
        });
        page.offset += nonspace_count(&element.text);
    }
    Some(output)
}

/// 空白の配置を保持するため、対応対象の文字数だけを数える。
fn nonspace_count(text: &str) -> usize {
    text.chars().filter(|ch| !ch.is_whitespace()).count()
}

/// Unicode置換後の正確なUTF-8容量を先に予約し、動的拡張を避ける。
fn replacement(
    budget: &DecodeBudget,
    pure: &str,
    native: &[char],
    form_page: bool,
) -> Option<String> {
    let mut characters = native.iter().copied();
    let mut capacity = 0usize;
    for ch in pure.chars() {
        let mapped = if ch.is_whitespace() {
            ch
        } else {
            characters.next()?
        };
        if form_page && ch != mapped {
            return None;
        }
        capacity = capacity.checked_add(mapped.len_utf8())?;
    }
    let mut output = budget.metadata_vector(capacity)?;
    let mut characters = native.iter().copied();
    for ch in pure.chars() {
        let mapped = if ch.is_whitespace() {
            ch
        } else {
            characters.next()?
        };
        let mut utf8 = [0; 4];
        output.extend_from_slice(mapped.encode_utf8(&mut utf8).as_bytes());
    }
    String::from_utf8(output).ok()
}
