//! Type1のcleartextから静的なEncodingを読み、PostScriptを実行しない。
use crate::decode::DecodeBudget;

/// 組み込み表へ適用する、明示的な初期Encoding。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Base {
    /// 初期値が.notdefの、256個の独自slot。
    Custom,
    /// Adobe StandardEncodingを元にする。
    Standard,
    /// PostScriptのISOLatin1Encodingを元にする。
    Latin1,
}

/// 入力を借用する256slotと、その初期表。
pub(crate) struct Encoding<'a> {
    /// assignmentのないslotが参照する初期表。
    pub(crate) base: Base,
    /// 後の静的assignmentを適用したglyph名。Noneは初期表を使用する。
    pub(crate) slots: Vec<Option<&'a [u8]>>,
}

#[path = "type1_program_lexer.rs"]
mod lex;
#[path = "type1_preamble.rs"]
mod preamble;
use crate::syntax::Token;
use lex::{Item, Lexer};

/// 有限の静的Encodingのみを解釈し、動的式や破損した表は確認不能として返す。
pub(crate) fn parse<'a>(budget: &DecodeBudget, program: &'a [u8]) -> Option<Encoding<'a>> {
    let mut lexer = Lexer {
        bytes: program,
        cursor: 0,
    };
    let mut table = None;
    let mut procedures = 0usize;
    let mut containers = 0usize;
    let mut dictionaries = 0usize;
    let mut ended = false;
    while let Some(token) = lexer.next(budget) {
        match token {
            Item::Procedure(true) => procedures = procedures.checked_add(1)?,
            Item::Procedure(false) => procedures = procedures.checked_sub(1)?,
            _ if procedures != 0 => {}
            Item::Pdf(Token::Open(_)) => containers = containers.checked_add(1)?,
            Item::Pdf(Token::Close(_)) => containers = containers.checked_sub(1)?,
            _ if containers != 0 => {}
            Item::Pdf(Token::Word(b"FontDirectory")) if dictionaries == 0 && table.is_none() => {
                preamble::skip(budget, &mut lexer)?;
            }
            Item::Pdf(Token::Word(b"begin")) => dictionaries = dictionaries.checked_add(1)?,
            Item::Pdf(Token::Word(b"end")) => dictionaries = dictionaries.checked_sub(1)?,
            _ if dictionaries > 1 => {}
            Item::Pdf(Token::Name(b"Encoding")) => table = Some(definition(budget, &mut lexer)?),
            Item::Pdf(Token::Name(
                b"def" | b"put" | b"dup" | b"array" | b"copy" | b"begin" | b"end",
            )) => return None,
            Item::Pdf(Token::Word(b"eexec")) => {
                ended = true;
                break;
            }
            Item::Pdf(Token::Word(b"Encoding")) => return None,
            Item::Pdf(Token::Word(word)) if !simple_word(word) => return None,
            _ => {}
        }
    }
    if budget.is_stopped() || !ended || procedures != 0 || containers != 0 {
        return None;
    }
    table
}

/// 一つの明示的な表の作成を、既知の静的演算だけでdefまで進める。
fn definition<'a>(budget: &DecodeBudget, lexer: &mut Lexer<'a>) -> Option<Encoding<'a>> {
    let first = lexer.next(budget)?;
    let (base, mut mutable) = match first {
        Item::Pdf(Token::Word(b"StandardEncoding")) => (Base::Standard, false),
        Item::Pdf(Token::Word(b"ISOLatin1Encoding")) => (Base::Latin1, false),
        Item::Pdf(Token::Word(b"256")) => {
            word(budget, lexer, b"array")?;
            (Base::Custom, true)
        }
        _ => return None,
    };
    let mut slots = budget.metadata_vector(256)?;
    slots.resize(256, None);
    let mut table = Encoding { base, slots };
    if !mutable {
        let mut lookahead = lexer.clone();
        if matches!(lookahead.next(budget), Some(Item::Pdf(Token::Word(b"256")))) {
            word(budget, &mut lookahead, b"array")?;
            word(budget, &mut lookahead, b"copy")?;
            *lexer = lookahead;
            mutable = true;
        }
    }
    loop {
        match lexer.next(budget)? {
            Item::Pdf(Token::Word(b"def")) => return Some(table),
            Item::Pdf(Token::Word(b"readonly" | b"noaccess")) => mutable = false,
            Item::Pdf(Token::Word(b"dup")) if mutable => {
                let Item::Pdf(Token::Word(index)) = lexer.next(budget)? else {
                    return None;
                };
                let index = std::str::from_utf8(index).ok()?.parse::<usize>().ok()?;
                let Item::Pdf(Token::Name(name)) = lexer.next(budget)? else {
                    return None;
                };
                if name.is_empty() {
                    return None;
                }
                word(budget, lexer, b"put")?;
                *table.slots.get_mut(index)? = Some(name);
            }
            Item::Pdf(Token::Word(b"0")) if mutable => {
                word(budget, lexer, b"1")?;
                word(budget, lexer, b"255")?;
                if !matches!(lexer.next(budget), Some(Item::Procedure(true))) {
                    return None;
                }
                for expected in [b"1".as_slice(), b"index", b"exch"] {
                    word(budget, lexer, expected)?;
                }
                if !matches!(lexer.next(budget), Some(Item::Pdf(Token::Name(b".notdef")))) {
                    return None;
                }
                word(budget, lexer, b"put")?;
                if !matches!(lexer.next(budget), Some(Item::Procedure(false))) {
                    return None;
                }
                word(budget, lexer, b"for")?;
                table.base = Base::Custom;
                table.slots.fill(None);
            }
            _ => return None,
        }
    }
}

/// 静的表の構文で要求するkeywordを、追加確保せず一つ消費する。
fn word(budget: &DecodeBudget, lexer: &mut Lexer<'_>, expected: &[u8]) -> Option<()> {
    matches!(lexer.next(budget), Some(Item::Pdf(Token::Word(actual))) if actual == expected)
        .then_some(())
}

/// 未知のprocedure実行を受け入れず、通常headerの有限な値と演算だけを通す。
fn simple_word(word: &[u8]) -> bool {
    matches!(
        word,
        b"dict"
            | b"dup"
            | b"def"
            | b"readonly"
            | b"noaccess"
            | b"executeonly"
            | b"bind"
            | b"currentdict"
            | b"currentfile"
            | b"true"
            | b"false"
            | b"pop"
    ) || std::str::from_utf8(word)
        .ok()
        .and_then(|word| word.parse::<f64>().ok())
        .is_some_and(f64::is_finite)
}

#[cfg(test)]
#[path = "type1_program_tests.rs"]
mod tests;
