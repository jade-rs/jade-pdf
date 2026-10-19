//! 互換入力のCMap streamを借用字句で選び、番号suffixや偽headerを除外する。
use crate::{
    decode::DecodeBudget,
    syntax::{Lexer, Token, integer},
};

/// headerとdictionaryを複製せず、generation 0の参照先と本文を借用する。
pub(super) fn find<'a>(
    budget: &DecodeBudget,
    bytes: &'a [u8],
    number: u32,
) -> Option<(&'a [u8], &'a [u8])> {
    let mut lexer = Lexer { bytes, cursor: 0 };
    let mut depth = 0usize;
    let mut object = None;
    while !budget.is_stopped() {
        let (token, span) = lexer.next_spanned()?;
        if depth == 0
            && object.is_none()
            && let Token::Word(raw) = token
            && let Some(found) = integer(raw).and_then(|n| u32::try_from(n).ok())
        {
            let mut header = lexer.clone();
            if let Some(Token::Word(raw)) = header.next()
                && let Some(generation) = integer(raw).and_then(|n| u16::try_from(n).ok())
                && matches!(header.next(), Some(Token::Word(b"obj")))
            {
                object = Some((found, generation, header.cursor));
                lexer = header;
                continue;
            }
        }
        match token {
            Token::Open(_) => depth = depth.saturating_add(1),
            Token::Close(_) => depth = depth.saturating_sub(1),
            Token::Word(b"endobj") if depth == 0 => {
                if object.is_some_and(|(found, generation, _)| found == number && generation == 0) {
                    return None;
                }
                object = None;
            }
            Token::Word(b"stream") if depth == 0 => {
                let dictionary = object.map_or(&bytes[span.start..span.start], |(_, _, start)| {
                    &bytes[start..span.start]
                });
                let (data, next) = crate::legacy_stream::payload(bytes, dictionary, span.start)?;
                if object.is_some_and(|(found, generation, _)| found == number && generation == 0) {
                    return Some((dictionary, data));
                }
                lexer.cursor = next;
            }
            _ => {}
        }
    }
    None
}
