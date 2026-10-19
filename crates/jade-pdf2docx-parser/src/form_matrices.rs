//! BT開始位置・文字描画中のCTM変更とq/Q退避を累積metadata容量で制限する。
use crate::{
    decode::DecodeBudget,
    metadata_workspace::try_push,
    parser,
    syntax::{Lexer, Token},
    text_draws::{Operand, operand},
};

/// 字句順に追加したCTMの変更点から各文字描画の行列を検索する対応表。
#[derive(Default)]
pub(crate) struct Matrices {
    /// 位置の単調増加順に保存し、全拡張容量を予約するslot。
    entries: Vec<(usize, [f32; 6])>,
}

impl Matrices {
    /// 描画位置以前の最後の変更点を追加の索引なしで二分探索する。
    pub(crate) fn get(&self, offset: &usize) -> Option<&[f32; 6]> {
        let index = self.entries.partition_point(|entry| entry.0 <= *offset);
        let matrix = &self.entries.get(index.checked_sub(1)?)?.1;
        matrix
            .iter()
            .all(|value| value.is_finite())
            .then_some(matrix)
    }

    /// 回帰テストで停止時の部分結果破棄を確認する。
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// BTと文字描画中の変更点にCTMを保存し、停止・容量超過では部分対応を破棄する。
pub(crate) fn text_matrices(budget: &DecodeBudget, content: &[u8], initial: [f32; 6]) -> Matrices {
    if budget.is_stopped() {
        return Matrices::default();
    }
    let mut result = Matrices::default();
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    let mut matrix = initial;
    let mut saved = Vec::new();
    let mut args = [Operand::Other; 6];
    let mut arg_len = 0;
    let mut in_text = false;
    while let Some((token, span)) = lexer.next_spanned() {
        if budget.is_stopped() {
            return Matrices::default();
        }
        let Token::Word(word) = token else {
            let value = operand(&mut lexer, token, span.start);
            if arg_len < args.len() {
                args[arg_len] = value;
                arg_len += 1;
            } else {
                arg_len = 0;
            }
            continue;
        };
        if let Some(value) = crate::page_attributes::number_value(word) {
            if arg_len < args.len() {
                args[arg_len] = Operand::Number(value);
                arg_len += 1;
            } else {
                arg_len = 0;
            }
            continue;
        }
        let mut numbers = [0.; 6];
        let numeric = args[..arg_len]
            .iter()
            .zip(&mut numbers)
            .all(|(arg, number)| {
                if let Operand::Number(value) = arg {
                    *number = *value;
                    true
                } else {
                    false
                }
            });
        let mut changed = false;
        match word {
            b"q" if arg_len == 0 => {
                if budget.check_form_graphics_depth(saved.len() + 1).is_none()
                    || try_push(budget, &mut saved, matrix).is_none()
                {
                    return Matrices::default();
                }
            }
            b"Q" if arg_len == 0 => {
                if let Some(previous) = saved.pop() {
                    matrix = previous;
                    changed = true;
                }
            }
            b"cm" if arg_len == 6 && numeric => {
                matrix = parser::concat_matrix(matrix, numbers);
                changed = true;
            }
            b"BT" if arg_len == 0 => {
                in_text = true;
                if try_push(budget, &mut result.entries, (span.start, matrix)).is_none() {
                    return Matrices::default();
                }
            }
            b"ET" if arg_len == 0 => in_text = false,
            b"BI" if arg_len == 0 => break,
            _ => {}
        }
        if changed
            && in_text
            && result.entries.last().is_none_or(|entry| entry.1 != matrix)
            && try_push(budget, &mut result.entries, (span.start, matrix)).is_none()
        {
            return Matrices::default();
        }
        arg_len = 0;
    }
    result
}

#[cfg(test)]
#[path = "form_matrices_tests.rs"]
mod tests;
