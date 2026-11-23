//! Encodingへ代入しない既知のfont cache前処理だけを字句として確認する。
use super::lex::{Item, Lexer};
use crate::{decode::DecodeBudget, syntax::Token};

/// 同じfont名の存在・UniqueID・FontTypeの比較だけを行う慣用句を消費する。
pub(super) fn skip(budget: &DecodeBudget, lexer: &mut Lexer<'_>) -> Option<()> {
    let Item::Pdf(Token::Name(name)) = lexer.next(budget)? else {
        return None;
    };
    if name.is_empty() {
        return None;
    }
    expect(budget, lexer, b"known")?;
    expect(budget, lexer, b"{")?;
    if !matches!(lexer.next(budget), Some(Item::Pdf(Token::Name(other))) if other == name) {
        return None;
    }
    for word in [
        b"findfont".as_slice(),
        b"dup",
        b"/UniqueID",
        b"known",
        b"{",
        b"dup",
        b"/UniqueID",
        b"get",
    ] {
        expect(budget, lexer, word)?;
    }
    let Item::Pdf(Token::Word(number)) = lexer.next(budget)? else {
        return None;
    };
    crate::syntax::integer(number)?;
    for word in [
        b"eq".as_slice(),
        b"exch",
        b"/FontType",
        b"get",
        b"1",
        b"eq",
        b"and",
        b"}",
        b"{",
        b"pop",
        b"false",
        b"}",
        b"ifelse",
        b"{",
        b"save",
        b"true",
        b"}",
        b"{",
        b"false",
        b"}",
        b"ifelse",
        b"}",
        b"{",
        b"false",
        b"}",
        b"ifelse",
    ] {
        expect(budget, lexer, word)?;
    }
    Some(())
}

/// 名前と演算子とprocedure delimiterを区別し、隠れた実行式を受け入れない。
fn expect(budget: &DecodeBudget, lexer: &mut Lexer<'_>, expected: &[u8]) -> Option<()> {
    let token = lexer.next(budget)?;
    let same = match (expected, token) {
        (b"{", Item::Procedure(true)) | (b"}", Item::Procedure(false)) => true,
        (_, Item::Pdf(Token::Name(actual))) => expected.strip_prefix(b"/") == Some(actual),
        (_, Item::Pdf(Token::Word(actual))) => expected == actual,
        _ => false,
    };
    same.then_some(())
}
