//! Form探索中の文字状態を固定長operandで追跡し、本文抽出と同じ更新規則を使う。
use crate::{
    decode::DecodeBudget,
    syntax::Token,
    text_draws::{Operand, State},
};
use std::ops::Range;

/// Formへ渡す選択済み文字状態と、そのFontを選択したResources。
pub(crate) struct Inherited<'a> {
    /// 呼び出し直前の文字状態。
    pub(crate) state: &'a State,
    /// 選択済みFontの定義を解決する呼び出し元辞書。
    pub(crate) resources: &'a [u8],
}

/// 借用本文から後で復元する固定長operand。
#[derive(Clone)]
enum Value {
    /// 有限な数値。
    Number(f32),
    /// resource名のbyte範囲。
    Name(Range<usize>),
    /// 文字列のbyte範囲。
    String(Range<usize>),
    /// その他のoperand。
    Other,
}

/// q/Qの文字状態と独立した字句走査状態。
pub(crate) struct Operands {
    /// 最大6個までの直前operand。
    values: [Value; 6],
    /// 現在保持するoperandの個数。
    len: usize,
    /// 配列・辞書内の偽命令を除外する深度。
    depth: usize,
    /// TDと引用符の状態更新を許可するtext objectの有無。
    in_text: bool,
}

impl Default for Operands {
    /// 初期text object外の空のoperand列を作る。
    fn default() -> Self {
        Self {
            values: std::array::from_fn(|_| Value::Other),
            len: 0,
            depth: 0,
            in_text: false,
        }
    }
}

impl Operands {
    /// 借用字句を追跡して共有文字状態を更新し、有効なTfだけtrueを返す。
    pub(crate) fn observe(
        &mut self,
        budget: &DecodeBudget,
        state: &mut State,
        token: Token<'_>,
        span: Range<usize>,
        bytes: &[u8],
        mut alpha: impl FnMut(&[u8]) -> crate::text_alpha::Parameters,
    ) -> bool {
        let value = match token {
            Token::Open(_) => {
                if self.depth == 0 {
                    self.push(Value::Other);
                }
                self.depth += 1;
                return false;
            }
            Token::Close(_) => {
                self.depth = self.depth.saturating_sub(1);
                return false;
            }
            _ if self.depth != 0 => return false,
            Token::Name(_) => Value::Name(span.start + 1..span.end),
            Token::Word(word) => {
                if let Some(number) = crate::page_attributes::number_value(word) {
                    Value::Number(number)
                } else {
                    let args = std::array::from_fn::<_, 6, _>(|index| match &self.values[index] {
                        Value::Number(value) => Operand::Number(*value),
                        Value::Name(range) => Operand::Name(&bytes[range.clone()]),
                        Value::String(range) => Operand::String(&bytes[range.clone()]),
                        Value::Other => Operand::Other,
                    });
                    state.alpha.update(word, &args[..self.len], &mut alpha);
                    let selected = state.update(budget, word, &args[..self.len], self.in_text);
                    if word == b"BT" && self.len == 0 {
                        self.in_text = true;
                    }
                    if word == b"ET" && self.len == 0 {
                        self.in_text = false;
                    }
                    self.len = 0;
                    return selected;
                }
            }
            Token::Other
                if matches!(bytes[span.start], b'(' | b'<')
                    && crate::pdf_binary_string::visit_string(&bytes[span.clone()], |_| {
                        Some(())
                    })
                    .is_some() =>
            {
                Value::String(span)
            }
            _ => Value::Other,
        };
        self.push(value);
        false
    }

    /// 容量を超えるoperand列を破棄し、可変長の確保を避ける。
    fn push(&mut self, value: Value) {
        if self.len < self.values.len() {
            self.values[self.len] = value;
            self.len += 1;
        } else {
            self.len = 0;
        }
    }
}
