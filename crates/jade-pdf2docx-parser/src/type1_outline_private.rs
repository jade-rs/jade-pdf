//! 復号済みPrivate辞書の静的なglyph/subroutine格納だけを読む。
use crate::decode::DecodeBudget;
use crate::syntax::{Token, integer, whitespace};
#[path = "type1_program_lexer.rs"]
mod lex;
use lex::{Item, Lexer};
#[path = "type1_outline_private_hints.rs"]
mod hints;

/// 同じ復号bufferを借用する一glyphの暗号化program。
pub(super) struct Entry<'a> {
    /// 原本のglyph名。
    pub(super) name: &'a [u8],
    /// RDで長さを宣言したcharstring byte列。
    pub(super) bytes: &'a [u8],
}

/// 動的なPostScriptを実行せずに確認できた辞書。
pub(super) struct Private<'a> {
    /// charstringで捨てるprefix。NoneはlenIV=-1の平文。
    pub(super) prefix: Option<usize>,
    /// 未定義slotを区別したsubroutine。
    pub(super) subrs: Vec<Option<&'a [u8]>>,
    /// 元の名前とprogramを保つglyph。
    pub(super) glyphs: Vec<Entry<'a>>,
    /// 辞書内の完全な定義を照合できた標準OtherSubrs。
    pub(super) standard: Option<super::standard::Verified>,
}

/// 既知の静的なRD/ND/NP定義と、長さで区切った辞書だけを読む。
pub(super) fn parse<'a>(budget: &DecodeBudget, bytes: &'a [u8]) -> Option<Private<'a>> {
    if bytes.len() > crate::parser::MAX_EMBEDDED_FONT_BYTES {
        return None;
    }
    let mut lexer = Lexer { bytes, cursor: 0 };
    words(budget, &mut lexer, &[b"dup"])?;
    name(budget, &mut lexer, b"Private")?;
    if number(budget, &mut lexer)? > 256 {
        return None;
    }
    words(budget, &mut lexer, &[b"dict", b"dup", b"begin"])?;
    procedure(
        budget,
        &mut lexer,
        b"RD",
        &[b"string", b"currentfile", b"exch", b"readstring", b"pop"],
    )?;
    procedure(budget, &mut lexer, b"ND", &[b"noaccess", b"def"])?;
    procedure(budget, &mut lexer, b"NP", &[b"noaccess", b"put"])?;
    let mut prefix = Some(4);
    let mut seen_prefix = false;
    let mut subrs = None;
    let mut standard = None;
    let mut seen_hints = 0_u32;
    let nested = loop {
        match lexer.next(budget)? {
            Item::Pdf(Token::Name(b"lenIV")) if !seen_prefix => {
                let Item::Pdf(Token::Word(value)) = lexer.next(budget)? else {
                    return None;
                };
                prefix = if value == b"-1" {
                    None
                } else {
                    Some(integer(value).filter(|n| *n <= 64)?)
                };
                seen_prefix = true;
                words(budget, &mut lexer, &[b"def"])?;
            }
            Item::Pdf(Token::Name(b"Subrs")) if subrs.is_none() => {
                subrs = Some(subroutines(budget, &mut lexer)?)
            }
            Item::Pdf(Token::Name(b"OtherSubrs")) if standard.is_none() => {
                let (verified, length) =
                    super::standard::verify(budget, &lexer.bytes[lexer.cursor..])?;
                lexer.cursor = lexer.cursor.checked_add(length)?;
                words(budget, &mut lexer, &[b"ND"])?;
                standard = Some(verified);
            }
            Item::Pdf(Token::Name(key)) => hints::skip(budget, &mut lexer, key, &mut seen_hints)?,
            Item::Pdf(Token::Word(b"end")) => {
                words(budget, &mut lexer, &[b"readonly", b"put", b"dup"])?;
                name(budget, &mut lexer, b"CharStrings")?;
                break false;
            }
            Item::Pdf(Token::Word(b"2")) if subrs.is_some() => {
                words(budget, &mut lexer, &[b"index"])?;
                name(budget, &mut lexer, b"CharStrings")?;
                break true;
            }
            _ => return None,
        }
    };
    let capacity = number(budget, &mut lexer)?;
    if capacity == 0 || capacity > 4096 {
        return None;
    }
    words(budget, &mut lexer, &[b"dict", b"dup", b"begin"])?;
    let mut glyphs: Vec<Entry<'a>> = budget.font_vector(capacity)?;
    loop {
        match lexer.next(budget)? {
            Item::Pdf(Token::Name(name)) if glyphs.len() < capacity => {
                if name.is_empty()
                    || name.len() > 127
                    || !name.is_ascii()
                    || glyphs.iter().any(|entry| entry.name == name)
                {
                    return None;
                }
                let bytes = binary(budget, &mut lexer)?;
                words(budget, &mut lexer, &[b"ND"])?;
                glyphs.push(Entry { name, bytes });
            }
            Item::Pdf(Token::Word(b"end")) => break,
            _ => return None,
        }
    }
    if nested {
        words(
            budget,
            &mut lexer,
            &[b"end", b"readonly", b"put", b"put", b"dup"],
        )?;
    } else {
        words(budget, &mut lexer, &[b"readonly", b"put", b"dup"])?;
    }
    name(budget, &mut lexer, b"FontName")?;
    words(
        budget,
        &mut lexer,
        &[
            b"get",
            b"exch",
            b"definefont",
            b"pop",
            b"mark",
            b"currentfile",
            b"closefile",
        ],
    )?;
    if lexer.next(budget).is_some()
        || budget.is_stopped()
        || !glyphs.iter().any(|entry| entry.name == b".notdef")
    {
        return None;
    }
    Some(Private {
        prefix,
        subrs: subrs.unwrap_or_default(),
        glyphs,
        standard,
    })
}

/// procedureの本体と保存方法を照合し、RDを任意の実行手続きへ差し替えさせない。
fn procedure(
    budget: &DecodeBudget,
    lexer: &mut Lexer<'_>,
    expected: &[u8],
    body: &[&[u8]],
) -> Option<()> {
    name(budget, lexer, expected)?;
    if !matches!(lexer.next(budget), Some(Item::Procedure(true))) {
        return None;
    }
    words(budget, lexer, body)?;
    if !matches!(lexer.next(budget), Some(Item::Procedure(false))) {
        return None;
    }
    words(budget, lexer, &[b"executeonly", b"def"])
}

/// 借用programを格納するslotを、確保前に制限する。
fn subroutines<'a>(budget: &DecodeBudget, lexer: &mut Lexer<'a>) -> Option<Vec<Option<&'a [u8]>>> {
    let count = number(budget, lexer)?;
    if count > 16_384 {
        return None;
    }
    words(budget, lexer, &[b"array"])?;
    let mut subrs = budget.font_vector(count)?;
    subrs.resize(count, None);
    loop {
        match lexer.next(budget)? {
            Item::Pdf(Token::Word(b"dup")) => {
                let index = number(budget, lexer)?;
                let slot = subrs.get_mut(index)?;
                if slot.is_some() {
                    return None;
                }
                *slot = Some(binary(budget, lexer)?);
                words(budget, lexer, &[b"NP"])?;
            }
            Item::Pdf(Token::Word(b"def" | b"ND")) => return Some(subrs),
            _ => return None,
        }
    }
}

/// RD直後の一separatorだけを消費し、payload内の空白やcommentを飛ばさない。
fn binary<'a>(budget: &DecodeBudget, lexer: &mut Lexer<'a>) -> Option<&'a [u8]> {
    let length = number(budget, lexer)?;
    words(budget, lexer, &[b"RD"])?;
    let delimiter = *lexer.bytes.get(lexer.cursor)?;
    if !whitespace(delimiter) {
        return None;
    }
    lexer.cursor += 1;
    if delimiter == b'\r' && lexer.bytes.get(lexer.cursor) == Some(&b'\n') {
        lexer.cursor += 1;
    }
    let end = lexer.cursor.checked_add(length)?;
    let bytes = lexer.bytes.get(lexer.cursor..end)?;
    lexer.cursor = end;
    Some(bytes)
}

/// 数値が含む任意の演算を解釈せず、非負整数の字句だけを読む。
fn number(budget: &DecodeBudget, lexer: &mut Lexer<'_>) -> Option<usize> {
    let Item::Pdf(Token::Word(word)) = lexer.next(budget)? else {
        return None;
    };
    integer(word)
}

/// literal nameの一致だけを許し、文字列や実行可能nameで代用させない。
fn name(budget: &DecodeBudget, lexer: &mut Lexer<'_>, expected: &[u8]) -> Option<()> {
    matches!(lexer.next(budget), Some(Item::Pdf(Token::Name(name))) if name == expected)
        .then_some(())
}

/// 有限の既知word列を、元の順序で照合する。
fn words(budget: &DecodeBudget, lexer: &mut Lexer<'_>, expected: &[&[u8]]) -> Option<()> {
    for word in expected {
        if !matches!(lexer.next(budget), Some(Item::Pdf(Token::Word(actual))) if actual == *word) {
            return None;
        }
    }
    Some(())
}

#[cfg(test)]
#[path = "type1_outline_private_tests.rs"]
mod tests;
