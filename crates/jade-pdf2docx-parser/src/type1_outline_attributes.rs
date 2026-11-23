//! 元FontInfoの外形属性とPDF descriptorの行metricを、元の1000単位で保持する。
use super::{bounds, cmap, file::Font, opentype, permission::License, resource::values};
use crate::{decode::DecodeBudget, resource_resolver::ResourceResolver};

/// 丸める前の元metricと、glyph外形自身のstyle属性。
pub(super) struct Attributes {
    /// PDF descriptorを優先する、baseline上の高さ。
    pub(super) ascent: f64,
    /// PDF descriptorを優先する、baseline下の負の値。
    pub(super) descent: f64,
    /// 確認済みweight class。通常400と明示Medium500を区別する。
    pub(super) weight: u16,
    /// 外形自身がboldのweightを持つか。
    pub(super) bold: bool,
    /// 外形自身の傾斜属性。
    pub(super) italic: bool,
    /// 元FontInfoを優先した傾斜角。
    pub(super) italic_angle: f64,
    /// 元FontInfoの指定を優先する等幅属性。
    pub(super) fixed: bool,
    /// 元下線位置。未確認は0。
    pub(super) underline_position: f64,
    /// 元下線厚さ。未確認は0。
    pub(super) underline_thickness: f64,
    /// 元小文字高。未確認は0。
    pub(super) x_height: f64,
    /// 元大文字高。未確認は0。
    pub(super) cap_height: f64,
}

/// 元font名からstyleを推測せず、確認できる元の指定と外形を読む。
pub(super) fn read<'bytes>(
    budget: &DecodeBudget,
    font: &Font<'_>,
    descriptor: &'bytes [u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<Attributes> {
    if budget.is_stopped() || font.header.matrix != [0.001, 0., 0., 0.001, 0., 0.] {
        return None;
    }
    let mut number = |key| numeric(budget, descriptor, key, resolver);
    let pdf_ascent = number(b"Ascent")?;
    let pdf_descent = number(b"Descent")?;
    let x_height = number(b"XHeight")?;
    let cap_height = number(b"CapHeight")?;
    let pdf_angle = number(b"ItalicAngle")?;
    let pdf_weight = number(b"FontWeight")?;
    if pdf_weight.is_some_and(|w| w.fract() != 0. || !(1. ..=1000.).contains(&w)) {
        return None;
    }
    let flags = number(b"Flags")?.unwrap_or(0.);
    if flags.fract() != 0. || !(0. ..=f64::from(u32::MAX)).contains(&flags) {
        return None;
    }
    let flags = flags as u32;
    let info = &font.header.info;
    let weight = weight(budget, info.weight)?
        .or(pdf_weight.map(|w| w as u16))
        .unwrap_or(400);
    let angle = info.italic_angle.or(pdf_angle);
    let italic_angle = angle.unwrap_or(0.);
    let mut ascent = pdf_ascent.or(info.ascent);
    let mut descent = pdf_descent.or(info.descent);
    if ascent.is_none() || descent.is_none() {
        let (mut top, mut bottom) = (0_f64, 0_f64);
        for glyph in &font.glyphs {
            if let Some(bounds) = bounds::source(budget, &glyph.outline)? {
                top = top.max(bounds[3]);
                bottom = bottom.min(bounds[1]);
            }
        }
        ascent = ascent.or(Some(top));
        descent = descent.or(Some(bottom));
    }
    let attributes = Attributes {
        ascent: ascent?,
        descent: descent?,
        weight,
        bold: weight >= 700,
        italic: angle.map_or(flags & 64 != 0, |a| a != 0.),
        italic_angle,
        fixed: info.fixed.unwrap_or(flags & 1 != 0),
        underline_position: info.underline_position.unwrap_or(0.),
        underline_thickness: info.underline_thickness.unwrap_or(0.),
        x_height: match x_height {
            Some(value) => value,
            None => glyph_top(budget, font, b"x")?,
        },
        cap_height: match cap_height {
            Some(value) => value,
            None => glyph_top(budget, font, b"H")?,
        },
    };
    if budget.is_stopped() || !attributes.valid() {
        None
    } else {
        Some(attributes)
    }
}

impl Attributes {
    /// 元単位から一度だけ丸め、metric・cmap・完全な元許諾を同じ出力へ結び付ける。
    pub(super) fn to_info<'a>(
        &self,
        units: u16,
        metrics: &'a [opentype::Metric],
        mappings: &'a [cmap::Mapping],
        license: &'a License,
    ) -> Option<opentype::Info<'a>> {
        if !(16..=16384).contains(&units) || !self.valid() {
            return None;
        }
        Some(opentype::Info {
            metrics,
            mappings,
            ascent: scaled(self.ascent, units)?,
            descent: scaled(self.descent, units)?,
            line_gap: 0,
            weight: self.weight,
            bold: self.bold,
            italic: self.italic,
            embedding_flags: license.flags,
            underline_position: scaled(self.underline_position, units)?,
            underline_thickness: scaled(self.underline_thickness, units)?,
            x_height: scaled(self.x_height, units)?,
            cap_height: scaled(self.cap_height, units)?,
            license: &license.text,
            license_url: license.url,
        })
    }

    /// 無限値・符号の反転や不正なline metricを飽和や絶対値で隠さない。
    fn valid(&self) -> bool {
        [
            self.ascent,
            self.descent,
            self.italic_angle,
            self.underline_position,
            self.underline_thickness,
            self.x_height,
            self.cap_height,
        ]
        .iter()
        .all(|v| v.is_finite())
            && self.ascent >= 0.
            && self.descent <= 0.
            && self.ascent > self.descent
            && self.underline_thickness >= 0.
            && self.x_height >= 0.
            && self.cap_height >= 0.
    }
}

/// null・未指定と破損参照を区別して、単一の有限数値だけを読む。
fn numeric<'bytes>(
    budget: &DecodeBudget,
    descriptor: &'bytes [u8],
    key: &[u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<Option<f64>> {
    match values::field(budget, descriptor, key, resolver)? {
        Some(raw) => Some(Some(values::number(raw)?)),
        None => Some(None),
    }
}

/// 実在する確認済みglyphの高さだけを補完し、欠落glyphから値を作らない。
fn glyph_top(budget: &DecodeBudget, font: &Font<'_>, name: &[u8]) -> Option<f64> {
    match font.glyphs.iter().find(|g| g.name == name) {
        Some(g) => Some(bounds::source(budget, &g.outline)?.map_or(0., |b| b[3].max(0.))),
        None => Some(0.),
    }
}

/// 既知の宣言名だけをOpenType classへ対応させ、未知の部分文字列をBoldと解釈しない。
fn weight(budget: &DecodeBudget, raw: Option<&[u8]>) -> Option<Option<u16>> {
    let Some(raw) = raw else {
        return Some(None);
    };
    let value = super::ps_string::decode(budget, raw)?;
    let names: [(&[u8], u16); 21] = [
        (b"Thin", 100),
        (b"ExtraLight", 200),
        (b"Extra-Light", 200),
        (b"UltraLight", 200),
        (b"Light", 300),
        (b"Book", 400),
        (b"Regular", 400),
        (b"Normal", 400),
        (b"Roman", 400),
        (b"Medium", 500),
        (b"SemiBold", 600),
        (b"Semi-Bold", 600),
        (b"Demi", 600),
        (b"DemiBold", 600),
        (b"Bold", 700),
        (b"ExtraBold", 800),
        (b"Extra-Bold", 800),
        (b"UltraBold", 800),
        (b"Black", 900),
        (b"Heavy", 900),
        (b"ExtraBlack", 950),
    ];
    Some(
        names
            .iter()
            .find(|(name, _)| value.trim_ascii().eq_ignore_ascii_case(name))
            .map(|(_, weight)| *weight),
    )
}

/// 選んだem単位へ最後に丸め、i16範囲外を切り詰めず拒否する。
fn scaled(value: f64, units: u16) -> Option<i16> {
    let value = (value * f64::from(units) / 1000.).round();
    (value.is_finite() && (f64::from(i16::MIN)..=f64::from(i16::MAX)).contains(&value))
        .then_some(value as i16)
}

#[cfg(test)]
#[path = "type1_outline_attributes_tests.rs"]
mod tests;
