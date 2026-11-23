//! headerで認める有限の値と、実行を伴わない終端だけを読む。
use super::{DecodeBudget, Item, Lexer, Token};

/// 想定した演算子を一つだけ消費する。
pub(super) fn word(budget: &DecodeBudget, lexer: &mut Lexer<'_>, expected: &[u8]) -> Option<()> {
    matches!(lexer.next(budget),Some(Item::Pdf(Token::Word(actual))) if actual==expected)
        .then_some(())
}

/// 長さを制限した有限の十進数を読む。
pub(super) fn number(budget: &DecodeBudget, lexer: &mut Lexer<'_>) -> Option<f64> {
    let Item::Pdf(Token::Word(raw)) = lexer.next(budget)? else {
        return None;
    };
    if raw.len() > 64 {
        return None;
    }
    let value = std::str::from_utf8(raw).ok()?.parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

/// FSTypeの元bitを整数として読み、許可判定は行わない。
pub(super) fn permission(budget: &DecodeBudget, lexer: &mut Lexer<'_>) -> Option<u16> {
    let value = number(budget, lexer)?;
    (value.fract() == 0. && (0. ..=65535.).contains(&value)).then_some(value as u16)
}

/// 任意のprocedureを実行せず、静的な数値列だけを持つarrayまたはbraceを読む。
pub(super) fn numbers<const N: usize>(
    budget: &DecodeBudget,
    lexer: &mut Lexer<'_>,
) -> Option<[f64; N]> {
    let procedure = match lexer.next(budget)? {
        Item::Procedure(true) => true,
        Item::Pdf(Token::Open(false)) => false,
        _ => return None,
    };
    let mut values = [0.; N];
    for value in &mut values {
        *value = number(budget, lexer)?;
    }
    match (procedure, lexer.next(budget)?) {
        (true, Item::Procedure(false)) | (false, Item::Pdf(Token::Close(false))) => Some(values),
        _ => None,
    }
}

/// 値の後に実行式を挿入できないよう、optional readonlyとdefだけを通す。
pub(super) fn definition(budget: &DecodeBudget, lexer: &mut Lexer<'_>) -> Option<()> {
    match lexer.next(budget)? {
        Item::Pdf(Token::Word(b"def")) => Some(()),
        Item::Pdf(Token::Word(b"readonly")) => word(budget, lexer, b"def"),
        _ => None,
    }
}

/// 一つのPostScript literal文字列を、escapeを展開せずに借用する。
pub(super) fn literal<'a>(budget: &DecodeBudget, lexer: &mut Lexer<'a>) -> Option<&'a [u8]> {
    loop {
        if budget.is_stopped() {
            return None;
        }
        let byte = *lexer.bytes.get(lexer.cursor)?;
        if crate::syntax::whitespace(byte) {
            lexer.cursor += 1;
            continue;
        }
        if byte == b'%' {
            while let Some(&byte) = lexer.bytes.get(lexer.cursor) {
                if budget.is_stopped() {
                    return None;
                }
                lexer.cursor += 1;
                if byte == b'\r' || byte == b'\n' {
                    break;
                }
            }
            continue;
        }
        break;
    }
    if lexer.bytes.get(lexer.cursor) != Some(&b'(') {
        return None;
    }
    lexer.cursor += 1;
    let start = lexer.cursor;
    let mut depth = 1u8;
    loop {
        if budget.is_stopped() || lexer.cursor - start > 65535 {
            return None;
        }
        let byte = *lexer.bytes.get(lexer.cursor)?;
        lexer.cursor += 1;
        match byte {
            b'\\' => {
                lexer.bytes.get(lexer.cursor)?;
                lexer.cursor += 1;
            }
            b'(' => {
                depth = depth.checked_add(1)?;
                if depth > 32 {
                    return None;
                }
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&lexer.bytes[start..lexer.cursor - 1]);
                }
            }
            _ => {}
        }
    }
}
