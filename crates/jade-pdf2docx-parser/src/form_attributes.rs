//! Form の幾何属性を描画前に検証し、不正属性による本文の読み飛ばしを防ぐ。
use crate::{
    ParseError, ParseLimits,
    decode::DecodeBudget,
    metadata,
    page_attributes::{Objects, is_null, number_value, resolve_value},
    syntax::{Lexer, Token},
};
use std::path::Path;

/// 必須 BBox と省略可能な Matrix/FormType を、未描画 Form も含めて検査する。
pub(crate) fn validate(
    path: &Path,
    objects: &Objects<'_>,
    dictionary: &[u8],
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<(), ParseError> {
    let bbox =
        metadata::value(dictionary, b"BBox").ok_or_else(|| invalid(path, "missing Form BBox"))?;
    let bounds = numbers::<4>(path, objects, bbox, limits, budget)?;
    // PDF の矩形は対角の指定順に依存せず、空領域も許す。
    if !(bounds[2].max(bounds[0]) - bounds[2].min(bounds[0])).is_finite()
        || !(bounds[3].max(bounds[1]) - bounds[3].min(bounds[1])).is_finite()
    {
        return Err(invalid(path, "Form BBox has a non-finite extent"));
    }
    if let Some(value) = metadata::value(dictionary, b"Matrix") {
        let (value, _) = resolve_value(path, objects, value, limits, budget)?;
        if !is_null(value) {
            numbers::<6>(path, objects, value, limits, budget)?;
        }
    }
    if let Some(value) = metadata::value(dictionary, b"FormType") {
        let (value, _) = resolve_value(path, objects, value, limits, budget)?;
        if !is_null(value) && !is_form_type_one(value) {
            return Err(invalid(path, "FormType is not integer 1"));
        }
    }
    Ok(())
}

/// 配列本体と各要素の参照を予算内で解決し、指定個数の有限 PDF 数値を要求する。
fn numbers<const N: usize>(
    path: &Path,
    objects: &Objects<'_>,
    value: &[u8],
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<[f32; N], ParseError> {
    let (value, _) = resolve_value(path, objects, value, limits, budget)?;
    let values = crate::metadata_workspace::array_values(budget, value, N)?
        .filter(|values| values.len() == N)
        .ok_or_else(|| invalid(path, "Form geometry is not an array of the required length"))?;
    let mut result = [0.0; N];
    for (target, value) in result.iter_mut().zip(values) {
        let (value, _) = resolve_value(path, objects, value, limits, budget)?;
        *target = number_value(value)
            .ok_or_else(|| invalid(path, "Form geometry contains an invalid number"))?;
    }
    Ok(result)
}

/// 実数や偽 token を許さず、既定 FormType の整数1だけを読み取る。
fn is_form_type_one(value: &[u8]) -> bool {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    let Some(Token::Word(word)) = lexer.next() else {
        return false;
    };
    lexer.next().is_none()
        && std::str::from_utf8(word)
            .ok()
            .and_then(|word| word.parse::<i64>().ok())
            == Some(1)
}

/// 属性不正を呼び出し元が判別できる共通の構造エラーにする。
fn invalid(path: &Path, reason: &'static str) -> ParseError {
    ParseError::InvalidPdfStructure {
        path: path.to_path_buf(),
        reason,
    }
}
