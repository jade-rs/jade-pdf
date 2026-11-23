//! SDKがU+0002へ置換した印刷hyphenだけを、文字indexとIsHyphen判定で復元する。
use crate::{ParseError, decode::DecodeBudget};
use pdfium_render::prelude::*;
use std::{ops::Range, path::Path};

/// 同じSDK文字順の全ページpreviewを、再確保せず一byteずつ正規化する。
pub(crate) fn page(
    budget: &DecodeBudget,
    path: &Path,
    text: &mut String,
    characters: &PdfPageTextChars<'_>,
) -> Result<(), ParseError> {
    restore(budget, path, text, characters, 0..characters.len())
}

/// 矩形内の先頭・末尾indexが確定するsegmentだけ、対応するSDKのhyphenを復元する。
pub(crate) fn segment(
    budget: &DecodeBudget,
    path: &Path,
    text: &mut String,
    characters: &PdfPageTextChars<'_>,
    rect: PdfRect,
) -> Result<(), ParseError> {
    budget.check()?;
    if !text.contains('\u{2}') {
        return Ok(());
    }
    let tolerance_x = rect.width() / 2.;
    let tolerance_y = rect.height() / 2.;
    let center = rect.bottom() + tolerance_y;
    let start = characters.get_char_near_point(rect.left(), tolerance_x, center, tolerance_y);
    let end = characters.get_char_near_point(rect.right(), tolerance_x, center, tolerance_y);
    if let (Some(start), Some(end)) = (start, end)
        && start.index() <= end.index()
    {
        restore(
            budget,
            path,
            text,
            characters,
            start.index()..end.index() + 1,
        )?;
    }
    Ok(())
}

/// 不透明なU+0002を一律置換せず、実際のSDK hyphen flagがある文字だけを区別する。
pub(crate) fn is_printed(
    budget: &DecodeBudget,
    path: &Path,
    character: &PdfPageTextChar<'_>,
) -> Result<bool, ParseError> {
    budget.check()?;
    if character.unicode_value() != 2 {
        return Ok(false);
    }
    character
        .is_hyphen()
        .map_err(|error| native_error(path, error))
}

/// U+0002の件数と順序が一致する場合だけ、既存String内のASCII byteを等長置換する。
fn restore(
    budget: &DecodeBudget,
    path: &Path,
    text: &mut String,
    characters: &PdfPageTextChars<'_>,
    range: Range<usize>,
) -> Result<(), ParseError> {
    budget.check()?;
    let count = text.bytes().filter(|byte| *byte == 2).count();
    if count == 0 {
        return Ok(());
    }
    let mut native_count = 0;
    for index in range.clone() {
        budget.check()?;
        let character = characters
            .get(index)
            .map_err(|error| native_error(path, error))?;
        native_count += usize::from(character.unicode_value() == 2);
    }
    if count != native_count {
        return Ok(());
    }
    let mut offset = 0;
    for index in range {
        budget.check()?;
        let character = characters
            .get(index)
            .map_err(|error| native_error(path, error))?;
        if character.unicode_value() != 2 {
            continue;
        }
        let Some(relative) = text.as_bytes()[offset..].iter().position(|byte| *byte == 2) else {
            break;
        };
        let index = offset + relative;
        if is_printed(budget, path, &character)? {
            // ASCIIの一byte同士なのでUTF-8境界を壊さず、元の容量も増やさない。
            text.replace_range(index..index + 1, "-");
        }
        offset = index + 1;
    }
    Ok(())
}

/// SDK検査の失敗を入力path付きの既存公開エラーへ伝える。
fn native_error(path: &Path, error: impl std::fmt::Display) -> ParseError {
    ParseError::Pdfium {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}
