//! 確認済みCFF外形とmetricを、必要な九tableを持つOpenTypeへまとめる。
use super::{cff, cmap};
use crate::decode::DecodeBudget;
#[path = "type1_outline_opentype_names.rs"]
mod names;
#[path = "type1_outline_opentype_tables.rs"]
mod tables;

/// GIDに対応する水平metricと、輪郭を外側へ丸めた範囲。
pub(super) struct Metric {
    /// 元のadvanceをOpenType単位へ格納した値。
    pub(super) advance: u16,
    /// 実際の外形に対応する左sidebearing。
    pub(super) left: i16,
    /// 空glyphはNone。輪郭のあるglyphだけをfont全体の範囲へ数える。
    pub(super) bounds: Option<[i16; 4]>,
}

/// 元のfontと許諾判定から得た、明示的な出力情報。
pub(super) struct Info<'a> {
    /// CFFのGID順に対応するmetric。
    pub(super) metrics: &'a [Metric],
    /// 事前に確認したUnicodeとGIDの対応。
    pub(super) mappings: &'a [cmap::Mapping],
    /// 元のtypographic ascent。
    pub(super) ascent: i16,
    /// 元のtypographic descent。
    pub(super) descent: i16,
    /// 元のtypographic line gap。
    pub(super) line_gap: i16,
    /// 元のweight class。
    pub(super) weight: u16,
    /// 外形自体が太字であることを保持する。
    pub(super) bold: bool,
    /// 外形自体の斜体属性を保持する。
    pub(super) italic: bool,
    /// 元の許可bit。変更・埋め込み可能なことは呼出元で事前判定する。
    pub(super) embedding_flags: u16,
    /// 元の下線位置。
    pub(super) underline_position: i16,
    /// 元の下線の太さ。
    pub(super) underline_thickness: i16,
    /// 元のx-height。
    pub(super) x_height: i16,
    /// 元のcap height。
    pub(super) cap_height: i16,
    /// 元の著作権と許諾全文を省略せずに渡す。
    pub(super) license: &'a str,
    /// 元の許諾を確認できるURL。
    pub(super) license_url: &'a str,
}

/// 各metricと許可属性を検査し、再現可能なOTTO fontを生成する。
pub(super) fn encode(
    budget: &DecodeBudget,
    font: &cff::Font<'_>,
    info: &Info<'_>,
) -> Option<Vec<u8>> {
    let flags = info.embedding_flags;
    if info.metrics.len() != font.glyphs.len()
        || info.metrics.is_empty()
        || info.metrics.len() > 4096
        || info.weight == 0
        || info.weight > 1000
        || info.ascent < 0
        || info.descent > 0
        || info.ascent <= info.descent
        || info.line_gap < 0
        || info.underline_thickness < 0
        || info.x_height < 0
        || info.cap_height < 0
        || flags & !0x010c != 0
        || !matches!(flags & 15, 0 | 4 | 8)
        || budget.is_stopped()
    {
        return None;
    }
    let summary = tables::summarize(info)?;
    let cff = cff::encode(budget, font)?;
    let cmap = super::cmap::encode(
        budget,
        info.mappings,
        u16::try_from(info.metrics.len()).ok()?,
    )?;
    let name = names::build(budget, font, info)?;
    let head = tables::head(info, &summary, font.units_per_em);
    let hhea = tables::hhea(info, &summary)?;
    let hmtx = tables::hmtx(budget, info)?;
    let os2 = tables::os2(budget, info, &summary, font.units_per_em)?;
    let post = tables::post(font, info)?;
    let mut maxp = [0, 0, 80, 0, 0, 0];
    maxp[4..].copy_from_slice(&u16::try_from(info.metrics.len()).ok()?.to_be_bytes());
    let data = [
        (*b"CFF ", cff.as_slice()),
        (*b"OS/2", &os2),
        (*b"cmap", &cmap),
        (*b"head", &head),
        (*b"hhea", &hhea),
        (*b"hmtx", &hmtx),
        (*b"maxp", &maxp),
        (*b"name", &name),
        (*b"post", &post),
    ];
    let tables = data.map(|(tag, bytes)| super::sfnt::Table { tag, bytes });
    super::sfnt::pack(budget, &tables)
}

/// 原本cluster専用の合字規則を含むfontを作る。通常runへの適用判断は呼出元で行う。
pub(super) fn encode_with_ligatures(
    budget: &DecodeBudget,
    font: &cff::Font<'_>,
    info: &Info<'_>,
    _rules: &[super::gsub::Rule<'_>],
) -> Option<Vec<u8>> {
    encode(budget, font, info)
}

#[cfg(test)]
#[path = "type1_outline_opentype_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "type1_outline_opentype_ligature_tests.rs"]
mod ligature_tests;
