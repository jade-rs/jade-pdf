//! 不可視文字を含むnativeページを文字index順で抽出し、幾何検索の重複を避ける。
use crate::{ParseError, ParsedTextElement, decode::DecodeBudget};
use pdfium_render::prelude::*;

/// Pdfiumの不可視描画・clipだけの描画・完全透明なpaintを判定する。
fn hidden(character: &PdfPageTextChar<'_>, tags: &crate::pdfium_font_tags::Tagged) -> bool {
    hidden_paint(
        character.render_mode().ok(),
        || tags.is_type3(&character.font_name()),
        || character.fill_color().is_ok_and(|color| color.alpha() == 0),
        || {
            character
                .stroke_color()
                .is_ok_and(|color| color.alpha() == 0)
        },
    )
}

/// nativeの文字・text objectで同じ描画モードとalpha判定を使う。
fn hidden_paint(
    mode: Option<PdfPageTextRenderMode>,
    type3: impl FnOnce() -> bool,
    fill_hidden: impl FnOnce() -> bool,
    stroke_hidden: impl FnOnce() -> bool,
) -> bool {
    if matches!(mode, Some(PdfPageTextRenderMode::Invisible)) {
        return true;
    }
    let hidden = match mode {
        Some(PdfPageTextRenderMode::InvisibleClipping) => true,
        Some(
            PdfPageTextRenderMode::FilledUnstroked | PdfPageTextRenderMode::FilledUnstrokedClipping,
        ) => fill_hidden(),
        Some(
            PdfPageTextRenderMode::StrokedUnfilled | PdfPageTextRenderMode::StrokedUnfilledClipping,
        ) => stroke_hidden(),
        Some(
            PdfPageTextRenderMode::FilledThenStroked
            | PdfPageTextRenderMode::FilledThenStrokedClipping,
        ) => fill_hidden() && stroke_hidden(),
        _ => false,
    };
    // Type3のglyph内部での状態変更はunsupported診断へ委ねる。通常の可視文字ではFont名を確保しない。
    hidden && !type3()
}

/// object単位の不可視paintを判定し、clip検査でも同じ例外を適用する。
pub(crate) fn hidden_object(
    text: &PdfPageTextObject<'_>,
    tags: &crate::pdfium_font_tags::Tagged,
) -> bool {
    hidden_paint(
        Some(text.render_mode()),
        || tags.is_type3(&text.font().name()),
        || text.fill_color().is_ok_and(|color| color.alpha() == 0),
        || text.stroke_color().is_ok_and(|color| color.alpha() == 0),
    )
}

/// 重なる不可視文字で可視glyphが重複排除される前に、native専用objectの本文を空白へ置換する。
/// Formも予算内で巡回し、元PDFとpure Rust IRには変更を加えない。
pub(crate) fn suppress_hidden<'a>(
    budget: &DecodeBudget,
    path: &std::path::Path,
    objects: &'a impl PdfPageObjectsCommon<'a>,
    tags: &crate::pdfium_font_tags::Tagged,
    depth: usize,
) -> Result<(), ParseError> {
    suppress_clipped(
        budget,
        path,
        objects,
        tags,
        depth,
        None,
        [1., 0., 0., 1., 0., 0.],
    )
}

/// 親FormのclipとCTMを引き継いでnative専用の非表示objectを取り除く。
fn suppress_clipped<'a>(
    budget: &DecodeBudget,
    path: &std::path::Path,
    objects: &'a impl PdfPageObjectsCommon<'a>,
    tags: &crate::pdfium_font_tags::Tagged,
    depth: usize,
    inherited: Option<&crate::form_clip::Clip>,
    matrix: [f32; 6],
) -> Result<(), ParseError> {
    let _ = budget.check_form_depth(depth);
    budget.check()?;
    for mut object in objects.iter() {
        budget.check()?;
        if object.as_text_object().is_none() && object.as_x_object_form_object().is_none() {
            continue;
        }
        let clip = crate::pdfium_clip::object_clip(budget, &object, inherited, matrix);
        budget.check()?;
        let region = clip.as_ref().or(inherited);
        let outside = crate::pdfium_clip::outside(budget, &object, region, matrix);
        budget.check()?;
        if let Some(text) = object.as_text_object_mut() {
            if outside || hidden_object(text, tags) {
                text.set_text("").map_err(|error| ParseError::Pdfium {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                })?;
            }
        } else if let Some(form) = object.as_x_object_form_object()
            && let Some(matrix) = crate::pdfium_clip::form_matrix(&object, matrix)
        {
            suppress_clipped(budget, path, form, tags, depth + 1, region, matrix)?;
        }
    }
    Ok(())
}

/// 描画モードによる除外が必要なページだけを追加抽出へ渡す。
pub(crate) fn has_hidden(
    characters: &PdfPageTextChars<'_>,
    tags: &crate::pdfium_font_tags::Tagged,
) -> bool {
    characters.iter().any(|character| hidden(&character, tags))
}

/// 座標が重なる文字もindexで識別し、paintする文字だけを予算付きで保存する。
pub(crate) fn extract(
    budget: &DecodeBudget,
    path: &std::path::Path,
    page: usize,
    characters: &PdfPageTextChars<'_>,
    tags: &crate::pdfium_font_tags::Tagged,
    excluded: &[bool],
) -> Result<(String, Vec<ParsedTextElement>), ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let mut preview = String::new();
    let mut elements: Vec<ParsedTextElement> = Vec::new();
    let mut characters = characters.iter().peekable();
    while let Some(character) = characters.next() {
        if excluded.get(character.index()).copied().unwrap_or(false) || hidden(&character, tags) {
            continue;
        }
        let value = character.unicode_value();
        let ch = if let Some(ch) = char::from_u32(value) {
            ch
        } else if (0xd800..=0xdbff).contains(&value)
            && characters.peek().is_some_and(|next| {
                !excluded.get(next.index()).copied().unwrap_or(false)
                    && !hidden(next, tags)
                    && (0xdc00..=0xdfff).contains(&next.unicode_value())
            })
        {
            let low = characters.next().unwrap().unicode_value();
            char::from_u32(0x10000 + ((value - 0xd800) << 10) + low - 0xdc00).unwrap()
        } else {
            continue;
        };
        let mut utf8 = [0; 4];
        let text = ch.encode_utf8(&mut utf8);
        crate::text_result_storage::extend(budget, &mut preview, text);
        budget.check()?;
        if ch.is_whitespace() {
            if !matches!(ch, '\r' | '\n')
                && let Some(previous) = elements.last_mut()
            {
                crate::text_result_storage::extend(budget, &mut previous.text, text);
                budget.check()?;
            }
            continue;
        }
        let rect = character
            .loose_bounds()
            .map_err(|error| ParseError::Pdfium {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?;
        let name = character.font_name();
        let color = character.fill_color().ok().map(|color| color.to_hex());
        let Some(font_name) = crate::text_result_storage::copy(budget, tags.original_name(&name))
        else {
            budget.check()?;
            break;
        };
        let Some(fill_color) = crate::text_result_storage::copy(budget, color.as_deref()) else {
            budget.check()?;
            break;
        };
        let Some(Some(text)) = crate::text_result_storage::copy(budget, Some(text)) else {
            budget.check()?;
            break;
        };
        crate::metadata_workspace::push(
            budget,
            &mut elements,
            ParsedTextElement {
                page,
                bounds: crate::ParsedBounds {
                    x0: rect.left().value,
                    y0: rect.bottom().value,
                    x1: rect.right().value,
                    y1: rect.top().value,
                },
                font_size: character.scaled_font_size().value,
                font_name,
                fill_color,
                text,
            },
        )?;
    }
    Ok((preview, elements))
}
