//! Type2と同じ絶対座標量子化から、実際の三次曲線のmetricを計算する。
use super::{
    charstring::{Command, Glyph},
    opentype::Metric,
};
use crate::decode::DecodeBudget;

/// 制御点の箱ではなく、描画される曲線の極値を外向きに丸めて格納する。
pub(super) fn measure(budget: &DecodeBudget, glyph: &Glyph) -> Option<Metric> {
    if budget.is_stopped()
        || glyph.commands.len() > 100_000
        || !glyph.advance.is_finite()
        || !(0. ..=65535.).contains(&glyph.advance)
    {
        return None;
    }
    let mut current = [0.; 2];
    let mut open = false;
    let mut bounds = None;
    for command in &glyph.commands {
        if budget.is_stopped() {
            return None;
        }
        match command {
            Command::Move(point) if !open => {
                current = point_after(*point, current)?;
                open = true;
            }
            Command::Line(point) if open => {
                let next = point_after(*point, current)?;
                include(&mut bounds, current);
                include(&mut bounds, next);
                current = next;
            }
            Command::Curve(points) if open => {
                let a = point_after([points[0], points[1]], current)?;
                let b = point_after([points[2], points[3]], a)?;
                let end = point_after([points[4], points[5]], b)?;
                include(&mut bounds, current);
                include(&mut bounds, end);
                for axis in 0..2 {
                    for t in extrema([current[axis], a[axis], b[axis], end[axis]]) {
                        if t > 0. && t < 1. {
                            include(
                                &mut bounds,
                                [
                                    evaluate([current[0], a[0], b[0], end[0]], t),
                                    evaluate([current[1], a[1], b[1], end[1]], t),
                                ],
                            );
                        }
                    }
                }
                current = end;
            }
            Command::Close if open => open = false,
            _ => return None,
        }
    }
    if open || budget.is_stopped() {
        return None;
    }
    let left = bounds.map_or(0., |b| (b[0] + 0.5).floor()) as i16;
    let bounds = bounds.map(|b| {
        [
            b[0].floor() as i16,
            b[1].floor() as i16,
            b[2].ceil() as i16,
            b[3].ceil() as i16,
        ]
    });
    Some(Metric {
        advance: (glyph.advance + 0.5).floor() as u16,
        left,
        bounds,
    })
}

/// 保存時と同じ16.16へ丸め、連続する点の差分も符号付き表現へ収める。
fn point_after(point: [f64; 2], previous: [f64; 2]) -> Option<[f64; 2]> {
    let mut output = [0.; 2];
    for axis in 0..2 {
        let value = point[axis];
        if !value.is_finite() || value.abs() > 32767. {
            return None;
        }
        let value = (value * 65536.).round() / 65536.;
        let delta = (value - previous[axis]) * 65536.;
        if !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&delta) {
            return None;
        }
        output[axis] = value;
    }
    Some(output)
}

/// 描画される端点・極値だけで、外形の実数範囲を拡張する。
fn include(bounds: &mut Option<[f64; 4]>, point: [f64; 2]) {
    match bounds {
        Some(b) => {
            b[0] = b[0].min(point[0]);
            b[1] = b[1].min(point[1]);
            b[2] = b[2].max(point[0]);
            b[3] = b[3].max(point[1]);
        }
        None => *bounds = Some([point[0], point[1], point[0], point[1]]),
    }
}

/// 三次Bezierの導関数を解き、桁落ちを避けた二次方程式から候補二点を返す。
fn extrema(p: [f64; 4]) -> [f64; 2] {
    let a = -p[0] + 3. * p[1] - 3. * p[2] + p[3];
    let b = 2. * (p[0] - 2. * p[1] + p[2]);
    let c = p[1] - p[0];
    if a == 0. {
        return if b == 0. { [-1.; 2] } else { [-c / b, -1.] };
    }
    let d = b * b - 4. * a * c;
    if d < 0. {
        return [-1.; 2];
    }
    let q = -0.5 * (b + d.sqrt().copysign(b));
    if q == 0. {
        [-b / (2. * a), -1.]
    } else {
        [q / a, c / q]
    }
}

/// de Casteljauの補間で、曲線上の点を制御点の範囲内で評価する。
fn evaluate(p: [f64; 4], t: f64) -> f64 {
    let u = 1. - t;
    let a = u * p[0] + t * p[1];
    let b = u * p[1] + t * p[2];
    let c = u * p[2] + t * p[3];
    u * (u * a + t * b) + t * (u * b + t * c)
}

#[cfg(test)]
#[path = "type1_outline_bounds_tests.rs"]
mod tests;
