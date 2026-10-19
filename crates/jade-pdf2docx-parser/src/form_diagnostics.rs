//! 描画されたFontの診断集合と出力slotをmetadataの累積容量で制限する。
use crate::{
    QualityIssue, QualityReport,
    decode::DecodeBudget,
    font_diagnostics::Selection,
    metadata_workspace::{Set, try_push},
};

/// ページ/Fontごとの診断を検出順に一度だけ保存する。
#[derive(Default)]
pub(crate) struct Fonts {
    /// 拡張前に全slot容量を予約する診断済み集合。
    seen: Set<(usize, Selection)>,
}

impl Fonts {
    /// 停止・集合/出力容量超過では新しい診断を公開せず、エラーを予算に保持する。
    pub(crate) fn record(
        &mut self,
        budget: &DecodeBudget,
        report: &mut QualityReport,
        page: usize,
        selection: Selection,
    ) -> Option<()> {
        if self.seen.try_insert(budget, (page, selection))? {
            try_push(
                budget,
                &mut report.issues,
                QualityIssue::UnsupportedFont {
                    page,
                    object_number: selection.object_number,
                    reason: selection.reason,
                },
            )?;
        }
        Some(())
    }
}

#[cfg(test)]
#[path = "form_diagnostics_tests.rs"]
mod tests;
