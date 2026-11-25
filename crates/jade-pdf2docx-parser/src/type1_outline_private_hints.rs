//! 外形を変更しない既知のhintデータだけを、有限のliteralとして検証する。
use super::{Item, Lexer, Token, words};
use crate::decode::DecodeBudget;

/// データ辞書の既知キー。RDなどの実行手続きの上書きは含めない。
const KEYS: &[&[u8]] = &[
    b"MinFeature",
    b"password",
    b"BlueValues",
    b"OtherBlues",
    b"FamilyBlues",
    b"FamilyOtherBlues",
    b"StdHW",
    b"StdVW",
    b"StemSnapH",
    b"StemSnapV",
    b"BlueScale",
    b"BlueShift",
    b"BlueFuzz",
    b"ForceBold",
    b"LanguageGroup",
    b"ExpansionFactor",
];

/// 値を評価せずliteralとして読み、重複や動的式を拒否する。
pub(super) fn skip(
    budget: &DecodeBudget,
    lexer: &mut Lexer<'_>,
    key: &[u8],
    seen: &mut u32,
) -> Option<()> {
    let index = KEYS.iter().position(|known| *known == key)?;
    let mask = 1_u32.checked_shl(index as u32)?;
    if *seen & mask != 0 {
        return None;
    }
    *seen |= mask;
    match index {
        0 => {
            if !matches!(lexer.next(budget), Some(Item::Procedure(true))) {
                return None;
            }
            words(budget, lexer, &[b"16", b"16"])?;
            if !matches!(lexer.next(budget), Some(Item::Procedure(false))) {
                return None;
            }
            words(budget, lexer, &[b"ND"])
        }
        1 => words(budget, lexer, &[b"5839", b"def"]),
        2..=9 => {
            if !matches!(lexer.next(budget), Some(Item::Pdf(Token::Open(false)))) {
                return None;
            }
            for count in 0..=32 {
                match lexer.next(budget)? {
                    Item::Pdf(Token::Close(false)) => return words(budget, lexer, &[b"ND"]),
                    Item::Pdf(Token::Word(number)) if count < 32 && literal_number(number) => {}
                    _ => return None,
                }
            }
            None
        }
        13 => {
            if !matches!(
                lexer.next(budget),
                Some(Item::Pdf(Token::Word(b"true" | b"false")))
            ) {
                return None;
            }
            words(budget, lexer, &[b"def"])
        }
        _ => {
            let Item::Pdf(Token::Word(number)) = lexer.next(budget)? else {
                return None;
            };
            if !literal_number(number) {
                return None;
            }
            words(budget, lexer, &[b"def"])
        }
    }
}

/// 操作名・非有限数・極端な値を、数値として扱わない。
fn literal_number(bytes: &[u8]) -> bool {
    bytes.len() <= 64
        && std::str::from_utf8(bytes)
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .is_some_and(|v| v.is_finite() && v.abs() <= 65535.)
}
