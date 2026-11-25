//! pure/native文字対応の索引・文字配列・結果コピーを累積予算へ予約する。
use crate::{ParsedTextElement, QualityReport, decode::DecodeBudget, metadata_map::Map};

#[cfg(test)]
#[path = "pdfium_reconcile_source_tests.rs"]
mod source_tests;

#[path = "pdfium_reconcile_mapping.rs"]
mod mapping;
#[path = "pdfium_reconcile_spaces.rs"]
mod spaces;

/// 一つのページでnative文字とpure文字数・対応位置を共有する。
struct Page {
    /// 非空白native文字の必要件数。
    native_count: usize,
    /// pure要素に含まれる非空白文字数。
    pure_count: usize,
    /// source selectorを除いた非空白scalar数。
    pure_base_count: usize,
    /// selectorが同じspan内のbase文字へ一対一で続いているか。
    valid_variations: bool,
    /// 生成fontがあるページでは、未検証のselector省略対応を採用しない。
    generated_font: bool,
    /// nativeが省いたselectorだけをsourceから保持するか。
    preserve_variations: bool,
    /// sourceのbase文字とnativeがページ全体で同順に一致したか。
    unicode_agrees: bool,
    /// 予約前に数えたSDKの語間件数。
    space_count: usize,
    /// SDKの語間を二回の走査で検出する状態。
    space_state: spaces::State,
    /// nativeの非空白位置で整列した語間境界。
    spaces: Vec<usize>,
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

/// 対応するページのsource属性・明示空白を保持し、一致を確認したSDK語間もspan内へ戻す。
/// selectorの省略はbase文字の完全一致で検証し、他の不一致ページはnative要素を使う。
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
                pure_base_count: 0,
                valid_variations: true,
                generated_font: false,
                preserve_variations: false,
                unicode_agrees: true,
                space_count: 0,
                space_state: Default::default(),
                spaces: Vec::new(),
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
        pages[slot].pure_base_count = pages[slot]
            .pure_base_count
            .checked_add(aligned_count(&element.text, true))?;
        pages[slot].valid_variations &= valid_variations(&element.text);
        pages[slot].generated_font |= generated_font(element);
    }
    for page in &mut pages {
        page.preserve_variations = page.pure_seen
            && page.pure_count > page.pure_base_count
            && page.native_count == page.pure_base_count
            && page.valid_variations
            && !page.generated_font;
        page.compatible =
            page.pure_seen && (page.native_count == page.pure_count || page.preserve_variations);
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
        let length = aligned_count(&element.text, page.preserve_variations);
        let end = page.offset.checked_add(length)?;
        let mapped = page.characters.get(page.offset..end)?;
        let agrees = element
            .text
            .chars()
            .filter(|ch| {
                !ch.is_whitespace() && !(page.preserve_variations && variation_selector(*ch))
            })
            .eq(mapped.iter().copied());
        page.unicode_agrees &= agrees;
        if (page.preserve_variations || page.form_page || generated_font(element)) && !agrees {
            page.compatible = false;
        }
        page.offset = end;
    }
    if !pages.iter().any(|page| page.compatible) {
        return None;
    }
    spaces::gather(budget, native_elements, &index, &mut pages)?;
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
        let text = mapping::Mapping {
            native: remaining,
            preserve_variations: page.preserve_variations,
            spaces: if generated_font(element) {
                &[]
            } else {
                &page.spaces
            },
            offset: page.offset,
        }
        .replacement(budget, &element.text)?;
        let mut copied = copy_element(budget, element, false)?;
        copied.text = text;
        output.push(copied);
        page.offset += aligned_count(&element.text, page.preserve_variations);
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
        font_weight: element.font_weight,
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

/// source fontが個別の符号化glyphを所有し、Unicodeの追加を許さないか返す。
fn generated_font(element: &ParsedTextElement) -> bool {
    element
        .font_name
        .as_deref()
        .is_some_and(|name| name.starts_with("JadeT1-"))
}

/// Unicodeの標準・補助variation selectorを識別する。
fn variation_selector(scalar: char) -> bool {
    matches!(scalar, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
}

/// nativeが省いたselectorを除き、一対一で対応する文字数を返す。
fn aligned_count(text: &str, omit_selectors: bool) -> usize {
    text.chars()
        .filter(|ch| !ch.is_whitespace() && !(omit_selectors && variation_selector(*ch)))
        .count()
}

/// leading・連続・空白直後のselectorを、同じglyphのvariationと推測しない。
fn valid_variations(text: &str) -> bool {
    let mut previous = None;
    for scalar in text.chars() {
        if variation_selector(scalar)
            && !previous.is_some_and(|ch: char| {
                !ch.is_whitespace() && !ch.is_control() && !variation_selector(ch)
            })
        {
            return false;
        }
        previous = Some(scalar);
    }
    true
}

#[cfg(test)]
mod source_weight_tests {
    use super::*;
    /// 確認済みsourceの複製では、新しい属性も文字・位置とともに失わない。
    #[test]
    fn budgeted_copy_preserves_source_font_weight() {
        let source = ParsedTextElement {
            font_weight: Some(700),
            baseline_y: Some(700.),
            page: 0,
            bounds: crate::ParsedBounds {
                x0: 72.,
                y0: 700.,
                x1: 100.,
                y1: 712.,
            },
            font_size: 12.,
            font_name: Some("Vendor-Regular".into()),
            fill_color: Some("123456".into()),
            text: "Weight".into(),
        };
        let budget = DecodeBudget::default();
        assert_eq!(copy_element(&budget, &source, true), Some(source.clone()));
        let mut without_text = source.clone();
        without_text.text.clear();
        assert_eq!(copy_element(&budget, &source, false), Some(without_text));
    }
}
