//! Type1のcleartext辞書から静的な座標系・属性・元の文字列だけを読む。
use crate::{decode::DecodeBudget, type1_program::Encoding};

/// 元のType1辞書を借用する情報。権利判断や派生名の生成は呼出元の責務。
pub(super) struct Header<'a> {
    /// 原本のPostScript名。派生fontのfamily名へそのまま使わない。
    pub(super) name: &'a [u8],
    /// 原本のFontMatrix六要素。
    pub(super) matrix: [f64; 6],
    /// 原本で宣言したFontBBox。
    pub(super) bbox: [f64; 4],
    /// 原本の静的なEncoding。
    pub(super) encoding: Encoding<'a>,
    /// FontInfoの情報。
    pub(super) info: Info<'a>,
}

/// 指定の有無を保つFontInfo。文字列は括弧を除いた未展開のPostScript byte列。
#[derive(Default)]
pub(super) struct Info<'a> {
    /// 元のNotice文字列。octal escape等は別途復号する。
    pub(super) notice: Option<&'a [u8]>,
    /// 元のCopyright文字列。
    pub(super) copyright: Option<&'a [u8]>,
    /// 元のversion文字列。
    pub(super) version: Option<&'a [u8]>,
    /// 元のFullName文字列。
    pub(super) full_name: Option<&'a [u8]>,
    /// 元のFamilyName文字列。
    pub(super) family: Option<&'a [u8]>,
    /// 元のWeight文字列。
    pub(super) weight: Option<&'a [u8]>,
    /// 元のItalicAngle。未指定はNone。
    pub(super) italic_angle: Option<f64>,
    /// 元の等幅指定。未指定はNone。
    pub(super) fixed: Option<bool>,
    /// 元の下線位置。
    pub(super) underline_position: Option<f64>,
    /// 元の下線の太さ。
    pub(super) underline_thickness: Option<f64>,
    /// 元のFSType。Noneを埋め込み許可の0へ置き換えない。
    pub(super) fs_type: Option<u16>,
    /// TeX系FontInfoのascent。PDF descriptorとの優先順位は呼出元で決める。
    pub(super) ascent: Option<f64>,
    /// TeX系FontInfoのdescent。
    pub(super) descent: Option<f64>,
}

/// PostScriptを実行せず、有限の静的定義と暗号化境界を確認する。
pub(super) fn parse<'a>(_budget: &DecodeBudget, _bytes: &'a [u8]) -> Option<Header<'a>> {
    None
}

#[cfg(test)]
#[path = "type1_outline_header_tests.rs"]
mod tests;
