//! FontInfoの静的な値を読み、元の指定を推測した既定値へ置き換えない。
use super::{DecodeBudget, Info, Item, Lexer, Token, values};

/// 既知の文字列・数値・boolだけを含むFontInfoを、重複を拒否して読む。
pub(super) fn parse<'a>(budget: &DecodeBudget, lexer: &mut Lexer<'a>) -> Option<Info<'a>> {
    let size = values::number(budget, lexer)?;
    if !(0. ..=256.).contains(&size) || size.fract() != 0. {
        return None;
    }
    for expected in [b"dict".as_slice(), b"dup", b"begin"] {
        values::word(budget, lexer, expected)?;
    }
    let mut info = Info::default();
    let mut seen = 0u16;
    loop {
        let key = match lexer.next(budget)? {
            Item::Pdf(Token::Word(b"end")) => break,
            Item::Pdf(Token::Name(key)) => key,
            _ => return None,
        };
        let bit = match key {
            b"Notice" => 0,
            b"Copyright" => 1,
            b"version" => 2,
            b"FullName" => 3,
            b"FamilyName" => 4,
            b"Weight" => 5,
            b"ItalicAngle" => 6,
            b"isFixedPitch" => 7,
            b"UnderlinePosition" => 8,
            b"UnderlineThickness" => 9,
            b"FSType" => 10,
            b"ascent" => 11,
            b"descent" => 12,
            _ => return None,
        };
        if seen & (1 << bit) != 0 {
            return None;
        }
        seen |= 1 << bit;
        match key {
            b"Notice" => info.notice = Some(values::literal(budget, lexer)?),
            b"Copyright" => info.copyright = Some(values::literal(budget, lexer)?),
            b"version" => info.version = Some(values::literal(budget, lexer)?),
            b"FullName" => info.full_name = Some(values::literal(budget, lexer)?),
            b"FamilyName" => info.family = Some(values::literal(budget, lexer)?),
            b"Weight" => info.weight = Some(values::literal(budget, lexer)?),
            b"ItalicAngle" => info.italic_angle = Some(values::number(budget, lexer)?),
            b"isFixedPitch" => {
                info.fixed = Some(match lexer.next(budget)? {
                    Item::Pdf(Token::Word(b"true")) => true,
                    Item::Pdf(Token::Word(b"false")) => false,
                    _ => return None,
                })
            }
            b"UnderlinePosition" => info.underline_position = Some(values::number(budget, lexer)?),
            b"UnderlineThickness" => {
                let value = values::number(budget, lexer)?;
                if value < 0. {
                    return None;
                }
                info.underline_thickness = Some(value);
            }
            b"FSType" => info.fs_type = Some(values::permission(budget, lexer)?),
            b"ascent" => info.ascent = Some(values::number(budget, lexer)?),
            b"descent" => info.descent = Some(values::number(budget, lexer)?),
            _ => return None,
        }
        values::definition(budget, lexer)?;
    }
    values::definition(budget, lexer)?;
    Some(info)
}
