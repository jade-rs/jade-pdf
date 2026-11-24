//! 同じセルの可変桁の数値行が原本の右辺付近へ揃う場合だけ、右端のanchorを保持する。
use super::*;

/// 複数の数値行の右端・左端の違いから確認できる右揃えだけを、元の行indexで返す。
pub(super) fn right_anchors(lines: &[StructuredTextLine], left: f32, right: f32) -> Vec<bool> {
    let mut out = vec![false; lines.len()];
    if !left.is_finite() || !right.is_finite() || left >= right {
        return out;
    }
    let numeric = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| numeric_token(&line.text))
        .collect::<Vec<_>>();
    if numeric.len() < 2 {
        return out;
    }
    let mut min_left = f32::INFINITY;
    let mut max_left = f32::NEG_INFINITY;
    let mut min_right = f32::INFINITY;
    let mut max_right = f32::NEG_INFINITY;
    let mut max_em = 0.0f32;
    for (_, line) in &numeric {
        let em = structured_line_max_font_size(line);
        if em <= 0.
            || line
                .runs
                .iter()
                .any(|run| !run.style.font_size.is_finite() || run.style.font_size <= 0.)
            || !line.left_x.is_finite()
            || !line.right_x.is_finite()
            || line.left_x < left
            || line.right_x > right
            || line.left_x >= line.right_x
            || right - line.right_x > em
        {
            return out;
        }
        min_left = min_left.min(line.left_x);
        max_left = max_left.max(line.left_x);
        min_right = min_right.min(line.right_x);
        max_right = max_right.max(line.right_x);
        max_em = max_em.max(em);
    }
    // 同じ幅の行は左右どちらの揃えとも解釈できるので変更しない。
    if max_right - min_right <= 0.5 && max_left - min_left >= max_em * 0.5 {
        for (index, _) in numeric {
            out[index] = true;
        }
    }
    out
}

/// 本文を正規化せず、少なくとも一桁を含む短いASCII数値・符号・区切りだけを対象にする。
fn numeric_token(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && text.bytes().any(|b| b.is_ascii_digit())
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b',' | b'.' | b'+' | b'-' | b'%'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 所有する原本の行配置だけを与え、製品が使うfontや字間から期待値を作らない。
    fn line(text: &str, left_x: f32, right_x: f32) -> StructuredTextLine {
        StructuredTextLine {
            top_y: 100.,
            left_x,
            right_x,
            alignment: ParagraphAlignment::Left,
            runs: vec![StructuredTextRun {
                text: text.into(),
                style: StructuredTextStyle::default(),
            }],
            split_from_row: false,
            text: text.into(),
        }
    }

    /// 単位や日本語を混ぜても、右端が揃う可変桁の数値だけへanchorを付ける。
    #[test]
    fn variable_width_numbers_share_right_anchors_but_labels_do_not() {
        let lines = [
            line("円", 80., 98.),
            line(" 9 ", 92., 98.),
            line("1,234", 70., 98.),
        ];
        assert_eq!(right_anchors(&lines, 0., 100.), [false, true, true]);
    }

    /// 同じ幅、単独行、左揃え、中央揃えには右揃えを推測しない。
    #[test]
    fn ambiguous_or_non_right_aligned_numbers_remain_unchanged() {
        for lines in [
            vec![line("9", 90., 98.)],
            vec![line("9", 90., 98.), line("8", 90., 98.)],
            vec![line("9", 10., 18.), line("1,234", 10., 38.)],
            vec![line("9", 46., 54.), line("1,234", 36., 64.)],
        ] {
            assert!(!right_anchors(&lines, 0., 100.).into_iter().any(|x| x));
        }
    }

    /// 原本の行がセルを越える、右端が揃わない、または書体サイズが不明なら全群を変更しない。
    #[test]
    fn invalid_or_inconsistent_source_groups_are_rejected() {
        for bad in [
            line("82", 70., 97.),
            line("82", 70., 101.),
            line("82", -1., 98.),
            line("82", f32::NAN, 98.),
            line("82", 70., 80.),
        ] {
            assert_eq!(
                right_anchors(&[line("9", 92., 98.), bad], 0., 100.),
                [false, false]
            );
        }
        let mut bad = line("82", 70., 98.);
        bad.runs[0].style.font_size = f32::NAN;
        assert_eq!(
            right_anchors(&[line("9", 92., 98.), bad], 0., 100.),
            [false, false]
        );
    }

    /// 同じ右advanceでも字形末尾のsidebearingが違う外接矩形を、font emの狭い範囲で扱う。
    #[test]
    fn small_ink_right_sidebearing_variation_keeps_source_numeric_anchors() {
        let lines=[line("9",92.,98.),line("1,234",70.,97.4)];
        assert_eq!(right_anchors(&lines,0.,100.),[true,true],"small source ink sidebearings must not discard a numeric right anchor");
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]
        /// 字形端の差がemの10%未満なら保持し、それを明確に超える配置差は原点・sizeに依存せず拒否する。
        #[test]
        fn sidebearing_band_scales_with_font_em(origin in 0i16..500,em in 8u16..30,gap_hundredths in 1u16..10) {
            let (origin,em)=(f32::from(origin),f32::from(em));let right=origin+em*8.;let anchor=right-em*0.25;
            let mut lines=[line("9",anchor-em*0.5,anchor),line("1,234",anchor-em*3.,anchor-em*f32::from(gap_hundredths)/100.)];
            for line in &mut lines {line.runs[0].style.font_size=em;}
            prop_assert_eq!(right_anchors(&lines,origin,right),[true,true]);
            lines[1].right_x-=em*0.15;
            prop_assert_eq!(right_anchors(&lines,origin,right),[false,false]);
        }
    }
}
