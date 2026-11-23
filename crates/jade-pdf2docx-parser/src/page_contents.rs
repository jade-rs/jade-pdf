//! 正式PageのContentsを検証し、間接配列の全streamを描画順で保持する。
use crate::{
    ParseError, ParseLimits,
    decode::DecodeBudget,
    metadata,
    metadata_workspace::{ensure_active, vector},
    page_attributes::{Objects, is_null, resolve_value},
    syntax::{Lexer, Token},
};
use std::path::Path;

/// aliasと世代を検証し、同じstreamの繰り返しも省略せず実行順へ展開する。
pub(crate) fn resolve(
    path: &Path,
    objects: &Objects<'_>,
    value: Option<&[u8]>,
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<Vec<u32>, ParseError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let (value, number) = resolve_value(path, objects, value, limits, budget)?;
    if is_null(value) {
        return Ok(Vec::new());
    }
    if let Some(entries) = metadata::array_entries(value, usize::MAX) {
        let mut count = 0usize;
        for entry in entries {
            ensure_active(budget)?;
            entry.ok_or_else(|| invalid(path, "invalid Contents array"))?;
            count += 1;
        }
        let mut streams = vector(budget, count)?;
        for entry in metadata::array_entries(value, usize::MAX).unwrap() {
            let entry = entry.ok_or_else(|| invalid(path, "invalid Contents array"))?;
            let (value, number) = resolve_value(path, objects, entry, limits, budget)?;
            streams.push(stream_number(path, value, number)?);
        }
        return Ok(streams);
    }
    let number = stream_number(path, value, number)?;
    let mut streams = vector(budget, 1)?;
    streams.push(number);
    Ok(streams)
}

/// Contentsの各要素は間接streamとし、配列・辞書・文字列への誤参照を拒否する。
fn stream_number(path: &Path, value: &[u8], number: Option<u32>) -> Result<u32, ParseError> {
    let number =
        number.ok_or_else(|| invalid(path, "Contents must reference an indirect stream"))?;
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    if !matches!(lexer.next(), Some(Token::Open(true))) {
        return Err(invalid(path, "Contents target is not a stream"));
    }
    let dictionary = metadata::dictionary_iter(value)
        .next()
        .ok_or_else(|| invalid(path, "invalid Contents stream dictionary"))?;
    let end = dictionary.as_ptr() as usize - value.as_ptr() as usize + dictionary.len();
    if !matches!(
        (Lexer {
            bytes: &value[end..],
            cursor: 0
        })
        .next(),
        Some(Token::Word(b"stream"))
    ) {
        return Err(invalid(path, "Contents target is not a stream"));
    }
    Ok(number)
}

/// 不正な本文参照を、入力パスを持つ構造エラーとして返す。
fn invalid(path: &Path, reason: &'static str) -> ParseError {
    ParseError::InvalidPdfStructure {
        path: path.into(),
        reason,
    }
}
