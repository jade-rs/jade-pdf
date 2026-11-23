//! Pdfium が Unicode を引けず文字コードをそのまま返した字を、本物の字と分けて私用面へ移す。
//!
//! ToUnicode が無く、CIDSystemInfo の Ordering が Adobe の標準の文字集合でもない CID フォント
//! （DynaFont の拡張字形、罫線用フォント MFHSLine など）では、Pdfium は CID を符号位置とみなして
//! 無関係な漢字を返す（CID 25344 が U+6300「挀」、CID 23075 が U+5A23「娣」）。同じ subset 名の
//! フォントが ToUnicode 付きと無しで並ぶこともあるので、フォント名ではなく Pdfium の文字ごとの
//! `FPDFText_HasUnicodeMapError` で見分け、その字を U+F0000 + CID へ移す。DOCX 側で
//! 確かめられた字だけを戻す。
use crate::{ParseError, RAW_CID_BASE, decode::DecodeBudget, model::ParsedTextElement};
use pdfium_render::prelude::PdfiumLibraryBindings;
use std::os::raw::c_double;

/// Unicode の対応が無いと Pdfium が判定した 1 字。
#[derive(Debug, Clone, Copy, PartialEq)]
struct RawChar {
    /// 0 始まりのページ番号。
    page: usize,
    /// Pdfium が返した字（文字コードそのもの）。
    ch: char,
    /// 字の矩形の中心（Pdfium のユーザー空間）。
    center: (f32, f32),
}

/// Unicode の対応が無い字を集め、その位置にある要素の字を私用面へ移す。
pub(crate) fn tag(
    budget: &DecodeBudget,
    bindings: &dyn PdfiumLibraryBindings,
    bytes: &[u8],
    text_elements: &mut [ParsedTextElement],
) -> Result<(), ParseError> {
    let raw = collect(budget, bindings, bytes)?;
    tag_elements(&raw, text_elements);
    Ok(())
}

/// 文書を別に開き、各ページの Unicode の対応が無い字を集める。開けないときは何も集めない。
fn collect(
    budget: &DecodeBudget,
    bindings: &dyn PdfiumLibraryBindings,
    bytes: &[u8],
) -> Result<Vec<RawChar>, ParseError> {
    let document = bindings.FPDF_LoadMemDocument64(bytes, None);
    if document.is_null() {
        return Ok(Vec::new());
    }
    let mut raw = Vec::new();
    let pages = bindings.FPDF_GetPageCount(document).max(0);
    let mut result = Ok(());
    for page_index in 0..pages {
        if let Err(error) = budget.check() {
            result = Err(error);
            break;
        }
        let page = bindings.FPDF_LoadPage(document, page_index);
        if page.is_null() {
            continue;
        }
        let text_page = bindings.FPDFText_LoadPage(page);
        if !text_page.is_null() {
            let count = bindings.FPDFText_CountChars(text_page).max(0);
            for index in 0..count {
                if bindings.FPDFText_HasUnicodeMapError(text_page, index) != 1 {
                    continue;
                }
                let Some(ch) = char::from_u32(bindings.FPDFText_GetUnicode(text_page, index))
                else {
                    continue;
                };
                let (mut left, mut right, mut bottom, mut top) = (
                    0.0 as c_double,
                    0.0 as c_double,
                    0.0 as c_double,
                    0.0 as c_double,
                );
                let found = bindings.FPDFText_GetCharBox(
                    text_page,
                    index,
                    &mut left,
                    &mut right,
                    &mut bottom,
                    &mut top,
                );
                if bindings.is_true(found) {
                    raw.push(RawChar {
                        page: page_index as usize,
                        ch,
                        center: (((left + right) / 2.0) as f32, ((bottom + top) / 2.0) as f32),
                    });
                }
            }
            bindings.FPDFText_ClosePage(text_page);
        }
        bindings.FPDF_ClosePage(page);
    }
    bindings.FPDF_CloseDocument(document);
    result.map(|()| raw)
}

/// 私用面へ移す字か。
///
/// 1 byte の文字コードの単純フォント（Type1・TrueType・Type3）は、Unicode の対応が無くても
/// 文字コードがおおむね ASCII・Latin-1 の字のままなので移さない（U+00FF 以下）。空白・私用領域の
/// 字（対応表で戻す）・BMP の外の字も移さない。
fn is_taggable(ch: char) -> bool {
    let code = ch as u32;
    code > 0xFF && !ch.is_whitespace() && !('\u{E000}'..='\u{F8FF}').contains(&ch) && code <= 0xFFFD
}

/// 要素の矩形の中に、同じ字で Unicode の対応が無い字があれば、その字を私用面へ移す。
fn tag_elements(raw: &[RawChar], text_elements: &mut [ParsedTextElement]) {
    const SLACK_PT: f32 = 1.0;
    if raw.is_empty() {
        return;
    }
    for text in text_elements.iter_mut() {
        let (x0, x1) = (
            text.bounds.x0.min(text.bounds.x1) - SLACK_PT,
            text.bounds.x0.max(text.bounds.x1) + SLACK_PT,
        );
        let (y0, y1) = (
            text.bounds.y0.min(text.bounds.y1) - SLACK_PT,
            text.bounds.y0.max(text.bounds.y1) + SLACK_PT,
        );
        let inside: Vec<char> = raw
            .iter()
            .filter(|raw| {
                raw.page == text.page
                    && (x0..=x1).contains(&raw.center.0)
                    && (y0..=y1).contains(&raw.center.1)
            })
            .map(|raw| raw.ch)
            .collect();
        if inside.is_empty() {
            continue;
        }
        text.text = text
            .text
            .chars()
            .map(|ch| {
                if is_taggable(ch) && inside.contains(&ch) {
                    char::from_u32(RAW_CID_BASE + ch as u32).unwrap_or(ch)
                } else {
                    ch
                }
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ParsedBounds;

    /// テスト用の要素。
    fn element(page: usize, x0: f32, text: &str) -> ParsedTextElement {
        ParsedTextElement {
            baseline_y: None,
            page,
            bounds: ParsedBounds {
                x0,
                y0: 0.0,
                x1: x0 + 30.0,
                y1: 10.0,
            },
            font_size: 10.0,
            font_name: None,
            fill_color: None,
            text: text.to_string(),
        }
    }

    /// 対応の無い字の位置にある要素の、その字だけを私用面へ移す。
    #[test]
    fn tags_only_unmapped_characters_at_their_position() {
        let raw = [
            RawChar {
                page: 0,
                ch: '挀',
                center: (5.0, 5.0),
            },
            RawChar {
                page: 0,
                ch: 'A',
                center: (5.0, 5.0),
            },
        ];
        let mut texts = vec![
            element(0, 0.0, "挀 持\u{EA74}A"),
            element(0, 100.0, "挀"),
            element(1, 0.0, "挀"),
        ];
        tag_elements(&raw, &mut texts);
        assert_eq!(texts[0].text, "\u{F6300} 持\u{EA74}A");
        assert_eq!(texts[1].text, "挀");
        assert_eq!(texts[2].text, "挀");
    }
}
