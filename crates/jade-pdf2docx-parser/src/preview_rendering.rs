//! ページのない互換previewで描画モードとq/Q保存を追跡する。
use crate::{decode::DecodeBudget, form_operands::Operands, text_rendering::Mode};

/// streamごとに初期化し、BT/ETを越えて維持する描画状態。
#[derive(Default)]
pub(super) struct State {
    /// 現在の文字描画モード。
    mode: Mode,
    /// qで保存したモードの容量制限付きstack。
    saved: Vec<Mode>,
    /// Trに渡す連続数値operand。
    operands: Operands,
}

impl State {
    /// 数値または演算子を処理し、保存容量・深度不足を通知する。
    pub(super) fn word(&mut self, budget: &DecodeBudget, word: &[u8]) -> Option<()> {
        if let Some(value) = crate::page_attributes::number_value(word) {
            self.operands.push(value);
            return Some(());
        }
        match word {
            b"q" if self.operands.len() == 0 => {
                budget.check_form_graphics_depth(self.saved.len().checked_add(1)?)?;
                crate::metadata_workspace::try_push(budget, &mut self.saved, self.mode)?;
            }
            b"Q" if self.operands.len() == 0 => {
                if let Some(mode) = self.saved.pop() {
                    self.mode = mode;
                }
            }
            b"Tr" if self.operands.len() == 1 => {
                if let Some(mode) = Mode::from_operand(self.operands.as_slice()[0]) {
                    self.mode = mode;
                }
            }
            _ => {}
        }
        self.clear_operands();
        Some(())
    }

    /// 別種のoperandが数値列を分断したことを反映する。
    pub(super) fn clear_operands(&mut self) {
        self.operands.clear();
    }

    /// 現在のモードが文字を描画するか返す。
    pub(super) fn paints(&self) -> bool {
        self.mode.paints()
    }
}
