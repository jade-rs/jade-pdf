//! 画像とsoft maskのFlate差分を、展開済みbuffer内で復元する。
use super::{DecodeBudget, image_attribute, image_decode_parameters};
use crate::{
    indexed_predictor::Predictor,
    syntax::{Lexer, Token},
};

/// 直接・間接・nullの整数属性を、端数を丸めず既定値とともに読む。
fn number(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary: &[u8],
    key: &[u8],
    default: usize,
) -> Option<usize> {
    if crate::metadata::value(dictionary, key).is_none() {
        return Some(default);
    }
    let value = image_attribute(budget, pdf, dictionary, key)?;
    let mut lexer = Lexer {
        bytes: &value,
        cursor: 0,
    };
    let Token::Word(word) = lexer.next()? else {
        return None;
    };
    if lexer.next().is_some() {
        return None;
    }
    if word == b"null" {
        return Some(default);
    }
    std::str::from_utf8(word).ok()?.parse().ok()
}

/// Predictorを復元し、差分ありなら復元後の画素byte数も検証する。
pub(super) fn apply(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary: &[u8],
    data: &mut Vec<u8>,
    expected: usize,
) -> Option<()> {
    if budget.is_stopped() {
        return None;
    }
    let parameters = image_decode_parameters(budget, pdf, dictionary)?;
    let changed = restore(budget, pdf, &parameters, data)?;
    (!changed || data.len() == expected).then_some(())
}

/// 圧縮codecの前段にも使えるよう、画素数を仮定せず予測差分を復元する。
pub(super) fn restore(
    budget: &DecodeBudget,
    pdf: &[u8],
    parameters: &[u8],
    data: &mut Vec<u8>,
) -> Option<bool> {
    if budget.is_stopped() {
        return None;
    }
    let kind = number(budget, pdf, parameters, b"Predictor", 1)?;
    if kind == 1 {
        return Some(false);
    }
    if !matches!(kind, 2 | 10..=15) {
        return None;
    }
    let predictor = Predictor::new(
        kind,
        number(budget, pdf, parameters, b"Colors", 1)?,
        number(budget, pdf, parameters, b"BitsPerComponent", 8)?,
        number(budget, pdf, parameters, b"Columns", 1)?,
    );
    predictor
        .apply_controlled(std::path::Path::new("image"), data, || budget.is_stopped())
        .ok()?;
    (!budget.is_stopped()).then_some(true)
}
