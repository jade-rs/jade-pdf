//! 原本の横罫線と縦罫線から、表の外辺と行帯が確認できる場合だけgridへ戻す。
use super::TABLE_RULE_JOIN_TOLERANCE_PT;
use jade_pdf2docx_parser::ParsedBounds;

/// 長い縦罫線から得た範囲の外側へ、両端の根拠がある部分外辺だけを追加する。
pub(super) fn augment(
    xs: &[i32],
    ys: &[i32],
    vertical: &[ParsedBounds],
    horizontal: &[ParsedBounds],
) -> Vec<i32> {
    let mut out = xs.to_vec();
    let (Some(&left), Some(&right), Some(&bottom), Some(&top)) =
        (xs.first(), xs.last(), ys.first(), ys.last())
    else {
        return out;
    };
    if left >= right || bottom >= top {
        return out;
    }
    for edge in vertical.iter().filter(|b| finite(b)) {
        let x = f64::from(edge.x0).round();
        if !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&x) {
            continue;
        }
        let x = x as i32;
        let is_left = i64::from(x) < i64::from(left) - 1;
        let is_right = i64::from(x) > i64::from(right) + 1;
        if !(is_left || is_right)
            || !ys
                .windows(2)
                .any(|band| edge.y0 <= band[0] as f32 + 1. && edge.y1 >= band[1] as f32 - 1.)
        {
            continue;
        }
        let anchored = |y: i32| {
            horizontal.iter().filter(|b| finite(b)).any(|rule| {
                (rule.y0 - y as f32).abs() <= 1.
                    && rule.x0 <= left as f32 + 1.
                    && rule.x1 >= right as f32 - 1.
                    && (if is_left { rule.x0 } else { rule.x1 } - x as f32).abs() <= 1.
            })
        };
        if anchored(bottom) && anchored(top) {
            out.push(x);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 全列の縦罫線が一つの全幅の行罫線を横切り、上下端がそろう場合の二行を返す。
pub(super) fn open_horizontal_rows(
    xs: &[i32],
    ys: &[i32],
    vertical: &[ParsedBounds],
    horizontal: &[ParsedBounds],
) -> Option<[i32; 3]> {
    let [left, _, .., right] = xs else {
        return None;
    };
    let [separator] = ys else {
        return None;
    };
    let tolerance = TABLE_RULE_JOIN_TOLERANCE_PT as f32;
    if !horizontal.iter().any(|line| {
        line.y0 == *separator as f32
            && (line.x0 - *left as f32).abs() <= tolerance
            && (line.x1 - *right as f32).abs() <= tolerance
    }) {
        return None;
    }

    vertical
        .iter()
        .filter(|line| {
            line.x0 == *left as f32
                && line.y0 + tolerance < *separator as f32
                && line.y1 - tolerance > *separator as f32
        })
        .find(|outer| {
            xs.iter().all(|&x| {
                vertical
                    .iter()
                    .any(|line| line.x0 == x as f32 && line.y0 == outer.y0 && line.y1 == outer.y1)
            })
        })
        .map(|outer| [outer.y0 as i32, *separator, outer.y1 as i32])
}

/// 座標不明の罫線を、floatから整数への飽和castで有効な外辺とみなさない。
fn finite(bounds: &ParsedBounds) -> bool {
    [bounds.x0, bounds.y0, bounds.x1, bounds.y1]
        .into_iter()
        .all(f32::is_finite)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 所有する罫線座標を、上下左右が正規化されたboundsへする。
    fn b(x0: f32, y0: f32, x1: f32, y1: f32) -> ParsedBounds {
        ParsedBounds { x0, y0, x1, y1 }
    }

    /// 上下端のずれた縦罫線や途中で切れた行罫線から行帯を推測しない。
    #[test]
    fn open_horizontal_rows_require_aligned_columns_and_a_full_width_separator() {
        let xs = [20, 120, 220];
        let ys = [110];
        let mut vertical = [
            b(20., 60., 20., 160.),
            b(120., 60., 120., 160.),
            b(220., 60., 220., 160.),
        ];
        let full_width_rule = [b(20., 110., 220., 110.)];
        assert_eq!(
            open_horizontal_rows(&xs, &ys, &vertical, &full_width_rule),
            Some([60, 110, 160])
        );
        assert_eq!(
            open_horizontal_rows(&xs, &ys, &vertical, &[b(20., 110., 200., 110.)]),
            None
        );
        vertical[1].y1 -= 10.;
        assert_eq!(
            open_horizontal_rows(&xs, &ys, &vertical, &full_width_rule),
            None
        );
    }

    /// 片側の外辺が全高の75%でも、上下の実線と行帯で確かめた外辺だけを追加する。
    #[test]
    fn both_anchored_partial_sides_extend_seed_bounds() {
        let horizontal = [b(10., 100., 90., 100.), b(10., 500., 90., 500.)];
        let vertical = [b(10., 100., 10., 400.), b(90., 100., 90., 400.)];
        assert_eq!(
            augment(&[40, 60], &[100, 400, 500], &vertical, &horizontal),
            [10, 40, 60, 90]
        );
    }

    /// 横罫線が長くても、対応する実際の縦罫線がなければ外辺を推測しない。
    #[test]
    fn horizontal_endpoints_alone_do_not_create_columns() {
        let horizontal = [b(10., 100., 90., 100.), b(10., 500., 90., 500.)];
        assert_eq!(
            augment(&[40, 60], &[100, 400, 500], &[], &horizontal),
            [40, 60]
        );
    }

    /// 上下どちらかの端が一致しない線は、無関係な外側の線と区別できず採用しない。
    #[test]
    fn both_horizontal_endpoints_are_required() {
        let vertical = [b(90., 100., 90., 400.)];
        for horizontal in [
            vec![b(10., 100., 90., 100.)],
            vec![b(10., 100., 90., 100.), b(10., 500., 95., 500.)],
            vec![b(10., 100., 95., 100.), b(10., 500., 90., 500.)],
            vec![b(65., 100., 90., 100.), b(65., 500., 90., 500.)],
        ] {
            assert_eq!(
                augment(&[40, 60], &[100, 400, 500], &vertical, &horizontal),
                [40, 60]
            );
        }
    }

    /// 行帯を覆わない短片や不明な座標は、実際の列境界として使わない。
    #[test]
    fn short_or_nonfinite_vertical_fragments_do_not_extend() {
        let horizontal = [b(10., 100., 90., 100.), b(10., 500., 90., 500.)];
        let vertical = [
            b(90., 150., 90., 350.),
            b(90., f32::NAN, 90., 400.),
            b(f32::INFINITY, 100., f32::INFINITY, 400.),
        ];
        assert_eq!(
            augment(&[40, 60], &[100, 400, 500], &vertical, &horizontal),
            [40, 60]
        );
    }

    /// 有限でもgridの整数範囲を超える外辺を、飽和castで有効な別位置へ移さない。
    #[test]
    fn finite_out_of_grid_range_does_not_saturate_into_a_column() {
        let x = i32::MAX as f32;
        let horizontal = [b(10., 100., x, 100.), b(10., 500., x, 500.)];
        let vertical = [b(x, 100., x, 400.)];
        assert_eq!(
            augment(&[40, 60], &[100, 400, 500], &vertical, &horizontal),
            [40, 60],
            "out-of-grid coordinate must not saturate into a source column"
        );
    }
}
