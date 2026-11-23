//! 元の文字名・外形・noticeを、実行コードのないCFF1 fontへまとめる。
use crate::decode::DecodeBudget;

/// 文字名と、完成済みのType2外形。
pub(super) struct Entry<'a> {
    /// 元のglyph名。先頭は必ず.notdefとする。
    pub(super) name: &'a [u8],
    /// 共有のnominalWidthを使う、完成したType2 program。
    pub(super) program: &'a [u8],
}

/// 許諾確認済みfontの出力用情報。元のPDF font名とは分離する。
pub(super) struct Font<'a> {
    /// reserved nameを使わずに呼出元で決めたPostScript名。
    pub(super) name: &'a [u8],
    /// 元の著作権と許諾情報を省略せずに渡す。
    pub(super) notice: &'a [u8],
    /// 1000単位での元の外形範囲。
    pub(super) bbox: [i16; 4],
    /// 元の傾斜角度。
    pub(super) italic_angle: f64,
    /// 元の等幅情報。
    pub(super) is_fixed_pitch: bool,
    /// GID順のglyph。元の名前とprogramの対応を変えない。
    pub(super) glyphs: &'a [Entry<'a>],
}

/// 名前・辞書・charset・文字programを、8MiB内のCFF1へ格納する。
pub(super) fn encode(budget: &DecodeBudget, font: &Font<'_>) -> Option<Vec<u8>> {
    let _ = (budget, font);
    None
}

/// Adobe CFFの標準391文字列を、固定SIDで参照する。
fn standard_sid(name: &[u8]) -> Option<u16> {
    let _ = name;
    None
}

#[cfg(test)]
#[path = "type1_outline_cff_tests.rs"]
mod tests;
