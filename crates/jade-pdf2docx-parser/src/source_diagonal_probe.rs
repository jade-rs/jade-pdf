//! 対角strokeがないページでは追加のResource解決・q保存を始めず、従来の資源制限を保つ。
use super::super::stroked_rectangles::drawing_operand;
use crate::{
    decode::DecodeBudget,
    syntax::{Lexer, Token},
    text_draws::Operand,
};

/// 定数容量の字句走査だけで候補を探す。CTMがある場合は保守的に実際の走査へ渡す。
/// q/Qの行列をここで再現せず、肯定した候補の色・clip・一様変換は本走査で検証する。
pub(super) fn has_candidate(budget: &DecodeBudget, content: &[u8]) -> bool {
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    let mut args = [Operand::Other; 6];
    let mut count = 0;
    let mut point: Option<[f32; 2]> = None;
    let mut transformed = false;
    let mut diagonal = false;
    while let Some((token, span)) = lexer.next_spanned() {
        if budget.is_stopped() {
            return false;
        }
        let Token::Word(word) = token else {
            let Some(value) = drawing_operand(&mut lexer, token, span.start) else {
                return false;
            };
            if count < args.len() {
                args[count] = value;
                count += 1;
            } else {
                count = 0;
            }
            continue;
        };
        if let Some(value) = crate::page_attributes::number_value(word) {
            if count < args.len() {
                args[count] = Operand::Number(value);
                count += 1;
            } else {
                count = 0;
            }
            continue;
        }
        match (word, &args[..count]) {
            (b"cm", _) => transformed = true,
            (b"m" | b"l", [Operand::Number(x), Operand::Number(y)]) => {
                if word == b"l" {
                    if let Some([old_x, old_y]) = point {
                        diagonal |= transformed || (old_x != *x && old_y != *y);
                    }
                }
                point = Some([*x, *y]);
            }
            (b"S", []) if diagonal => return true,
            (b"n" | b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*", _) => {
                point = None;
                diagonal = false;
            }
            (b"c" | b"v" | b"y", _) => {
                point = None;
                diagonal = false;
            }
            (b"BI", _) => return false,
            _ => {}
        }
        count = 0;
    }
    false
}
