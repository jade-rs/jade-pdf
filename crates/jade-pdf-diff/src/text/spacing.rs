//! 字面の左右余白を単語間隔と誤認しないための、限定した水平送りセル。
use crate::Bounds;
#[path = "japanese_data.rs"]
mod japanese_data;

/// 割り当て済みの全角Han/Kana等だけを、固定したUnicode18の資料で確認する。
fn japanese_letter(c: char) -> bool {
    let code = c as u32;
    let at = japanese_data::RANGES.partition_point(|&(_, end)| end < code);
    japanese_data::RANGES
        .get(at)
        .is_some_and(|&(start, end)| start <= code && code <= end)
}

/// 末尾の空白と既知の結合要素を除き、最後の送りセルが所属する基底文字を取得する。
/// 濁点が正準分解されていても、原文の空白や文字列そのものは変更しない。
pub(super) fn last_base(text: &str) -> Option<char> {
    text.chars()
        .rev()
        .find(|c| !c.is_whitespace() && !super::is_combining(*c))
}

/// 文字列の両端だけの送りセル。途中の字体や結合記号から端の幅を推測しない。
#[derive(Clone, Copy, Default)]
pub(super) struct Edges {
    /// 先頭の文字の送りセル。不明なら字面の判定を保持する。
    pub(super) first: Option<Cell>,
    /// 末尾の基底文字の送りセル。不明なら字面の判定を保持する。
    pub(super) last: Option<Cell>,
}

/// 回転・せん断のないLTR文字について確認できた、原点から送り末尾までの区間。
#[derive(Clone, Copy)]
pub(super) struct Cell {
    /// 送りセルの左端。字面の左端ではない。
    start: f64,
    /// 送りセルの右端。公開する字面座標は変更しない。
    end: f64,
}

impl Cell {
    /// SDKのlooseセルが原点を始点とする有限な水平送りの場合だけ利用する。
    pub(super) fn from_loose(
        origin: f64,
        loose: Bounds,
        em: f64,
        matrix: [f64; 4],
    ) -> Option<Self> {
        let [a, b, c, d] = matrix;
        let width = loose.x1 - origin;
        (matrix.into_iter().all(f64::is_finite)
            && [origin, loose.x0, loose.x1, width, em]
                .into_iter()
                .all(f64::is_finite)
            && a > 0.
            && d > 0.
            && b.abs() <= a * 1e-6
            && c.abs() <= d * 1e-6
            && em > 0.
            && width > 0.
            && width <= em * 3.
            && (loose.x0 - origin).abs() <= em * 0.001)
            .then_some(Self {
                start: origin,
                end: loose.x1,
            })
    }

    /// 双方向の入力順で、送りセルが接するか重なることを同じ物理許容差で判定する。
    fn touches(self, other: Self, em: f64) -> bool {
        (other.start - self.end).max(self.start - other.end) <= em * 0.01
    }

    /// 全角の送りを1%以内で照合し、重なりや四分アキを超えた間隔を和字の字間に含めない。
    fn japanese_tracking(self, other: Self, em: f64) -> bool {
        let width = self.end - self.start;
        let other_width = other.end - other.start;
        let gap = (other.start - self.end).max(self.start - other.end);
        em.is_finite()
            && em > 0.
            && (width - em).abs() <= em * 0.01
            && (other_width - em).abs() <= em * 0.01
            && gap >= -em * 0.01
            && gap <= em * 0.25
    }
}

/// 和字同士の、送り幅を確認した四分アキ以内の字間だけを単語の区切りから除く。
/// 原文に書かれた空白には適用せず、空白の削除は読み順の確定まで遅延する。
pub(super) fn japanese_tracking(
    first: Option<char>,
    second: Option<char>,
    a: Option<Cell>,
    b: Option<Cell>,
    em: f64,
) -> bool {
    first
        .zip(second)
        .is_some_and(|(a, b)| japanese_letter(a) && japanese_letter(b))
        && a.zip(b).is_some_and(|(a, b)| a.japanese_tracking(b, em))
}

/// 従来の字面間隔を保ち、接する送りセルが確認できた場合だけ余分な空白を抑える。
pub(super) fn requires_space(
    first: Option<Cell>,
    second: Option<Cell>,
    ink_gap: f64,
    em: f64,
) -> bool {
    ink_gap > em * 0.15 && !first.zip(second).is_some_and(|(a, b)| a.touches(b, em))
}

/// 並び順を確定してから生成空白だけを除き、文字セルと原文領域の添字を同時に詰める。
pub(super) fn finish_order(mut runs: Vec<super::ExtractedRun>) -> Vec<super::ExtractedRun> {
    for run in &mut runs {
        let pending = std::mem::take(&mut run.pending_spaces);
        if pending.is_empty() {
            continue;
        }
        let old_spans = std::mem::take(&mut run.spans);
        let mut text = String::with_capacity(run.text.len());
        let mut spans = Vec::with_capacity(old_spans.len() - pending.len());
        let mut at = 0;
        for (i, c) in run.text.chars().enumerate() {
            if pending.get(at) == Some(&i) {
                debug_assert_eq!(c, ' ');
                at += 1;
            } else {
                text.push(c);
                spans.push(old_spans[i]);
            }
        }
        for region in &mut run.source_regions {
            region.start -= pending.partition_point(|i| *i < region.start);
            region.end -= pending.partition_point(|i| *i < region.end);
        }
        run.text = text;
        run.spans = spans;
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 文字符号だけで約物・欧文・ハングルや未割当を和字の字間へ巻き込まない。
    #[test]
    fn japanese_class_is_assigned_wide_letters_and_explicit_jlreq_additions_only() {
        for c in [
            '申',
            'の',
            'カ',
            'ー',
            '〇',
            '〓',
            '゛',
            '゜',
            '々',
            '\u{20000}',
            '\u{33479}',
        ] {
            assert!(japanese_letter(c), "U+{:X}", c as u32);
        }
        for c in [
            'A',
            'Ａ',
            'ｱ',
            '가',
            '、',
            '。',
            '「',
            ' ',
            '\u{3000}',
            '\u{0378}',
            '\u{e000}',
            '\u{2fa1e}',
            '\u{3347a}',
            '∑',
            '🀄',
        ] {
            assert!(!japanese_letter(c), "U+{:X}", c as u32);
        }
        for pair in japanese_data::RANGES.windows(2) {
            assert!(pair[0].0 <= pair[0].1 && pair[0].1 + 1 < pair[1].0);
        }
    }

    /// 不明な送り幅・過大な空き・欧文やハングルは従来の区切り判定を保持する。
    #[test]
    fn tracking_needs_full_em_verified_cells_and_a_bounded_gap() {
        let a = Cell {
            start: 0.,
            end: 12.,
        };
        let b = Cell {
            start: 14.04,
            end: 26.04,
        };
        assert!(japanese_tracking(
            Some('申'),
            Some('請'),
            Some(a),
            Some(b),
            12.
        ));
        for (first, last, left, right, em) in [
            ('A', 'B', Some(a), Some(b), 12.),
            ('한', '글', Some(a), Some(b), 12.),
            ('申', '請', None, Some(b), 12.),
            ('申', '請', Some(a), None, 12.),
            ('申', '請', Some(a), Some(Cell { end: 20.04, ..b }), 12.),
            (
                '申',
                '請',
                Some(a),
                Some(Cell {
                    start: 16.2,
                    end: 28.2,
                }),
                12.,
            ),
            ('申', '請', Some(a), Some(a), 12.),
            ('申', '請', Some(a), Some(b), f64::NAN),
        ] {
            assert!(!japanese_tracking(Some(first), Some(last), left, right, em));
        }
        for matrix in [[1., 0., 0.2, 1.], [0., 1., -1., 0.], [-1., 0., 0., 1.]] {
            assert!(Cell::from_loose(
                0.,
                Bounds {
                    x0: 0.,
                    y0: 0.,
                    x1: 12.,
                    y1: 12.
                },
                12.,
                matrix
            )
            .is_none());
        }
    }

    /// 不明・非水平・過大・非有限なセルは、従来の字面に基づく判定へ戻す。
    #[test]
    fn unsafe_metrics_do_not_establish_touching_advances() {
        let bounds = Bounds {
            x0: 0.,
            y0: 0.,
            x1: 6.,
            y1: 8.,
        };
        for matrix in [
            [0., 1., -1., 0.],
            [-1., 0., 0., 1.],
            [1., 0., 0.2, 1.],
            [1., 0., 0., 0.],
            [f64::NAN, 0., 0., 1.],
        ] {
            assert!(Cell::from_loose(0., bounds, 12., matrix).is_none());
        }
        for (origin, em, end) in [
            (1., 12., 6.),
            (0., 0., 6.),
            (0., 12., 40.),
            (0., 12., f64::INFINITY),
        ] {
            assert!(
                Cell::from_loose(origin, Bounds { x1: end, ..bounds }, em, [1., 0., 0., 1.])
                    .is_none()
            );
        }
        assert!(requires_space(None, None, 2., 12.));
        assert!(!requires_space(None, None, 1., 12.));
    }

    proptest! {
        /// 和字の字間の判定は物理単位・平行移動・入力順に依存せず、境界を超えた語間を残す。
        #[test]
        fn japanese_tracking_is_unit_translation_and_input_order_covariant(x in -10_000i32..10_000, exponent in -6i32..7, reverse in any::<bool>(),gap in 0.02f64..0.249) {
            let point=2f64.powi(exponent);let em=12.*point;let start=f64::from(x)*point;
            let a=Cell{start,end:start+em};
            let b=Cell{start:start+em*(1.+gap),end:start+em*(2.+gap)};
            let (first,last)=if reverse {(b,a)} else {(a,b)};
            prop_assert!(japanese_tracking(Some('申'),Some('の'),Some(first),Some(last),em));
            let far=Cell{start:start+em*1.35,end:start+em*2.35};
            prop_assert!(!japanese_tracking(Some('申'),Some('の'),Some(a),Some(far),em));
        }

        /// 区間の二分探索を独立した線形照合と比較し、補助文字や区間外の既定値を検査する。
        #[test]
        fn japanese_table_lookup_matches_explicit_intervals(c in any::<char>()) {
            let expected=japanese_data::RANGES.iter().any(|&(a,b)|a<=c as u32&&c as u32<=b);
            prop_assert_eq!(japanese_letter(c),expected);
        }

        /// 平行移動・物理単位・入力順で、接する文字と実際の単語間隔の判定を変えない。
        #[test]
        fn touching_cells_and_real_gaps_are_translation_and_unit_covariant(x in -1_000_000f64..1_000_000., exponent in -10i32..11, reverse in any::<bool>()) {
            let point=2f64.powi(exponent);
            let em=12.*point;
            let cell=|start:f64| Cell::from_loose(start*point,Bounds{x0:start*point,x1:(start+12.)*point,y0:0.,y1:em},em,[1.,0.,0.,1.]).unwrap();
            let a=cell(x);
            for (distance, expected) in [(12.,false),(12.03,false),(18.,true)] {
                let b=cell(x+distance);
                let (a,b)=if reverse{(b,a)}else{(a,b)};
                prop_assert_eq!(requires_space(Some(a),Some(b),6.*point,em),expected);
            }
        }
    }
}
