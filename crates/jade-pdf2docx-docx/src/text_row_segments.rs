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
    let numeric_only = numeric_row(row);
    let mut cuts = Vec::new();
    for text in row.iter().skip(1) {
        if text.bounds.x0 - right > avg_height * 8.0
            || gutter.is_some_and(|g| {
                g.cuts_row(first) && right < g.x && g.x < text.bounds.x0 && !numeric_only
            })
        {
            cuts.push(right / 2.0 + text.bounds.x0 / 2.0);
        }
        right = right.max(text.bounds.x1);
    }
    cuts
}

/// 数値・符号・単位記号だけの短い二断片を、散文の狭い段間の根拠から除く。
pub(super) fn numeric_pair(left: &str, right: &str) -> bool {
    numeric_fragment(left) && numeric_fragment(right)
}

/// 行全体が数値表記の場合だけ散文の段間から除き、散文内の年・巻号は除かない。
pub(super) fn numeric_row(row: &[&ParsedTextElement]) -> bool {
    !row.is_empty() && row.iter().all(|text| numeric_fragment(&text.text))
}

/// 値の妥当性は推測せず、数字を含む短い非散文の表記だけを確認する。
fn numeric_fragment(text: &str) -> bool {
    text.len() <= 128
        && text.bytes().any(|c| c.is_ascii_digit())
        && text.chars().all(|c| {
            c.is_ascii_digit()
                || c.is_whitespace()
                || matches!(c, '+' | '-' | '−' | '.' | ',' | '%' | '(' | ')')
        })
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
            source_background: None,
            fill_stroke_width: None,
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
                let mut row = vec![
                    text(90., 294., 700. - i as f32 * 20.),
                    text(318., 522., 700. - i as f32 * 20.),
                ];
                row[0].text = "left prose content".into();
                row[1].text = "right prose content".into();
                row
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
        let mut within = [text(280., 298., 680.), text(314., 332., 680.)];
        within[0].text = "left prose".into();
        within[1].text = "right prose".into();
        assert!(!cuts(&within.iter().collect::<Vec<_>>(), Some(gutter)).is_empty());
        let wide = [text(90., 108., 740.), text(318., 336., 740.)];
        assert!(!cuts(&wide.iter().collect::<Vec<_>>(), Some(gutter)).is_empty());
    }

    /// 本文と同じ高さでも、数値の表記だけの隙間を散文の段間へ昇格しない。
    #[test]
    fn numeric_fields_do_not_supply_narrow_prose_column_cuts() {
        let rows = body();
        let refs = rows
            .iter()
            .map(|row| row.iter().collect())
            .collect::<Vec<_>>();
        let gutter = text_column_gutter::infer(&refs, 612.).unwrap();
        for (left, right) in [
            ("224", "144"),
            ("224", "7 - 6.67%"),
            ("21.0", "−2.4"),
            ("1,234", "(4.82%)"),
        ] {
            let mut row = [text(280., 298., 680.), text(314., 332., 680.)];
            row[0].text = left.into();
            row[1].text = right.into();
            assert!(
                cuts(&row.iter().collect::<Vec<_>>(), Some(gutter)).is_empty(),
                "{left} | {right}"
            );
        }
        for (left, right) in [
            ("left prose", "144"),
            ("224", "右段本文"),
            ("1e3", "144"),
            ("NaN", "144"),
        ] {
            let mut row = [text(280., 298., 680.), text(314., 332., 680.)];
            row[0].text = left.into();
            row[1].text = right.into();
            assert!(
                !cuts(&row.iter().collect::<Vec<_>>(), Some(gutter)).is_empty(),
                "{left} | {right}"
            );
        }
    }

    /// 散文行に含まれる年や巻号だけを見て、別段の参考文献の末尾を左段へ結合しない。
    #[test]
    fn numeric_suffixes_inside_prose_rows_keep_the_column_cut() {
        let rows = body();
        let refs = rows
            .iter()
            .map(|row| row.iter().collect())
            .collect::<Vec<_>>();
        let gutter = text_column_gutter::infer(&refs, 612.).unwrap();
        let mut row = [
            text(90., 250., 680.),
            text(260., 298., 680.),
            text(314., 400., 680.),
        ];
        row[0].text = "Reference in left column".into();
        row[1].text = "2016.".into();
        row[2].text = "6181, 2018.".into();
        assert!(!cuts(&row.iter().collect::<Vec<_>>(), Some(gutter)).is_empty());
    }

    /// 二段の本文の上に、同じ段間位置へ数値欄を持つ表と全幅captionを配置する。
    fn numerical_table_above_body() -> Vec<Vec<ParsedTextElement>> {
        let mut source = body();
        for text in source.iter_mut().flatten() {
            text.bounds.y0 -= 300.;
            text.bounds.y1 -= 300.;
            text.baseline_y = text.baseline_y.map(|y| y - 300.);
        }
        for i in 0..4 {
            let y = 740. - i as f32 * 20.;
            let mut row = vec![text(90., 230., y), text(240., 280., y), text(320., 522., y)];
            row[0].text = "Model label".into();
            source.push(row);
        }
        let mut caption = text(90., 522., 560.);
        caption.text = "Full width caption above prose".into();
        source.push(vec![caption]);
        source
    }

    /// 表の数値欄と本文の段間が重なっても、新しい本文段組を上の図表へ延ばさない。
    #[test]
    fn refined_prose_gutter_does_not_cut_the_leading_full_width_figure() {
        let source = numerical_table_above_body();
        let refs = source
            .iter()
            .map(|row| row.iter().collect())
            .collect::<Vec<_>>();
        let gutter = text_column_gutter::infer(&refs, 612.)
            .expect("only verified prose supplies body scope");
        let mut figure = [text(280., 298., 600.), text(314., 332., 600.)];
        figure[0].text = "FLOPS label".into();
        figure[1].text = "FLOPS label".into();
        assert!(cuts(&figure.iter().collect::<Vec<_>>(), Some(gutter)).is_empty());
        let legacy = body();
        let refs = legacy
            .iter()
            .map(|row| row.iter().collect())
            .collect::<Vec<_>>();
        let legacy_gutter = text_column_gutter::infer(&refs, 612.).unwrap();
        assert!(
            !cuts(&figure.iter().collect::<Vec<_>>(), Some(legacy_gutter)).is_empty(),
            "retain established legacy cuts outside body"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]
        /// 座標の倍率・移動・行順によらず、新規の狭い段間を確認済み本文区間へ限定する。
        #[test]
        fn refined_gutter_scope_is_stable_under_affine_changes(scale in 0.5f32..2., shift in -10f32..10.) {
            let transform = |text: &mut ParsedTextElement| {
                text.bounds.x0 = (text.bounds.x0 + shift) * scale;
                text.bounds.x1 = (text.bounds.x1 + shift) * scale;
                text.bounds.y0 = (text.bounds.y0 + shift) * scale;
                text.bounds.y1 = (text.bounds.y1 + shift) * scale;
                text.baseline_y = text.baseline_y.map(|y| (y + shift) * scale);
                text.font_size *= scale;
            };
            let mut source = numerical_table_above_body();
            for text in source.iter_mut().flatten() { transform(text); }
            for reverse in [false, true] {
                if reverse { source.reverse(); }
                let refs = source.iter().map(|row| row.iter().collect()).collect::<Vec<_>>();
                let gutter = text_column_gutter::infer(&refs, 612. * scale).unwrap();
                for (y, should_split) in [(600., false), (380., true)] {
                    let mut row = [text(280., 298., y), text(314., 332., y)];
                    row[0].text = "left prose".into(); row[1].text = "right prose".into();
                    for text in &mut row { transform(text); }
                    prop_assert_eq!(!cuts(&row.iter().collect::<Vec<_>>(), Some(gutter)).is_empty(), should_split);
                }
            }
        }

        /// 一様倍率と平行移動によらず、上下の全幅数値行を本文の段組へ混ぜない。
        #[test]
        fn numeric_rows_do_not_become_prose_columns_under_affine_changes(scale in 0.5f32..2., shift in -10f32..10.) {
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
                if should_split { row[0].text = "left prose".into(); row[1].text = "right prose".into(); }
                for text in &mut row { transform(text); }
                prop_assert_eq!(!cuts(&row.iter().collect::<Vec<_>>(), Some(gutter)).is_empty(), should_split);
            }
        }
    }
}
