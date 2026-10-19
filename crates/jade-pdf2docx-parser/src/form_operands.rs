//! CTMの最後の6数値を、追加allocationなしで保持する。

/// 数値の連続列から最後の6件だけを保存する固定容量operand。
#[derive(Default)]
pub(crate) struct Operands {
    /// CTMに必要な最大6数値の保存領域。
    values: [f32; 6],
    /// 有効な数値の件数。
    len: usize,
}

impl Operands {
    /// 満杯では最古の数値を除き、最新の数値を追加する。
    pub(crate) fn push(&mut self, value: f32) {
        if self.len == self.values.len() {
            self.values.copy_within(1.., 0);
            self.len -= 1;
        }
        self.values[self.len] = value;
        self.len += 1;
    }

    /// 演算子・別のoperandで連続数値の状態を解除する。
    pub(crate) fn clear(&mut self) {
        self.len = 0;
    }

    /// 有効な件数を返す。
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// 有効な数値だけを借用する。
    pub(crate) fn as_slice(&self) -> &[f32] {
        &self.values[..self.len]
    }
}

#[cfg(test)]
#[path = "form_operands_tests.rs"]
mod tests;
