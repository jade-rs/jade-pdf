//! OpenTypeの固定長metric tableを、明示的な元属性から作る。
use super::{Info, cff};
use crate::decode::DecodeBudget;
#[path = "type1_outline_unicode_ranges.rs"]
mod unicode;

/// headとhheaへ渡す、輪郭glyphだけの集計。
pub(super) struct Summary {
    /// font全体の実際の輪郭範囲。空fontでは0。
    pub(super) bbox: [i16; 4],
    /// 全glyphの最大advance。
    pub(super) max_advance: u16,
    /// 輪郭glyphの最小左sidebearing。
    pub(super) min_left: i16,
    /// 輪郭glyphの最小右sidebearing。
    pub(super) min_right: i16,
    /// 輪郭glyphの最大水平extent。
    pub(super) max_extent: i16,
    /// 非zero advanceの平均。FWORDへ収まらなければunknownの0。
    pub(super) average: i16,
}

/// 空glyphを輪郭範囲から除外し、FWORDを越えるmetricは拒否する。
pub(super) fn summarize(info: &Info<'_>) -> Option<Summary> {
    let mut bbox = None::<[i16; 4]>;
    let mut left = None;
    let mut right = None;
    let mut extent = None;
    let mut max = 0;
    let mut sum = 0_u64;
    let mut count = 0;
    for metric in info.metrics {
        max = max.max(metric.advance);
        if metric.advance != 0 {
            sum += u64::from(metric.advance);
            count += 1;
        }
        if let Some(b) = metric.bounds {
            if b[0] > b[2] || b[1] > b[3] {
                return None;
            }
            bbox = Some(match bbox {
                None => b,
                Some(old) => [
                    old[0].min(b[0]),
                    old[1].min(b[1]),
                    old[2].max(b[2]),
                    old[3].max(b[3]),
                ],
            });
            let width = i32::from(b[2]) - i32::from(b[0]);
            let r =
                i16::try_from(i32::from(metric.advance) - i32::from(metric.left) - width).ok()?;
            let e = i16::try_from(i32::from(metric.left) + width).ok()?;
            left = Some(left.map_or(metric.left, |old: i16| old.min(metric.left)));
            right = Some(right.map_or(r, |old: i16| old.min(r)));
            extent = Some(extent.map_or(e, |old: i16| old.max(e)));
        } else if metric.left != 0 {
            return None;
        }
    }
    let average = if count == 0 {
        0
    } else {
        i16::try_from((sum + count / 2) / count).unwrap_or(0)
    };
    Some(Summary {
        bbox: bbox.unwrap_or([0; 4]),
        max_advance: max,
        min_left: left.unwrap_or(0),
        min_right: right.unwrap_or(0),
        max_extent: extent.unwrap_or(0),
        average,
    })
}

/// 固定長tableの16bit領域へ符号bitを保持して書く。
fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
}
/// 固定長tableの32bit領域へ書く。
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

/// fontの実輪郭と書体属性からheadを作る。時刻は再現可能なUnix epochに固定する。
pub(super) fn head(info: &Info<'_>, summary: &Summary) -> [u8; 54] {
    let mut bytes = [0; 54];
    put32(&mut bytes, 0, 0x10000);
    put32(&mut bytes, 4, 0x10000);
    put32(&mut bytes, 12, 0x5f0f3cf5);
    put16(&mut bytes, 16, 1);
    put16(&mut bytes, 18, 1000);
    for offset in [20, 28] {
        bytes[offset..offset + 8].copy_from_slice(&2082844800_i64.to_be_bytes());
    }
    for (index, value) in summary.bbox.iter().enumerate() {
        put16(&mut bytes, 36 + 2 * index, *value as u16);
    }
    put16(
        &mut bytes,
        44,
        u16::from(info.bold) + 2 * u16::from(info.italic),
    );
    put16(&mut bytes, 46, 3);
    put16(&mut bytes, 48, 2);
    bytes
}

/// typographic line metricと、実輪郭を持つglyphの水平集計を保持する。
pub(super) fn hhea(info: &Info<'_>, summary: &Summary) -> Option<[u8; 36]> {
    let mut bytes = [0; 36];
    put32(&mut bytes, 0, 0x10000);
    for (offset, value) in [
        (4, info.ascent),
        (6, info.descent),
        (8, info.line_gap),
        (12, summary.min_left),
        (14, summary.min_right),
        (16, summary.max_extent),
    ] {
        put16(&mut bytes, offset, value as u16);
    }
    put16(&mut bytes, 10, summary.max_advance);
    put16(&mut bytes, 18, 1);
    put16(&mut bytes, 34, u16::try_from(info.metrics.len()).ok()?);
    Some(bytes)
}

/// 全GIDの水平metricを省略せず、確保前に保存量を予約する。
pub(super) fn hmtx(budget: &DecodeBudget, info: &Info<'_>) -> Option<Vec<u8>> {
    let mut bytes = budget.font_vector(info.metrics.len().checked_mul(4)?)?;
    for metric in info.metrics {
        if budget.is_stopped() {
            return None;
        }
        bytes.extend_from_slice(&metric.advance.to_be_bytes());
        bytes.extend_from_slice(&metric.left.to_be_bytes());
    }
    (!budget.is_stopped()).then_some(bytes)
}

/// CFF1では必須のpost3.0を使い、元の角度・下線・等幅情報を保つ。
pub(super) fn post(font: &cff::Font<'_>, info: &Info<'_>) -> Option<[u8; 32]> {
    let fixed = (font.italic_angle * 65536.).round();
    if !fixed.is_finite() || fixed < f64::from(i32::MIN) || fixed > f64::from(i32::MAX) {
        return None;
    }
    let mut bytes = [0; 32];
    put32(&mut bytes, 0, 0x30000);
    put32(&mut bytes, 4, fixed as i32 as u32);
    put16(&mut bytes, 8, info.underline_position as u16);
    put16(&mut bytes, 10, info.underline_thickness as u16);
    put32(&mut bytes, 12, u32::from(font.is_fixed_pitch));
    Some(bytes)
}

/// OS/2 version4に元の許可・書体・line metricと、実cmapのUnicode coverageを保存する。
pub(super) fn os2(budget: &DecodeBudget, info: &Info<'_>, summary: &Summary) -> Option<[u8; 96]> {
    let mut bytes = [0; 96];
    for (offset, value) in [
        (0, 4),
        (2, summary.average as u16),
        (4, info.weight),
        (6, 5),
        (8, info.embedding_flags),
        (10, 650),
        (12, 600),
        (16, 75),
        (18, 650),
        (20, 600),
        (24, 350),
        (26, 50),
        (28, 300),
    ] {
        put16(&mut bytes, offset, value);
    }
    let mut ranges = [0_u32; 4];
    let mut first = 65535;
    let mut last = 0;
    for mapping in info.mappings {
        if budget.is_stopped() {
            return None;
        }
        first = first.min(mapping.scalar.min(65535));
        last = last.max(mapping.scalar.min(65535));
        if mapping.scalar >= 65536 {
            ranges[1] |= 1 << 25;
        }
        for &(bit, start, end) in unicode::RANGES {
            if (start..=end).contains(&mapping.scalar) {
                ranges[usize::from(bit / 32)] |= 1 << (bit % 32);
            }
        }
    }
    for (index, value) in ranges.iter().enumerate() {
        put32(&mut bytes, 42 + 4 * index, *value);
    }
    bytes[58..62].fill(b' ');
    let selection = 128
        + 32 * u16::from(info.bold)
        + u16::from(info.italic)
        + if !info.bold && !info.italic { 64 } else { 0 };
    put16(&mut bytes, 62, selection);
    put16(&mut bytes, 64, first as u16);
    put16(&mut bytes, 66, last as u16);
    for (offset, value) in [
        (68, info.ascent),
        (70, info.descent),
        (72, info.line_gap),
        (86, info.x_height),
        (88, info.cap_height),
    ] {
        put16(&mut bytes, offset, value as u16);
    }
    let ascent = i32::from(info.ascent)
        .max(i32::from(summary.bbox[3]))
        .max(0);
    let descent = (-i32::from(info.descent))
        .max(-i32::from(summary.bbox[1]))
        .max(0);
    put16(&mut bytes, 74, u16::try_from(ascent).ok()?);
    put16(&mut bytes, 76, u16::try_from(descent).ok()?);
    put16(
        &mut bytes,
        92,
        if info.mappings.iter().any(|m| m.scalar == 32) {
            32
        } else {
            0
        },
    );
    put16(&mut bytes, 94, 1);
    (!budget.is_stopped()).then_some(bytes)
}
