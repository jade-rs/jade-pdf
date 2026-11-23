//! font resourceの有効世代の値と、f64の数値を厳密に読む。
use crate::{
    decode::DecodeBudget,
    metadata,
    resource_resolver::ResourceResolver,
    syntax::{Lexer, Token},
};

/// 重複keyと破損辞書を拒否し、null・未指定と参照の失敗を区別する。
pub(in crate::type1_outline) fn field<'bytes>(
    budget: &DecodeBudget,
    dictionary: &'bytes [u8],
    key: &[u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<Option<&'bytes [u8]>> {
    if budget.is_stopped() || !metadata::dictionary_entries_complete(dictionary) {
        return None;
    }
    let mut raw = None;
    for (name, value) in metadata::entries(dictionary)? {
        if budget.is_stopped() {
            return None;
        }
        if crate::syntax::name_is(name, key) {
            if raw.is_some() {
                return None;
            }
            raw = Some(value);
        }
    }
    match raw {
        None => Some(None),
        Some(raw) => {
            let (value, _) = resolver.resolve(raw)?;
            Some((value.trim_ascii() != b"null").then_some(value))
        }
    }
}

/// 単一の有限なPDF数値を、f32へ縮めずに読む。
pub(in crate::type1_outline) fn number(value: &[u8]) -> Option<f64> {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    let Token::Word(raw) = lexer.next()? else {
        return None;
    };
    if raw.len() > 64 || lexer.next().is_some() {
        return None;
    }
    let value = std::str::from_utf8(raw).ok()?.parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

/// 単byte fontのcode範囲へ収まる整数値だけを読む。
pub(super) fn code(value: &[u8]) -> Option<usize> {
    let value = number(value)?;
    (value.fract() == 0. && (0. ..=255.).contains(&value)).then_some(value as usize)
}

/// 正常な単一のPDF nameを、escapeを展開せず借用する。
pub(in crate::type1_outline) fn name(value: &[u8]) -> Option<&[u8]> {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    let Token::Name(raw) = lexer.next()? else {
        return None;
    };
    (lexer.next().is_none() && crate::metadata_names::valid_resource_name(raw)).then_some(raw)
}
