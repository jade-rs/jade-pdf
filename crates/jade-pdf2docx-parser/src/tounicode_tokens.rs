//! CMap operandを借用し、コメント・文字列・名前を命令やhexと混同しない。
use crate::{
    decode::DecodeBudget,
    syntax::{Lexer, Token as PdfToken},
};
use std::iter::Peekable;
/// CMapに必要な字句だけを区別し、不正hexも一つのoperandとして残す。
#[derive(Clone, Copy)]
pub(super) enum Token<'a> {
    /// 実際の裸のkeyword。
    Word(&'a [u8]),
    /// ASCII hex payload。不正な内容はNone、空payloadはSome("")。
    Hex(Option<&'a str>),
    /// 辞書（true）または配列（false）の開始。
    Open(bool),
    /// 辞書（true）または配列（false）の終了。
    Close(bool),
    /// 命令として解釈しない名前・literalなど。
    Other,
}
/// 元UTF-8本文の範囲だけを保持し、正規化用の全文コピーを作らない。
#[derive(Clone)]
pub(super) struct Tokens<'a> {
    /// 元CMap本文。
    source: &'a str,
    /// 共通のPDF字句走査器。
    lexer: Lexer<'a>,
}
impl<'a> Tokens<'a> {
    /// 未走査のCMapを借用する。
    pub(super) fn new(source: &'a str) -> Self {
        Self {
            source,
            lexer: Lexer {
                bytes: source.as_bytes(),
                cursor: 0,
            },
        }
    }
}
impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;
    /// hexだけをpayloadへ変換し、literal内の山括弧を取り込まない。
    fn next(&mut self) -> Option<Self::Item> {
        let (token, span) = self.lexer.next_spanned()?;
        Some(match token {
            PdfToken::Word(word) => Token::Word(word),
            PdfToken::Open(kind) => Token::Open(kind),
            PdfToken::Close(kind) => Token::Close(kind),
            PdfToken::Other if self.source.as_bytes()[span.start] == b'<' => {
                let raw = &self.source[span];
                let hex = raw
                    .strip_prefix('<')
                    .and_then(|s| s.strip_suffix('>'))
                    .filter(|s| s.bytes().all(|b| b.is_ascii_hexdigit()));
                Token::Hex(hex)
            }
            _ => Token::Other,
        })
    }
}
/// 閉じたdestination配列の最外hexだけを読み、後続recordのcursorを渡せる走査器。
#[derive(Clone)]
pub(super) struct ArrayHex<'a, 'budget> {
    /// 閉じた配列の直後まで進める字句列。
    pub(super) tokens: Peekable<Tokens<'a>>,
    /// 元の配列を実際に閉じたか。
    pub(super) closed: bool,
    /// 再帰せずに数える入れ子の深さ。
    depth: usize,
    /// 走査時間と既存停止状態を共有する予算。
    budget: &'budget DecodeBudget,
}
impl<'a, 'budget> ArrayHex<'a, 'budget> {
    /// 開始角括弧の直後から読み始める。
    pub(super) fn new(tokens: Peekable<Tokens<'a>>, budget: &'budget DecodeBudget) -> Self {
        Self {
            tokens,
            closed: false,
            depth: 1,
            budget,
        }
    }
}
impl<'a> Iterator for ArrayHex<'a, '_> {
    type Item = &'a str;
    /// 入れ子をmappingへ混ぜず、配列終端や予算停止で必ず終わる。
    fn next(&mut self) -> Option<Self::Item> {
        if self.closed {
            return None;
        }
        while self.budget.check().is_ok() {
            match self.tokens.next()? {
                Token::Open(_) => self.depth = self.depth.checked_add(1)?,
                Token::Close(kind) => {
                    self.depth = self.depth.checked_sub(1)?;
                    if self.depth == 0 {
                        self.closed = !kind;
                        return None;
                    }
                }
                Token::Hex(Some(hex)) if self.depth == 1 => return Some(hex),
                _ => {}
            }
        }
        None
    }
}
