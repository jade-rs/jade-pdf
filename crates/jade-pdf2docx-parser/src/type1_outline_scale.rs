//! PDF側の幅を優先し、フォントの表現範囲内でem単位数と外形をそろえる。
use super::charstring::{Command, Glyph};
use crate::decode::DecodeBudget;

/// 元の1000単位系で集計した、変換時の最大絶対値。
pub(super) struct Limits {
    /// 外形の座標上限。
    pub(super) coordinate: f64,
    /// Type2で格納する連続点の差分上限。
    pub(super) delta: f64,
    /// header等の符号付きmetricの上限。
    pub(super) metric: f64,
    /// PDF幅を持たないglyphも含むadvance上限。
    pub(super) advance: f64,
}

/// 実際のPDF幅の最大量子化誤差を最小化し、同等なら細かい単位を選ぶ。
pub(super) fn choose_units(budget: &DecodeBudget, widths: &[f64], limits: &Limits) -> Option<u16> {
    if widths.len() > 256 || budget.is_stopped() {
        return None;
    }
    let mut advance = limits.advance;
    for &width in widths {
        if !width.is_finite() || width < 0. {
            return None;
        }
        advance = advance.max(width);
    }
    let mut upper = 16384_f64;
    for (value, maximum) in [
        (limits.coordinate, 32767.),
        (limits.delta, 32767.),
        (limits.metric, 32767.),
        (advance, 65535.),
    ] {
        if !value.is_finite() || value < 0. {
            return None;
        }
        if value > 0. {
            upper = upper.min((maximum * 1000. / value).floor());
        }
    }
    if upper < 16. {
        return None;
    }
    let upper = upper as u16;
    if widths.is_empty() {
        return Some(upper.min(1000));
    }
    let mut best = None;
    let mut best_error = f64::INFINITY;
    for units in (16..=upper).rev() {
        if budget.is_stopped() {
            return None;
        }
        let scale = f64::from(units) / 1000.;
        let mut error = 0_f64;
        for &width in widths {
            let rounded = (width * scale + 0.5).floor();
            error = error.max((rounded / scale - width).abs());
            if error > best_error + 1e-9 {
                break;
            }
        }
        if error + 1e-9 < best_error {
            best = Some(units);
            best_error = error;
        }
    }
    if budget.is_stopped() { None } else { best }
}

/// 確認済みPDF幅があればfont内advanceより優先し、外形と一緒に単位を変換する。
pub(super) fn prepare(
    budget: &DecodeBudget,
    glyph: &Glyph,
    pdf_width: Option<f64>,
    units: u16,
) -> Option<Glyph> {
    if !(16..=16384).contains(&units) || glyph.commands.len() > 100_000 || budget.is_stopped() {
        return None;
    }
    let width = pdf_width.unwrap_or(glyph.advance);
    if !width.is_finite() || width < 0. {
        return None;
    }
    let scale = f64::from(units) / 1000.;
    let advance = (width * scale + 0.5).floor();
    if !advance.is_finite() || !(0. ..=65535.).contains(&advance) {
        return None;
    }
    let mut commands = budget.font_vector(glyph.commands.len())?;
    for command in &glyph.commands {
        if budget.is_stopped() {
            return None;
        }
        let converted = match command {
            Command::Move(point) => {
                Command::Move([coordinate(point[0], scale)?, coordinate(point[1], scale)?])
            }
            Command::Line(point) => {
                Command::Line([coordinate(point[0], scale)?, coordinate(point[1], scale)?])
            }
            Command::Curve(points) => {
                let mut output = [0.; 6];
                for (index, &value) in points.iter().enumerate() {
                    output[index] = coordinate(value, scale)?;
                }
                Command::Curve(output)
            }
            Command::Close => Command::Close,
        };
        commands.push(converted);
    }
    (!budget.is_stopped()).then_some(Glyph { advance, commands })
}

/// 座標を比率で変換し、非有限値やheaderへ収まらない範囲を飽和させない。
fn coordinate(value: f64, scale: f64) -> Option<f64> {
    let output = value * scale;
    (output.is_finite() && output.abs() <= 32767.).then_some(output)
}

#[cfg(test)]
#[path = "type1_outline_scale_tests.rs"]
mod tests;
