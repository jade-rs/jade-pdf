//! コメント・文字列・streamを除き、name直後の完全な間接参照を借用する。
use crate::syntax::{Lexer, Token, integer};

/// 追加配列を作らず、元入力のresource名と参照番号/世代を列挙する。
pub(crate) fn named(bytes: &[u8]) -> impl Iterator<Item = (&[u8], u32, u16)> {
    let mut lexer = Lexer { bytes, cursor: 0 };
    let mut depth = 0usize;
    let mut dictionary_start = None;
    let mut stream_length = None;
    std::iter::from_fn(move || {
        while let Some((token, span)) = lexer.next_spanned() {
            match token {
                Token::Name(name) => {
                    let mut value = lexer.clone();
                    if let Some(Token::Word(number)) = value.next()
                        && let Some(number) = integer(number).and_then(|n| u32::try_from(n).ok())
                        && let Some(Token::Word(generation)) = value.next()
                        && let Some(generation) =
                            integer(generation).and_then(|n| u16::try_from(n).ok())
                        && matches!(value.next(), Some(Token::Word(b"R")))
                    {
                        lexer = value;
                        return Some((name, number, generation));
                    }
                }
                Token::Open(dictionary) => {
                    if depth == 0 {
                        dictionary_start = dictionary.then_some(span.start);
                    }
                    depth += 1;
                }
                Token::Close(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        stream_length = dictionary_start.take().and_then(|start| {
                            crate::metadata::value(&bytes[start..span.end], b"Length")
                                .and_then(|value| integer(value.trim_ascii()))
                        });
                    }
                }
                Token::Word(b"stream") if depth == 0 => {
                    lexer.skip_stream(stream_length.take());
                }
                Token::Word(b"endobj") if depth == 0 => {
                    stream_length = None;
                }
                _ => {}
            }
        }
        None
    })
}
