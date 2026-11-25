//! 添字復元をまたいで、本文の水平分割の根拠を保持する。
use super::{ParsedTextElement, TextRowSegment, text_column_gutter};

/// 添字を復元する前の本文行から、元の水平分割位置を確定する。
pub(super) fn cuts(
    row: &[&ParsedTextElement],
    gutter: Option<text_column_gutter::Gutter>,
) -> Vec<f32> {
    let Some(first) = row.first() else {
        return Vec::new();
    };
    let avg_height = row
        .iter()
        .map(|text| (text.bounds.y1 - text.bounds.y0).abs().max(1.0))
        .sum::<f32>()
        / row.len() as f32;
    let mut right = first.bounds.x1;
    let mut cuts = Vec::new();
    for text in row.iter().skip(1) {
        if text.bounds.x0 - right > avg_height * 8.0
            || gutter.is_some_and(|g| g.contains(first) && right < g.x && g.x < text.bounds.x0)
        {
            cuts.push(right / 2.0 + text.bounds.x0 / 2.0);
        }
        right = right.max(text.bounds.x1);
    }
    cuts
}

/// 復元した添字で尺度を再推定せず、元の行の分割位置を使う。
pub(super) fn split<'a>(
    row: &[&'a ParsedTextElement],
    gutter: Option<text_column_gutter::Gutter>,
    cuts: &[f32],
) -> Vec<TextRowSegment<'a>> {
    let Some(first) = row.first() else {
        return Vec::new();
    };
    let mut segments: Vec<Vec<&ParsedTextElement>> = vec![vec![*first]];
    let mut bin = cuts.partition_point(|cut| *cut <= first.bounds.x0);
    for text in row.iter().copied().skip(1) {
        let next = cuts.partition_point(|cut| *cut <= text.bounds.x0);
        if next != bin {
            segments.push(vec![text]);
        } else if let Some(segment) = segments.last_mut() {
            segment.push(text);
        }
        bin = next;
    }
    let split_from_row = segments.len() > 1 || gutter.is_some_and(|gutter| gutter.contains(first));
    segments
        .into_iter()
        .map(|texts| TextRowSegment {
            texts,
            split_from_row,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jade_pdf2docx_parser::ParsedBounds;
    use proptest::prelude::*;

    /// 既知の原点・em高を持つ所有断片を作り、製品の段組推定から期待値を逆算しない。
    fn text(left: f32, right: f32, y: f32) -> ParsedTextElement {
        ParsedTextElement {
            page: 0,
            text: "376".into(),
            font_name: None,
            font_size: 10.,
            font_weight: None,
            fill_color: None,
            baseline_y: Some(y),
            bounds: ParsedBounds {
                x0: left,
                x1: right,
                y0: y,
                y1: y + 10.,
            },
        }
    }

    /// 四行で確認した本文の段間だけを作り、上下の短い数値行は根拠へ混ぜない。
    fn body() -> Vec<Vec<ParsedTextElement>> {
        (0..4)
            .map(|i| {
                vec![
                    text(90., 294., 700. - i as f32 * 20.),
                    text(318., 522., 700. - i as f32 * 20.),
                ]
            })
            .collect()
    }

    /// 本文の外にある全幅数値行は狭い段間で割らず、本文内と従来の広い間隔は分割する。
    #[test]
    fn narrow_body_gutter_does_not_split_leading_or_trailing_numeric_rows() {
        let rows = body();
        let refs = rows
            .iter()
            .map(|row| row.iter().collect())
            .collect::<Vec<_>>();
        let gutter = text_column_gutter::infer(&refs, 612.).unwrap();
        for y in [740., 600.] {
            let row = [text(280., 298., y), text(314., 332., y)];
            assert!(
                cuts(&row.iter().collect::<Vec<_>>(), Some(gutter)).is_empty(),
                "outside body at y={y}"
            );
        }
        let within = [text(280., 298., 680.), text(314., 332., 680.)];
        assert!(!cuts(&within.iter().collect::<Vec<_>>(), Some(gutter)).is_empty());
        let wide = [text(90., 108., 740.), text(318., 336., 740.)];
        assert!(!cuts(&wide.iter().collect::<Vec<_>>(), Some(gutter)).is_empty());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]
        /// 一様倍率と平行移動によらず、狭い段間の適用を元の本文の垂直区間だけへ限定する。
        #[test]
        fn narrow_gutter_keeps_its_vertical_scope_under_affine_changes(scale in 0.5f32..2., shift in -10f32..10.) {
            let transform = |text: &mut ParsedTextElement| {
                text.bounds.x0 = (text.bounds.x0 + shift) * scale;
                text.bounds.x1 = (text.bounds.x1 + shift) * scale;
                text.bounds.y0 = (text.bounds.y0 + shift) * scale;
                text.bounds.y1 = (text.bounds.y1 + shift) * scale;
                text.baseline_y = text.baseline_y.map(|y| (y + shift) * scale);
                text.font_size *= scale;
            };
            let mut rows = body();
            for text in rows.iter_mut().flatten() { transform(text); }
            let refs = rows.iter().map(|row| row.iter().collect()).collect::<Vec<_>>();
            let gutter = text_column_gutter::infer(&refs, 612. * scale).unwrap();
            for (y, should_split) in [(740., false), (600., false), (680., true)] {
                let mut row = [text(280., 298., y), text(314., 332., y)];
                for text in &mut row { transform(text); }
                prop_assert_eq!(!cuts(&row.iter().collect::<Vec<_>>(), Some(gutter)).is_empty(), should_split);
            }
        }
    }
}
