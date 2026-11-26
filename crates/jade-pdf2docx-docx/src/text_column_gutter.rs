//! 複数の本文行が共有する狭い空白帯を、通常の単語間隔と区別する。
use jade_pdf2docx_parser::ParsedTextElement;

/// 十分な左右本文幅を持つ一行で、段間候補となる水平空白を表す。
struct Gap {
    /// 空白帯の左端。
    left: f32,
    /// 空白帯の右端。
    right: f32,
    /// 同じ行から複数候補が出ても、垂直範囲の検査で元の行を参照する。
    row: usize,
    /// 数値同士の隙間は競合候補には残し、本文区間を広げる根拠にはしない。
    body_evidence: bool,
}

/// 共通する空白帯と、その存在を複数行で確認できた本文の垂直区間。
#[derive(Clone, Copy)]
pub(super) struct Gutter {
    /// 左右の段を分ける空白帯内のx座標。
    pub(super) x: f32,
    /// 根拠となる本文行の最下端baseline。
    bottom: f32,
    /// 根拠となる本文行の最上端baseline。
    top: f32,
    /// 既存の推定が成立する場合だけ、従来どおり本文範囲外の行も分割する。
    cut_outside_body: bool,
}

impl Gutter {
    /// 実際に空白帯を確認した垂直区間にある片側の継続行も同じ段組へ含める。
    pub(super) fn contains(self, text: &ParsedTextElement) -> bool {
        (self.bottom..=self.top).contains(&super::native_baseline::line_y(text))
    }

    /// 新しく本文だけから復元した段間は、確認した本文区間内だけに使う。
    pub(super) fn cuts_row(self, text: &ParsedTextElement) -> bool {
        self.cut_outside_body || self.contains(text)
    }
}

/// 四行以上が共有する候補が一帯だけあり、本文区間を他の文字が横断しない場合だけ返す。
/// 通常の広い段間判定は変更せず、狭い段間の補助として使用する。
/// 各rowはx座標順で、同じbaselineの断片が一つのrowへ集約されていること。
pub(super) fn infer(rows: &[Vec<&ParsedTextElement>], width: f32) -> Option<Gutter> {
    if !width.is_finite() || width <= 0.0 {
        return None;
    }
    let mut gaps = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let Some(first) = row.first() else { continue };
        if row.iter().any(|text| {
            ![
                text.bounds.x0,
                text.bounds.x1,
                text.bounds.y0,
                text.bounds.y1,
                text.font_size,
                super::native_baseline::line_y(text),
            ]
            .iter()
            .all(|v| v.is_finite())
                || text.bounds.x1 <= text.bounds.x0
                || text.font_size <= 0.0
        }) {
            continue;
        }
        let end = row
            .iter()
            .map(|t| t.bounds.x1)
            .fold(first.bounds.x1, f32::max);
        let mut right = first.bounds.x1;
        let mut previous_size = first.font_size;
        let mut previous = *first;
        for text in row.iter().skip(1) {
            let next = text.bounds.x0;
            let centre = right / 2.0 + next / 2.0;
            if next - right >= previous_size.max(text.font_size) * 1.25
                && right - first.bounds.x0 >= width * 0.2
                && end - next >= width * 0.2
                && (width * 0.3..=width * 0.7).contains(&centre)
            {
                gaps.push(Gap {
                    left: right,
                    right: next,
                    row: index,
                    body_evidence: !super::text_row_segments::numeric_pair(
                        &previous.text,
                        &text.text,
                    ),
                });
            }
            right = right.max(text.bounds.x1);
            previous_size = text.font_size;
            previous = text;
        }
    }
    // 数値の候補を除くだけで、元は複数だった段間を一意と見なさない。
    let original_cut = supported_cut(&gaps, width, false)?;
    let (bottom, top) = vertical_span(&gaps, rows, original_cut, false)?;
    if vertical_span(&gaps, rows, original_cut, true).is_some()
        && !crosses(rows, original_cut, bottom, top)
    {
        return Some(Gutter {
            x: original_cut,
            bottom,
            top,
            cut_outside_body: true,
        });
    }
    let cut = supported_cut(&gaps, width, true)?;
    let (bottom, top) = vertical_span(&gaps, rows, cut, true)?;
    (!crosses(rows, cut, bottom, top)).then_some(Gutter {
        x: cut,
        bottom,
        top,
        cut_outside_body: false,
    })
}

/// 四行以上で共有する空白帯が一つだけあるとき、その帯の最も支持された点を返す。
fn supported_cut(gaps: &[Gap], width: f32, body_only: bool) -> Option<f32> {
    let mut events = gaps
        .iter()
        .filter(|gap| !body_only || gap.body_evidence)
        .flat_map(|gap| [(gap.left, 1_i32), (gap.right, -1)])
        .collect::<Vec<_>>();
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut active = 0_i32;
    let mut best = (0_i32, 0_f32);
    let mut supported_regions = 0;
    let mut previously_supported = false;
    let mut index = 0;
    while index < events.len() {
        let x = events[index].0;
        while index < events.len() && events[index].0 == x {
            active += events[index].1;
            index += 1;
        }
        if let Some((next, _)) = events.get(index) {
            let supported = active >= 4 && *next > x;
            if supported && !previously_supported {
                supported_regions += 1;
            }
            previously_supported = supported;
            let centre = x / 2.0 + next / 2.0;
            if active > best.0
                || (active == best.0 && (centre - width / 2.0).abs() < (best.1 - width / 2.0).abs())
            {
                best = (active, centre);
            }
        }
    }
    (best.0 >= 4 && supported_regions == 1).then_some(best.1)
}

/// 選んだ段間を共有する四行以上の、実際のbaseline範囲を返す。
fn vertical_span(
    gaps: &[Gap],
    rows: &[Vec<&ParsedTextElement>],
    cut: f32,
    body_only: bool,
) -> Option<(f32, f32)> {
    let mut vertical = gaps
        .iter()
        .filter(|gap| (!body_only || gap.body_evidence) && gap.left < cut && cut < gap.right)
        .map(|gap| super::native_baseline::line_y(rows[gap.row][0]));
    let first = vertical.next()?;
    let mut body_rows = 1;
    let (bottom, top) = vertical.fold((first, first), |(bottom, top), y| {
        body_rows += 1;
        (bottom.min(y), top.max(y))
    });
    (body_rows >= 4).then_some((bottom, top))
}

/// 確認した本文範囲内で段間を横断する文字があれば、その段組は採用しない。
fn crosses(rows: &[Vec<&ParsedTextElement>], cut: f32, bottom: f32, top: f32) -> bool {
    rows.iter().any(|row| {
        row.first().is_some_and(|first| {
            (bottom..=top).contains(&super::native_baseline::line_y(first))
                && row
                    .iter()
                    .any(|text| text.bounds.x0 < cut && cut < text.bounds.x1)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jade_pdf2docx_parser::ParsedBounds;
    use proptest::prelude::*;

    /// 単語内容に依存しない、独立した左右の本文矩形を作る。
    fn rows() -> Vec<Vec<ParsedTextElement>> {
        (0..4)
            .map(|index| {
                [(90.0, 294.0), (318.0, 522.0)]
                    .into_iter()
                    .map(|(left, right)| text(left, right, 700.0 - index as f32 * 20.0))
                    .collect()
            })
            .collect()
    }

    /// 所有の文字列と既知の描画矩形だけを持つ本文断片を作る。
    fn text(left: f32, right: f32, y: f32) -> ParsedTextElement {
        ParsedTextElement {
            font_weight: None,
            baseline_y: None,
            page: 0,
            bounds: ParsedBounds {
                x0: left,
                y0: y,
                x1: right,
                y1: y + 10.0,
            },
            font_size: 10.0,
            font_name: None,
            fill_color: None,
            text: "owned body text".to_owned(),
        }
    }

    /// 元の所有データを変更せず、表示行の借用だけを渡す。
    fn infer_owned(rows: &[Vec<ParsedTextElement>], width: f32) -> Option<f32> {
        infer(
            &rows
                .iter()
                .map(|row| row.iter().collect())
                .collect::<Vec<_>>(),
            width,
        )
        .map(|gutter| gutter.x)
    }

    /// 少数行・通常の語間・短い本文断片を段間へ昇格しない。
    #[test]
    fn insufficient_rows_or_normal_word_gaps_are_not_columns() {
        assert_eq!(infer_owned(&rows()[..3], 612.0), None);
        let mut short = rows();
        for row in &mut short {
            row[0].bounds.x1 = 110.0;
            row[1].bounds.x0 = 125.0;
            row[1].bounds.x1 = 160.0;
        }
        assert_eq!(infer_owned(&short, 612.0), None);
        let mut words = rows();
        for row in &mut words {
            row[1].bounds.x0 = 296.0;
        }
        assert_eq!(infer_owned(&words, 612.0), None);
    }

    /// 三段など複数の独立した空白帯を、一つの二段本文へ決め打ちしない。
    #[test]
    fn multiple_sustained_gutters_are_ambiguous() {
        let source = (0..8)
            .map(|row| {
                [(54., 200.), (220., 392.), (412., 558.)]
                    .into_iter()
                    .map(|(left, right)| text(left, right, 700. - row as f32 * 18.))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            infer_owned(&source, 612.),
            None,
            "multiple sustained gutters must not be forced into two columns"
        );
    }

    /// 幾何だけなら段間になる反復数値表も、数値同士の隙間を散文の根拠へ数えない。
    #[test]
    fn repeated_numeric_fields_do_not_establish_a_prose_gutter() {
        let mut source = rows();
        for row in &mut source {
            row[0].text = "224".into();
            row[1].text = "144 7 - 6.67%".into();
        }
        assert_eq!(infer_owned(&source, 612.), None);
        let mut partly_numeric = rows();
        partly_numeric[3][0].text = "224".into();
        partly_numeric[3][1].text = "144".into();
        assert_eq!(
            infer_owned(&partly_numeric, 612.),
            None,
            "numeric rows cannot supply the missing fourth body row"
        );
    }

    /// 本文と数値表で異なる二つの段間候補が反復する所有入力を作る。
    fn mixed_gutters() -> Vec<Vec<ParsedTextElement>> {
        let mut source = (0..4)
            .map(|i| {
                vec![
                    text(70., 210., 700. - i as f32 * 20.),
                    text(230., 542., 700. - i as f32 * 20.),
                ]
            })
            .collect::<Vec<_>>();
        for i in 0..4 {
            let mut row = vec![
                text(70., 370., 600. - i as f32 * 20.),
                text(390., 542., 600. - i as f32 * 20.),
            ];
            row[0].text = "224".into();
            row[1].text = "144".into();
            source.push(row);
        }
        source
    }

    /// 数値の競合候補を取り除いた結果だけで、元は曖昧だった段間を確定しない。
    #[test]
    fn numeric_competition_still_keeps_mixed_gutters_ambiguous() {
        assert_eq!(
            infer_owned(&mixed_gutters(), 612.),
            None,
            "discarding numerical evidence must not create a new unambiguous prose layout"
        );
    }

    /// 反復する数値行と本文の間に、元の段間を横断する一段行を持つ所有原本。
    fn crossing_numeric_scope() -> Vec<Vec<ParsedTextElement>> {
        let mut source = rows();
        let mut numeric = vec![text(90., 294., 580.), text(318., 522., 580.)];
        numeric[0].text = "224".into();
        numeric[1].text = "144".into();
        source.push(numeric);
        source.push(vec![text(90., 522., 670.)]);
        source
    }

    /// 数値根拠の範囲を縮めても、元の候補範囲を横断する本文の拒否を解除しない。
    #[test]
    fn numeric_scope_reduction_does_not_remove_a_crossing_line_veto() {
        assert_eq!(
            infer_owned(&crossing_numeric_scope(), 612.),
            None,
            "removing numeric support must not create a new prose column layout"
        );
    }

    /// 本文区間の外の数値行は、散文の横断行として本文の段組を拒否しない。
    #[test]
    fn numeric_crossing_row_outside_body_does_not_veto_prose_columns() {
        let mut source = crossing_numeric_scope();
        source.last_mut().unwrap()[0].bounds.y0 = 610.;
        source.last_mut().unwrap()[0].bounds.y1 = 620.;
        source.last_mut().unwrap()[0].text = "224 144 7 6.67%".into();
        assert_eq!(infer_owned(&source, 612.), Some(306.));
    }

    /// 左側に散文があっても、右側が数値だけの行を二段の散文の根拠にはしない。
    #[test]
    fn prose_rows_with_only_numeric_right_side_do_not_establish_the_gutter() {
        let source = (0..4)
            .map(|i| {
                let y = 700. - i as f32 * 20.;
                let mut row = vec![text(90., 250., y), text(260., 294., y), text(318., 522., y)];
                row[1].text = "2016.".into();
                row[2].text = "6181, 2018.".into();
                row
            })
            .collect::<Vec<_>>();
        assert_eq!(infer_owned(&source, 612.), None);
    }

    /// 本文区間を横断する一段行は拒否し、区間外の見出しは段組を壊さない。
    #[test]
    fn crossing_body_line_blocks_gutter_but_heading_does_not() {
        let mut body = rows();
        body.push(vec![text(90.0, 522.0, 670.0)]);
        assert_eq!(infer_owned(&body, 612.0), None);
        body.last_mut().unwrap()[0].bounds.y0 = 740.0;
        assert_eq!(infer_owned(&body, 612.0), Some(306.0));
    }

    /// 非有限の段組寸法や行は、四行分の根拠として数えない。
    #[test]
    fn invalid_geometry_does_not_support_gutter() {
        for width in [f32::NAN, f32::INFINITY, 0.0, -1.0] {
            assert_eq!(infer_owned(&rows(), width), None);
        }
        for y in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut invalid = rows();
            invalid[0][0].bounds.y0 = y;
            assert_eq!(infer_owned(&invalid, 612.0), None);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// 数値だけの横断行を本文から外す判断も、一様倍率・移動・行順で変えない。
        #[test]
        fn numeric_only_crossing_scope_is_stable_under_affine_changes(scale in 0.5_f32..2., shift in -10_f32..10.) {
            let mut source = crossing_numeric_scope();
            source.last_mut().unwrap()[0].bounds.y0 = 610.;
            source.last_mut().unwrap()[0].bounds.y1 = 620.;
            source.last_mut().unwrap()[0].text = "224 144 7 6.67%".into();
            for text in source.iter_mut().flatten() {
                text.bounds.x0 = (text.bounds.x0 + shift) * scale;
                text.bounds.x1 = (text.bounds.x1 + shift) * scale;
                text.bounds.y0 = (text.bounds.y0 + shift) * scale;
                text.bounds.y1 = (text.bounds.y1 + shift) * scale;
                text.font_size *= scale;
            }
            let cut = infer_owned(&source, 612. * scale).unwrap();
            prop_assert!((cut - (306. + shift) * scale).abs() < 0.001);
            source.reverse();
            prop_assert_eq!(infer_owned(&source, 612. * scale), Some(cut));
        }

        /// 倍率・平行移動・行順を変えても、数値区間と本文の間の横断行は拒否し続ける。
        #[test]
        fn numeric_scope_keeps_crossing_veto_under_affine_changes(scale in 0.5_f32..2., shift in -10_f32..10.) {
            let mut source = crossing_numeric_scope();
            for text in source.iter_mut().flatten() {
                text.bounds.x0 = (text.bounds.x0 + shift) * scale;
                text.bounds.x1 = (text.bounds.x1 + shift) * scale;
                text.bounds.y0 = (text.bounds.y0 + shift) * scale;
                text.bounds.y1 = (text.bounds.y1 + shift) * scale;
                text.font_size *= scale;
            }
            prop_assert_eq!(infer_owned(&source, 612. * scale), None);
            source.reverse();
            prop_assert_eq!(infer_owned(&source, 612. * scale), None);
        }

        /// 座標の一様倍率・平行移動・行順によらず、数値候補を含む曖昧さを維持する。
        #[test]
        fn mixed_gutter_ambiguity_is_preserved_under_affine_changes(scale in 0.5_f32..2., shift in -10_f32..10.) {
            let mut source = mixed_gutters();
            for text in source.iter_mut().flatten() {
                text.bounds.x0 = (text.bounds.x0 + shift) * scale;
                text.bounds.x1 = (text.bounds.x1 + shift) * scale;
                text.bounds.y0 *= scale;
                text.bounds.y1 *= scale;
                text.font_size *= scale;
            }
            prop_assert_eq!(infer_owned(&source, 612. * scale), None);
            source.reverse();
            prop_assert_eq!(infer_owned(&source, 612. * scale), None);
        }

        /// ページ・本文・fontの一様倍率と平行移動で、同じ物理的な空白帯を保つ。
        #[test]
        fn affine_geometry_keeps_the_same_gutter(scale in 0.5_f32..2.0, shift in -10.0_f32..10.0) {
            let mut source = rows();
            for row in &mut source {
                for text in row {
                    text.bounds.x0 = (text.bounds.x0 + shift) * scale;
                    text.bounds.x1 = (text.bounds.x1 + shift) * scale;
                    text.bounds.y0 *= scale;
                    text.bounds.y1 *= scale;
                    text.font_size *= scale;
                }
            }
            let cut = infer_owned(&source, 612.0 * scale).unwrap();
            prop_assert!((cut - (306.0 + shift) * scale).abs() < 0.001);
            source.reverse();
            prop_assert_eq!(infer_owned(&source, 612.0 * scale), Some(cut));
        }
    }
}
