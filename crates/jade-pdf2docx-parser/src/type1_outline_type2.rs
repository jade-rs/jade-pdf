//! 元の外形を、hintやPostScriptを含まない有限のType2 programへ格納する。
use super::charstring::{Command, Glyph};
use crate::decode::DecodeBudget;

/// u16の全advanceをType2の符号付き範囲へ収める、Private DICT共通の基準幅。
pub(super) const NOMINAL_WIDTH: i32 = 32768;

/// 絶対座標を一度だけ固定小数点へ丸め、保存前に全byteを累積予算へ予約する。
pub(super) fn encode(budget: &DecodeBudget, glyph: &Glyph) -> Option<Vec<u8>> {
    let mut size = 0_usize;
    walk(budget, glyph, |bytes| {
        size = size.checked_add(bytes.len())?;
        (size <= 65_535).then_some(())
    })?;
    let mut output = budget.font_vector(size)?;
    walk(budget, glyph, |bytes| {
        output.extend_from_slice(bytes);
        Some(())
    })?;
    (!budget.is_stopped()).then_some(output)
}

/// 数量計測と書込で同じ有限の変換を使い、途中失敗では保存結果を公開しない。
fn walk(
    budget: &DecodeBudget,
    glyph: &Glyph,
    mut emit: impl FnMut(&[u8]) -> Option<()>,
) -> Option<()> {
    if budget.is_stopped() || !glyph.advance.is_finite() || !(0. ..=65535.).contains(&glyph.advance)
    {
        return None;
    }
    let (bytes, len) = number(fixed(glyph.advance - f64::from(NOMINAL_WIDTH))?)?;
    emit(&bytes[..len])?;
    let mut current = [0_i64; 2];
    let mut open = false;
    for command in &glyph.commands {
        if budget.is_stopped() {
            return None;
        }
        match command {
            Command::Move(point) if !open => {
                point_bytes(point, &mut current, &mut emit)?;
                emit(&[21])?;
                open = true;
            }
            Command::Line(point) if open => {
                point_bytes(point, &mut current, &mut emit)?;
                emit(&[5])?;
            }
            Command::Curve(points) if open => {
                for point in points.chunks_exact(2) {
                    point_bytes(point, &mut current, &mut emit)?;
                }
                emit(&[8])?;
            }
            Command::Close if open => open = false,
            _ => return None,
        }
    }
    if open || budget.is_stopped() {
        return None;
    }
    emit(&[14])
}

/// 量子化済み絶対座標との差分を使い、各線分で誤差が累積することを避ける。
fn point_bytes(
    point: &[f64],
    current: &mut [i64; 2],
    emit: &mut impl FnMut(&[u8]) -> Option<()>,
) -> Option<()> {
    for axis in 0..2 {
        if point[axis].abs() > 32767. {
            return None;
        }
        let target = fixed(point[axis])?;
        let (bytes, len) = number(target - current[axis])?;
        emit(&bytes[..len])?;
        current[axis] = target;
    }
    Some(())
}

/// 16.16の表現範囲を先に検査し、キャストの飽和に頼らず最寄り値へ丸める。
fn fixed(value: f64) -> Option<i64> {
    let rounded = (value * 65536.).round();
    (rounded.is_finite() && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&rounded))
        .then_some(rounded as i64)
}

/// Type2整数の三つの短縮表現と、符号付き16.16を区別して格納する。
fn number(value: i64) -> Option<([u8; 5], usize)> {
    let value = i32::try_from(value).ok()?;
    let mut bytes = [0; 5];
    if value & 65535 != 0 {
        bytes[0] = 255;
        bytes[1..].copy_from_slice(&value.to_be_bytes());
        return Some((bytes, 5));
    }
    let integer = value / 65536;
    let len = match integer {
        -107..=107 => {
            bytes[0] = (integer + 139) as u8;
            1
        }
        108..=1131 => {
            let n = integer - 108;
            bytes[0] = (247 + n / 256) as u8;
            bytes[1] = (n % 256) as u8;
            2
        }
        -1131..=-108 => {
            let n = -integer - 108;
            bytes[0] = (251 + n / 256) as u8;
            bytes[1] = (n % 256) as u8;
            2
        }
        _ => {
            bytes[0] = 28;
            bytes[1..3].copy_from_slice(&(integer as i16).to_be_bytes());
            3
        }
    };
    Some((bytes, len))
}

#[cfg(test)]
#[path = "type1_outline_type2_tests.rs"]
mod tests;
