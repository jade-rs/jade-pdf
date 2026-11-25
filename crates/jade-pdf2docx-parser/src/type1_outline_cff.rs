//! 元の文字名・外形・noticeを、実行コードのないCFF1 fontへまとめる。
use crate::decode::DecodeBudget;
#[path = "type1_outline_cff_dict.rs"]
mod dict;
#[path = "type1_outline_cff_strings.rs"]
mod strings;

/// 文字名と、完成済みのType2外形。
pub(super) struct Entry<'a> {
    /// 元のglyph名。先頭は必ず.notdefとする。
    pub(super) name: &'a [u8],
    /// 共有のnominalWidthを使う、完成したType2 program。
    pub(super) program: &'a [u8],
}

/// 許諾確認済みfontの出力用情報。元のPDF font名とは分離する。
pub(super) struct Font<'a> {
    /// OpenTypeのem当たり単位数。FontMatrixにも同じ値を適用する。
    pub(super) units_per_em: u16,
    /// reserved nameを使わずに呼出元で決めたPostScript名。
    pub(super) name: &'a [u8],
    /// 元の著作権と許諾情報を省略せずに渡す。
    pub(super) notice: &'a [u8],
    /// units_per_emと同じ座標単位へ変換した外形範囲。
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
    if !(16..=16384).contains(&font.units_per_em)
        || !valid_name(font.name, 63)
        || font.notice.len() > 65_535
        || !font.italic_angle.is_finite()
        || font.italic_angle.abs() > 32767.
        || font.bbox[0] > font.bbox[2]
        || font.bbox[1] > font.bbox[3]
        || font.glyphs.is_empty()
        || font.glyphs.len() > 4096
        || font.glyphs[0].name != b".notdef"
    {
        return None;
    }
    let mut pool = budget.font_vector(font.glyphs.len() + 2)?;
    let notice = intern(budget, &mut pool, font.notice)?;
    let name = intern(budget, &mut pool, font.name)?;
    let mut ids = budget.font_vector(font.glyphs.len())?;
    let mut programs = budget.font_vector(font.glyphs.len())?;
    for glyph in font.glyphs {
        if !valid_name(glyph.name, 127)
            || glyph.program.len() > 65_535
            || glyph.program.last() != Some(&14)
        {
            return None;
        }
        let id = intern(budget, &mut pool, glyph.name)?;
        if ids.contains(&id) {
            return None;
        }
        ids.push(id);
        programs.push(glyph.program);
    }
    let names = super::cff_index::encode(budget, &[font.name])?;
    let custom_strings = super::cff_index::encode(budget, &pool)?;
    let charstrings = super::cff_index::encode(budget, &programs)?;
    let mut charset = budget.font_vector(1 + 2 * (font.glyphs.len() - 1))?;
    charset.push(0);
    for id in &ids[1..] {
        if budget.is_stopped() {
            return None;
        }
        charset.extend_from_slice(&id.to_be_bytes());
    }
    let placeholder = dict::top(font, notice, name, 0, 0, 0)?;
    if placeholder.as_slice().len() > 254 {
        return None;
    }
    let charset_offset = 4_usize
        .checked_add(names.len())?
        .checked_add(5 + placeholder.as_slice().len())?
        .checked_add(custom_strings.len())?
        .checked_add(2)?;
    let charstrings_offset = charset_offset.checked_add(charset.len())?;
    let private_offset = charstrings_offset.checked_add(charstrings.len())?;
    let total = private_offset.checked_add(dict::PRIVATE.len())?;
    if total > crate::parser::MAX_EMBEDDED_FONT_BYTES {
        return None;
    }
    let top = dict::top(
        font,
        notice,
        name,
        charset_offset,
        charstrings_offset,
        private_offset,
    )?;
    if top.as_slice().len() != placeholder.as_slice().len() {
        return None;
    }
    let top_index = super::cff_index::encode(budget, &[top.as_slice()])?;
    let mut output = budget.font_vector(total)?;
    for bytes in [
        [1, 0, 4, 4].as_slice(),
        &names,
        &top_index,
        &custom_strings,
        &[0, 0],
        &charset,
        &charstrings,
        dict::PRIVATE,
    ] {
        if budget.is_stopped() {
            return None;
        }
        output.extend_from_slice(bytes);
    }
    (!budget.is_stopped()).then_some(output)
}

/// Adobe CFFの標準391文字列を、固定SIDで参照する。
fn standard_sid(name: &[u8]) -> Option<u16> {
    strings::STANDARD
        .iter()
        .position(|s| *s == name)
        .and_then(|index| u16::try_from(index).ok())
}

/// 独自文字列も一度だけ保存し、SIDと文字列の対応を変更しない。
fn intern<'a>(budget: &DecodeBudget, pool: &mut Vec<&'a [u8]>, value: &'a [u8]) -> Option<u16> {
    if budget.is_stopped() {
        return None;
    }
    if let Some(id) = standard_sid(value) {
        return Some(id);
    }
    if let Some(index) = pool.iter().position(|s| *s == value) {
        return u16::try_from(391 + index).ok();
    }
    if pool.len() == pool.capacity() {
        return None;
    }
    let id = u16::try_from(391 + pool.len()).ok()?;
    pool.push(value);
    Some(id)
}

/// PostScriptのdelimiterと制御文字を避け、固定上限のASCII名だけを使う。
fn valid_name(name: &[u8], max: usize) -> bool {
    !name.is_empty()
        && name.len() <= max
        && name
            .iter()
            .all(|b| (33..=126).contains(b) && !b"[](){}<>/%".contains(b))
}

#[cfg(test)]
#[path = "type1_outline_cff_tests.rs"]
mod tests;
