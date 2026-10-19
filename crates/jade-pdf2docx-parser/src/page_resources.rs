//! ページと Form に所属する Font と XObject の型・参照を検証する。
use crate::{
    ParseError, ParseLimits,
    decode::DecodeBudget,
    limits::check_limit,
    metadata,
    page_attributes::{Objects, dictionary_value, is_null, resolve_value},
    syntax::{Lexer, Token},
};
use std::path::Path;

/// resource map と各参照先を検査し、黙った部分欠落を防ぐ。
pub(crate) fn validate(
    path: &Path,
    objects: &Objects<'_>,
    resources: &[u8],
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<(), ParseError> {
    let mut pending = crate::metadata_workspace::Queue::default();
    pending.push(budget, (resources, 0usize))?;
    let mut forms = crate::metadata_workspace::Set::default();
    while let Some((resources, depth)) = pending.pop() {
        validate_dictionary(
            path,
            objects,
            (resources, depth),
            limits,
            budget,
            &mut forms,
            &mut pending,
        )?;
    }
    Ok(())
}

/// 一つの辞書を検査し、未検査 Form の局所辞書を幅優先の作業列へ追加する。
fn validate_dictionary<'a>(
    path: &Path,
    objects: &Objects<'a>,
    node: (&'a [u8], usize),
    limits: &ParseLimits,
    budget: &DecodeBudget,
    forms: &mut crate::metadata_workspace::Set<u32>,
    pending: &mut crate::metadata_workspace::Queue<(&'a [u8], usize)>,
) -> Result<(), ParseError> {
    let (resources, depth) = node;
    for category in [b"Font".as_slice(), b"XObject"] {
        let Some(value) = metadata::value(resources, category) else {
            continue;
        };
        let (value, _) = resolve_value(path, objects, value, limits, budget)?;
        if is_null(value) {
            continue;
        }
        let dictionary = dictionary_value(path, value)?;
        for (_, value) in metadata::entries(dictionary).into_iter().flatten() {
            let (value, number) = resolve_value(path, objects, value, limits, budget)?;
            if is_null(value) {
                continue;
            }
            let is_font = category == b"Font";
            let dictionary = if is_font {
                dictionary_value(path, value)?
            } else {
                if number.is_none() {
                    return Err(invalid(path, "XObject must be an indirect stream"));
                }
                stream_dictionary(path, value)?
            };
            let expected = if is_font {
                b"Font".as_slice()
            } else {
                b"XObject"
            };
            match metadata::value(dictionary, b"Type") {
                Some(value) => {
                    let (value, _) = resolve_value(path, objects, value, limits, budget)?;
                    if !(metadata::is_name(value, expected) || !is_font && is_null(value)) {
                        return Err(invalid(path, "incorrect resource Type"));
                    }
                }
                None if is_font => return Err(invalid(path, "missing Font Type")),
                None => {}
            }
            let subtype = metadata::value(dictionary, b"Subtype")
                .ok_or_else(|| invalid(path, "missing resource Subtype"))?;
            let (subtype, _) = resolve_value(path, objects, subtype, limits, budget)?;
            let mut lexer = Lexer {
                bytes: subtype,
                cursor: 0,
            };
            if !matches!(lexer.next(), Some(Token::Name(_))) || lexer.next().is_some() {
                return Err(invalid(path, "resource Subtype is not a name"));
            }
            if !is_font && metadata::is_name(subtype, b"Form") {
                // 幅優先で最短の到達深度を使い、共有 Form は辞書の記載順によらず一度だけ検査する。
                // 間接 alias の循環は resolve_value が拒否する。
                if !forms.insert(budget, number.expect("XObject stream was checked"))? {
                    continue;
                }
                let depth = depth + 1;
                check_limit(
                    "Form resource depth",
                    depth as u64,
                    limits.max_container_nesting as u64,
                )?;
                crate::form_attributes::validate(path, objects, dictionary, limits, budget)?;
                if let Some(value) = metadata::value(dictionary, b"Resources") {
                    let (value, _) = resolve_value(path, objects, value, limits, budget)?;
                    if !is_null(value) {
                        pending.push(budget, (dictionary_value(path, value)?, depth))?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// stream 本文の先頭辞書と stream token を検査する。payload は構造検証時に除去済み。
fn stream_dictionary<'a>(path: &Path, value: &'a [u8]) -> Result<&'a [u8], ParseError> {
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    if !matches!(lexer.next(), Some(Token::Open(true))) {
        return Err(invalid(path, "XObject is not a stream"));
    }
    let dictionary = metadata::dictionary_iter(value)
        .next()
        .ok_or_else(|| invalid(path, "invalid XObject dictionary"))?;
    let end = dictionary.as_ptr() as usize - value.as_ptr() as usize + dictionary.len();
    if !matches!(
        (Lexer {
            bytes: &value[end..],
            cursor: 0
        })
        .next(),
        Some(Token::Word(b"stream"))
    ) {
        return Err(invalid(path, "XObject is not a stream"));
    }
    Ok(dictionary)
}

/// 個別 resource の破損を型付き構造エラーに変換する。
fn invalid(path: &Path, reason: &'static str) -> ParseError {
    ParseError::InvalidPdfStructure {
        path: path.into(),
        reason,
    }
}
