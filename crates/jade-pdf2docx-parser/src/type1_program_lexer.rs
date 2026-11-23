//! PDFの安全な文字列・comment走査を再利用し、PostScriptのprocedure区切りだけを加える。
use crate::{
    decode::DecodeBudget,
    syntax::{Lexer as PdfLexer, Token},
};

/// 借用tokenと、実行しないprocedureの開始・終了。
#[derive(Clone, Copy)]
pub(super) enum Item<'a> {
    /// PDFと共通のname・word・array・文字列。
    Pdf(Token<'a>),
    /// trueはprocedureの開始、falseは終了。
    Procedure(bool),
}

/// 再確保せず、cleartext内のtokenを一つずつ返す。
#[derive(Clone)]
pub(super) struct Lexer<'a> {
    /// 解析元のcleartext。
    pub(super) bytes: &'a [u8],
    /// 次の未消費byte位置。
    pub(super) cursor: usize,
}

impl<'a> Lexer<'a> {
    /// 文字列・comment中のbraceは無視し、隣接したprocedure delimiterも分離する。
    pub(super) fn next(&mut self, budget: &DecodeBudget) -> Option<Item<'a>> {
        if budget.is_stopped() {
            return None;
        }
        let mut source = PdfLexer {
            bytes: self.bytes,
            cursor: self.cursor,
        };
        let (token, span) = source.next_spanned()?;
        self.cursor = source.cursor;
        if &self.bytes[span.clone()] == b"{" {
            return Some(Item::Procedure(true));
        }
        if &self.bytes[span.clone()] == b"}" {
            return Some(Item::Procedure(false));
        }
        let raw = match token {
            Token::Name(raw) | Token::Word(raw) => raw,
            _ => return Some(Item::Pdf(token)),
        };
        let Some(offset) = raw.iter().position(|byte| matches!(byte, b'{' | b'}')) else {
            return Some(Item::Pdf(token));
        };
        let start = span.end - raw.len();
        if offset == 0 && matches!(token, Token::Word(_)) {
            self.cursor = start + 1;
            return Some(Item::Procedure(raw[0] == b'{'));
        }
        self.cursor = start + offset;
        Some(Item::Pdf(match token {
            Token::Name(_) => Token::Name(&raw[..offset]),
            _ => Token::Word(&raw[..offset]),
        }))
    }
}
