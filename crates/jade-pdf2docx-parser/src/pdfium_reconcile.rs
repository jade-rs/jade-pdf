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
    /// native側の要素数。
    native_elements: usize,
    /// ページ単位の文字対応が安全に成立したか。
    compatible: bool,
    /// 対応しないページのnative要素位置。
    fallback: Vec<usize>,
    /// fallbackページの二重出力を防ぐ。
    emitted: bool,
    /// Formのあるページでは異なる抽出順への置換を拒否する。
    form_page: bool,
}

/// 対応するページだけpureの幾何/style/空白を保持し、他はnative要素を使う。
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
                native_elements: 0,
                compatible: false,
                fallback: Vec::new(),
                emitted: false,
                form_page: quality.issues.iter().any(|issue| matches!(issue,
                    crate::QualityIssue::UnsupportedXObject { page, reason: crate::UnsupportedXObjectReason::Form, .. }
                        if *page == element.page)),
            })?;
            slot
        };
        pages[slot].native_elements = pages[slot].native_elements.checked_add(1)?;
        pages[slot].native_count = pages[slot]
            .native_count
            .checked_add(nonspace_count(&element.text))?;
    }
    for element in pure_elements {
        if budget.is_stopped() {
            return None;
        }
        let Some(&slot) = index.get(&element.page) else {
            continue;
        };
        pages[slot].pure_seen = true;
        pages[slot].pure_count = pages[slot]
            .pure_count
            .checked_add(nonspace_count(&element.text))?;
    }
    for page in &mut pages {
        page.compatible = page.pure_seen && page.native_count == page.pure_count;
        if page.compatible {
            page.characters = budget.metadata_vector(page.native_count)?;
        }
    }
    for element in native_elements {
        if budget.is_stopped() {
            return None;
        }
        let slot = *index.get(&element.page)?;
        if pages[slot].compatible {
            pages[slot]
                .characters
                .extend(element.text.chars().filter(|ch| !ch.is_whitespace()));
        }
    }
    // Formの順序と生成cmapのUnicodeを、結果を確保する前にページ単位で検証する。
    for element in pure_elements {
        let Some(&slot) = index.get(&element.page) else {
            continue;
        };
        let page = &mut pages[slot];
        if !page.compatible {
            continue;
        }
        let length = nonspace_count(&element.text);
        let end = page.offset.checked_add(length)?;
        let mapped = page.characters.get(page.offset..end)?;
        if (page.form_page
            || element
                .font_name
                .as_deref()
                .is_some_and(|name| name.starts_with("JadeT1-")))
            && !element
                .text
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .eq(mapped.iter().copied())
        {
            page.compatible = false;
        }
        page.offset = end;
    }
    if !pages.iter().any(|page| page.compatible) {
        return None;
    }
    let mut capacity = pure_elements
        .iter()
        .filter(|element| {
            index
                .get(&element.page)
                .is_some_and(|&slot| pages[slot].compatible)
        })
        .count();
    for page in &mut pages {
        page.offset = 0;
        if !page.compatible {
            page.fallback = budget.metadata_vector(page.native_elements)?;
            capacity = capacity.checked_add(page.native_elements)?;
        }
    }
    for (position, element) in native_elements.iter().enumerate() {
        let slot = *index.get(&element.page)?;
        if !pages[slot].compatible {
            pages[slot].fallback.push(position);
        }
    }
    let mut output = budget.metadata_vector(capacity)?;
    for element in pure_elements {
        let Some(&slot) = index.get(&element.page) else {
            continue;
        };
        let page = &mut pages[slot];
        if !page.compatible {
            emit_fallback(budget, page, native_elements, &mut output)?;
            continue;
        }
        let remaining = &page.characters[page.offset..];
        let text = replacement(budget, &element.text, remaining, false)?;
        let mut copied = copy_element(budget, element, false)?;
        copied.text = text;
        output.push(copied);
        page.offset += nonspace_count(&element.text);
    }
    // pure側にないnativeページも落とさず、nativeの出現順で追加する。
    for element in native_elements {
        let slot = *index.get(&element.page)?;
        if !pages[slot].compatible {
            emit_fallback(budget, &mut pages[slot], native_elements, &mut output)?;
        }
    }
    Some(output)
}

/// fallback要素の本文と属性を累積予算で複製し、一ページを一度だけ出力する。
fn emit_fallback(
    budget: &DecodeBudget,
    page: &mut Page,
    native: &[ParsedTextElement],
    output: &mut Vec<ParsedTextElement>,
) -> Option<()> {
    if page.emitted {
        return Some(());
    }
    for &position in &page.fallback {
        output.push(copy_element(budget, native.get(position)?, true)?);
    }
    page.emitted = true;
    Some(())
}

/// 本文の要否を区別し、複製する文字列を全て予約する。
fn copy_element(
    budget: &DecodeBudget,
    element: &ParsedTextElement,
    text: bool,
) -> Option<ParsedTextElement> {
    Some(ParsedTextElement {
        font_weight: None,
        baseline_y: element.baseline_y,
        page: element.page,
        bounds: element.bounds,
        font_size: element.font_size,
        font_name: crate::text_result_storage::copy(budget, element.font_name.as_deref())?,
        fill_color: crate::text_result_storage::copy(budget, element.fill_color.as_deref())?,
        text: if text {
            crate::text_result_storage::copy(budget, Some(&element.text))??
        } else {
            String::new()
        },
    })
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
