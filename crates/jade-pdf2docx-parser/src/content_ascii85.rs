//! ASCII85→Flate本文の宣言順・parameter・二段の展開予算を検査する。
use super::*;
use crate::syntax::{Lexer, Token};

/// Filter値のASCII85名だけを検出し、未対応の順序を既存のFlate探索へ流さない。
pub(super) fn handles(dictionary: &[u8]) -> bool {
    let Some(filter) = crate::metadata::value(dictionary, b"Filter") else {
        return false;
    };
    let mut lexer = Lexer {
        bytes: filter,
        cursor: 0,
    };
    while let Some(token) = lexer.next() {
        if let Token::Name(name) = token
            && crate::metadata_names::resource_name_order(name, "ASCII85Decode").is_eq()
        {
            return true;
        }
    }
    false
}

/// 二つのfilterと既定parameterを要求し、破損を空白の成功として返さない。
pub(super) fn decode(budget: &DecodeBudget, dictionary: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    let filters = crate::metadata::value(dictionary, b"Filter")?;
    let supported = crate::metadata::array_entries(filters, 2).is_some_and(|mut entries| {
        entries
            .next()
            .flatten()
            .is_some_and(|v| crate::metadata::is_name(v, b"ASCII85Decode"))
            && entries
                .next()
                .flatten()
                .is_some_and(|v| crate::metadata::is_name(v, b"FlateDecode"))
            && entries.next().is_none()
    }) && crate::metadata::value(dictionary, b"DecodeParms").is_none_or(|value| {
        crate::page_attributes::is_null(value)
            || crate::metadata::array_entries(value, 2).is_some_and(|mut entries| {
                entries
                    .next()
                    .flatten()
                    .is_some_and(crate::page_attributes::is_null)
                    && entries
                        .next()
                        .flatten()
                        .is_some_and(crate::page_attributes::is_null)
                    && entries.next().is_none()
            })
    });
    if !supported {
        budget.reject_content(
            std::io::ErrorKind::Unsupported,
            "unsupported ASCII85 content filter chain or DecodeParms",
        );
        return None;
    }
    let decoded = image_ascii85::decode(data, budget).and_then(|bytes| decode_zlib(&bytes, budget));
    if decoded.is_none() {
        budget.reject_content(
            std::io::ErrorKind::InvalidData,
            "invalid ASCII85 Flate content stream",
        );
    }
    decoded
}
