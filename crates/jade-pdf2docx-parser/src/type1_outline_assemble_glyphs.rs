//! 原本外形とcode別幅から単位数を選び、全原本glyphと幅違いを一度ずつ生成する。
use crate::{
    decode::DecodeBudget,
    type1_outline::{
        attributes::Attributes, bounds, charstring::Command, file::Font, opentype::Metric,
        plan::Plan, scale, type2,
    },
};

/// 複数の出力faceで同じ外形・metricを共有する生成結果。
pub(super) struct Data {
    /// PDF幅の最大誤差を最小化するem単位数。
    pub(super) units: u16,
    /// 原本名を保ち、幅違いだけ衝突しない別名を持つglyph名。
    pub(super) names: Vec<Vec<u8>>,
    /// GID順の完成Type2 program。
    pub(super) programs: Vec<Vec<u8>>,
    /// 保存された外形とadvanceに対応するmetric。
    pub(super) metrics: Vec<Metric>,
    /// 保存済み全外形を包む整数範囲。
    pub(super) bbox: [i16; 4],
}

/// 元resourceの全幅と実外形を見て単位数を選び、未変更の輪郭順を保つ。
pub(super) fn build(
    budget: &DecodeBudget,
    font: &Font<'_>,
    plan: &Plan,
    attributes: &Attributes,
) -> Option<Data> {
    let limits = limits(budget, font, plan, attributes)?;
    let mut widths = [0.; 256];
    for (target, code) in widths.iter_mut().zip(&plan.codes) {
        *target = code.mapping.source.width;
    }
    let units = scale::choose_units(budget, &widths, &limits)?;
    let mut names = budget.font_vector(plan.glyphs.len())?;
    let mut programs = budget.font_vector(plan.glyphs.len())?;
    let mut metrics = budget.font_vector(plan.glyphs.len())?;
    let mut bbox: Option<[i16; 4]> = None;
    for (gid, entry) in plan.glyphs.iter().enumerate() {
        if budget.is_stopped() {
            return None;
        }
        let original = font.glyphs.get(usize::from(entry.original))?;
        let name = if entry.pdf_width.is_some() {
            variant_name(budget, font, gid)?
        } else {
            let mut copy = budget.font_vector(original.name.len())?;
            copy.extend_from_slice(&original.name);
            copy
        };
        let glyph = scale::prepare(budget, &original.outline, entry.pdf_width, units)?;
        let program = type2::encode(budget, &glyph)?;
        let metric = bounds::measure(budget, &glyph)?;
        if let Some(b) = metric.bounds {
            match &mut bbox {
                Some(out) => {
                    out[0] = out[0].min(b[0]);
                    out[1] = out[1].min(b[1]);
                    out[2] = out[2].max(b[2]);
                    out[3] = out[3].max(b[3]);
                }
                None => bbox = Some(b),
            }
        }
        names.push(name);
        programs.push(program);
        metrics.push(metric);
    }
    (!budget.is_stopped()).then_some(Data {
        units,
        names,
        programs,
        metrics,
        bbox: bbox.unwrap_or([0; 4]),
    })
}

/// 元座標・連続点差分・実sidebearing・属性の最大値を、丸める前に集める。
fn limits(
    budget: &DecodeBudget,
    font: &Font<'_>,
    plan: &Plan,
    attributes: &Attributes,
) -> Option<scale::Limits> {
    let mut limits = scale::Limits {
        coordinate: 0.,
        delta: 0.,
        metric: 0.,
        advance: 0.,
    };
    for value in [
        attributes.ascent,
        attributes.descent,
        attributes.underline_position,
        attributes.underline_thickness,
        attributes.x_height,
        attributes.cap_height,
    ] {
        limits.metric = limits.metric.max(value.abs());
    }
    for entry in &plan.glyphs {
        if budget.is_stopped() {
            return None;
        }
        let glyph = &font.glyphs.get(usize::from(entry.original))?.outline;
        let advance = entry.pdf_width.unwrap_or(glyph.advance);
        limits.advance = limits.advance.max(advance);
        let extent = bounds::source(budget, glyph)?;
        limits.metric = limits
            .metric
            .max(extent.map_or(advance, |b| (advance - b[2]).abs()));
        let mut current = [0.; 2];
        for command in &glyph.commands {
            if budget.is_stopped() {
                return None;
            }
            match command {
                Command::Move(p) | Command::Line(p) => point(&mut limits, &mut current, *p)?,
                Command::Curve(p) => {
                    for pair in p.chunks_exact(2) {
                        point(&mut limits, &mut current, [pair[0], pair[1]])?;
                    }
                }
                Command::Close => {}
            }
        }
    }
    limits.metric = limits.metric.max(limits.coordinate);
    Some(limits)
}

/// 曲線制御点も含む実数範囲を集め、非有限値を単位数選択へ渡さない。
fn point(limits: &mut scale::Limits, current: &mut [f64; 2], point: [f64; 2]) -> Option<()> {
    for axis in 0..2 {
        if !point[axis].is_finite() {
            return None;
        }
        limits.coordinate = limits.coordinate.max(point[axis].abs());
        limits.delta = limits.delta.max((point[axis] - current[axis]).abs());
    }
    *current = point;
    Some(())
}

/// 全原本名を残し、必要な幅違いへだけ予約量を明示した衝突のないASCII名を作る。
fn variant_name(budget: &DecodeBudget, font: &Font<'_>, gid: usize) -> Option<Vec<u8>> {
    for salt in 0..=font.glyphs.len() {
        if budget.is_stopped() {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes[..9].copy_from_slice(b"JadeWidth");
        let mut end = 9;
        decimal(&mut bytes, &mut end, gid)?;
        *bytes.get_mut(end)? = b'-';
        end += 1;
        decimal(&mut bytes, &mut end, salt)?;
        if font.glyphs.iter().any(|g| g.name == bytes[..end]) {
            continue;
        }
        let mut name = budget.font_vector(end)?;
        name.extend_from_slice(&bytes[..end]);
        return Some(name);
    }
    None
}

/// 一時format文字列を確保せず、固定bufferへ有限のGID・saltを十進数で書く。
fn decimal(bytes: &mut [u8; 32], end: &mut usize, mut value: usize) -> Option<()> {
    let mut reverse = [0u8; 20];
    let mut count = 0;
    loop {
        *reverse.get_mut(count)? = b'0' + (value % 10) as u8;
        count += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for &digit in reverse[..count].iter().rev() {
        *bytes.get_mut(*end)? = digit;
        *end += 1;
    }
    Some(())
}
