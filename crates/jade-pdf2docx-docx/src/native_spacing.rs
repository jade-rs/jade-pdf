//! 可視本文の間に独立して描かれた空白を、行復元とOCR照合で共有する。
use super::*;

/// 行の空白復元へ渡す候補をページごとの原本描画順で保持する。
#[derive(Default)]
pub(super) struct PageSpaces<'a> {
    /// 各ページで独立して描かれた可視空白。
    pages: BTreeMap<usize, Vec<&'a ParsedTextElement>>,
}

impl<'a> PageSpaces<'a> {
    /// 文書を一度走査し、空白だけを複製せず登録する。
    pub(super) fn new(elements: &'a [ParsedTextElement]) -> Self {
        let mut pages = BTreeMap::<_, Vec<_>>::new();
        for element in elements.iter().filter(|element| is_space(element)) {
            pages.entry(element.page).or_default().push(element);
        }
        Self { pages }
    }

    /// 指定ページに属する候補だけを返す。
    pub(super) fn for_page(&self, page: usize) -> &[&'a ParsedTextElement] {
        self.pages.get(&page).map_or(&[], Vec::as_slice)
    }
}

/// 同一baselineの本文断片に挟まれ、連続する空白だけを行へ戻す。
pub(super) fn with_interior_spaces<'a>(
    elements: impl IntoIterator<Item = &'a ParsedTextElement>,
    mut row: Vec<&'a ParsedTextElement>,
) -> Vec<&'a ParsedTextElement> {
    if row.len() < 2 {
        return row;
    }
    let page = row[0].page;
    let left = row
        .iter()
        .map(|text| text.bounds.x0)
        .fold(f32::INFINITY, f32::min);
    let right = row
        .iter()
        .map(|text| text.bounds.x1)
        .fold(f32::NEG_INFINITY, f32::max);
    let low = row
        .iter()
        .map(|text| native_baseline::line_y(text))
        .fold(f32::INFINITY, f32::min);
    let high = row
        .iter()
        .map(|text| native_baseline::line_y(text))
        .fold(f32::NEG_INFINITY, f32::max);
    let mut local = if high - low > TEXT_ROW_Y_TOLERANCE_PT {
        row.clone()
    } else {
        Vec::new()
    };
    local.sort_by(|a, b| a.bounds.x0.total_cmp(&b.bounds.x0));
    row.extend(elements.into_iter().filter(|text| {
        let b = text.bounds;
        let baseline = native_baseline::line_y(text);
        text.page == page
            && is_space(text)
            && [b.x0, b.x1, b.y0, b.y1]
                .iter()
                .all(|value| value.is_finite())
            && b.x0 >= left
            && b.x1 <= right
            && b.x0 < b.x1
            && b.y0 < b.y1
            && (high.max(baseline) - low.min(baseline) <= TEXT_ROW_Y_TOLERANCE_PT
                || matches_adjacent_baseline(&local, text))
    }));
    row.sort_by(|a, b| a.bounds.x0.total_cmp(&b.bounds.x0));
    let mut kept = Vec::with_capacity(row.len());
    let mut index = 0;
    while index < row.len() {
        if !is_space(row[index]) {
            kept.push(row[index]);
            index += 1;
            continue;
        }
        let start = index;
        while index < row.len() && is_space(row[index]) {
            index += 1;
        }
        if start > 0
            && index < row.len()
            && row[start - 1..=index].windows(2).all(|pair| {
                let gap = pair[1].bounds.x0 - pair[0].bounds.x1;
                let Some(left_height) = spacing_height(pair[0]) else {
                    return false;
                };
                let Some(right_height) = spacing_height(pair[1]) else {
                    return false;
                };
                gap >= 0.0 && gap <= left_height.min(right_height) * 0.8
            })
        {
            kept.extend_from_slice(&row[start..index]);
        }
    }
    kept
}

/// 添字で行全体の縦範囲が広い場合も、直近の字に沿う実空白だけを二分探索で確認する。
fn matches_adjacent_baseline(row: &[&ParsedTextElement], space: &ParsedTextElement) -> bool {
    let baseline = native_baseline::line_y(space);
    let next = row.partition_point(|t| t.bounds.x0 < space.bounds.x0);
    [next.checked_sub(1).and_then(|i| row.get(i)), row.get(next)]
        .into_iter()
        .flatten()
        .any(|text| (native_baseline::line_y(text) - baseline).abs() <= TEXT_ROW_Y_TOLERANCE_PT)
}

/// 空白の極薄の字面をfontのem高と区別し、非有限・非正の尺度は採用しない。
fn spacing_height(text: &ParsedTextElement) -> Option<f32> {
    let height = if is_space(text) {
        text.font_size
    } else {
        text.bounds.y1 - text.bounds.y0
    };
    (height.is_finite() && height > 0.0).then_some(height)
}

/// 制御文字や空文字を含まない、表示上の空白だけの断片か判定する。
pub(super) fn is_space(text: &ParsedTextElement) -> bool {
    !text.text.is_empty()
        && text
            .text
            .chars()
            .all(|ch| ch.is_whitespace() && !ch.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 罫線表の行を処理するたびに他ページや可視文字を調べず、元の描画順を保つ。
    #[test]
    fn page_lookup_keeps_only_visible_spaces_in_source_order() {
        let mut other = fragment(" ", 10., 2.);
        other.page = 7;
        let elements = [
            other,
            fragment(" ", 24., 2.),
            fragment("Visible", 10., 10.),
            fragment("\t", 9., 2.),
            fragment("\u{3000}", 13., 2.),
        ];
        let spaces = PageSpaces::new(&elements);
        assert_eq!(spaces.for_page(0), [&elements[1], &elements[4]]);
        assert_eq!(spaces.for_page(7), [&elements[0]]);
        assert!(spaces.for_page(8).is_empty());
    }

    /// 検証用に正立した文字矩形を作る。
    fn fragment(text: &str, x: f32, width: f32) -> ParsedTextElement {
        ParsedTextElement {
            source_background: None,
            fill_stroke_width: None,
            font_weight: None,
            baseline_y: None,
            page: 0,
            bounds: ParsedBounds {
                x0: x,
                x1: x + width,
                y0: 10.0,
                y1: 22.0,
            },
            font_size: 12.0,
            font_name: None,
            fill_color: None,
            text: text.into(),
        }
    }

    /// 行外・別ページ・別baseline・重複・非有限矩形の空白は本文へ混ぜない。
    #[test]
    fn unrelated_spaces_do_not_change_native_row() {
        let left = fragment("a", 10.0, 6.0);
        let right = fragment("b", 22.0, 6.0);
        let space = fragment(" ", 16.0, 6.0);
        let mut candidates = vec![fragment(" ", 0.0, 6.0), fragment(" ", 40.0, 6.0)];
        for (page, y0, x0) in [
            (1, 10.0, 16.0),
            (0, 30.0, 16.0),
            (0, 10.0, 15.0),
            (0, 10.0, f32::NAN),
        ] {
            let mut invalid = space.clone();
            invalid.page = page;
            invalid.bounds.y0 = y0;
            invalid.bounds.y1 = y0 + 12.0;
            invalid.bounds.x0 = x0;
            candidates.push(invalid);
        }
        let row = with_interior_spaces(&candidates, vec![&left, &right]);
        assert_eq!(row.len(), 2);
        assert!(with_interior_spaces(&candidates, vec![]).is_empty());
        assert_eq!(with_interior_spaces(&candidates, vec![&left]).len(), 1);
    }

    proptest! {
        /// 描画順や分割数に依存せず、独立した連続空白をそのまま出力する。
        #[test]
        fn standalone_spaces_preserve_exact_word_boundaries(
            words in prop::collection::vec("[a-z日本語]{1,5}", 2..7),
            spaces in 1usize..4, reverse in any::<bool>(),
        ) {
            let expected = words.join(&" ".repeat(spaces));
            let mut elements = Vec::new();
            let mut x = 10.0;
            for (index, word) in words.iter().enumerate() {
                let width = word.chars().count() as f32 * 6.0;
                elements.push(fragment(word, x, width));
                x += width;
                if index + 1 < words.len() {
                    for _ in 0..spaces {
                        elements.push(fragment(" ", x, 3.0));
                        x += 3.0;
                    }
                }
            }
            if reverse { elements.reverse(); }
            let ink = elements.iter().filter(|text| !is_space(text)).collect();
            let row = with_interior_spaces(&elements, ink);
            let rendered = structured_text_runs_from_row(&row, &[]).into_iter().map(|run| run.text).collect::<String>();
            prop_assert_eq!(rendered, expected);
        }
    }

    /// spaceのSDK字面が薄くても、本文とfont幅に沿った宣言済みの単語境界を消さない。
    #[test]
    fn declared_space_ink_height_does_not_erase_word_boundary() {
        let left = fragment("a", 10.0, 6.0);
        let mut space = fragment(" ", 16.7, 3.0);
        space.bounds.y1 = space.bounds.y0 + 0.012;
        let right = fragment("b", 20.4, 6.0);
        let elements = [space];
        let row = with_interior_spaces(&elements, vec![&left, &right]);
        assert_eq!(
            row.iter().map(|e| e.text.as_str()).collect::<String>(),
            "a b"
        );
    }
    /// 無効なfont高を可視の空白へ昇格させず、元の本文断片を保つ。
    #[test]
    fn invalid_space_font_size_does_not_invent_a_separator() {
        let left = fragment("a", 10.0, 6.0);
        let right = fragment("b", 20.4, 6.0);
        for size in [0.0, -12.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut space = fragment(" ", 16.7, 3.0);
            space.bounds.y1 = space.bounds.y0 + 0.012;
            space.font_size = size;
            let elements = [space];
            let row = with_interior_spaces(&elements, vec![&left, &right]);
            assert_eq!(
                row.iter().map(|e| e.text.as_str()).collect::<String>(),
                "ab"
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(32))]
        /// 添字の上下・原点・描画順を変えても本文と添字の空白を保ち、別基線の空白を拒否する。
        #[test]
        fn mixed_script_baselines_keep_both_source_word_spaces(
            half_points in 9i16..=12, raised in any::<bool>(),
            x in 0i16..500, y in 100i16..700, reverse in any::<bool>(),
        ) {
            let delta = f32::from(half_points) / 2. * if raised { 1. } else { -1. };
            let positioned = |text: &str, left: f32, width: f32, size: f32, shift: f32| {
                let mut t = fragment(text, left + f32::from(x), width);
                t.font_size = size;
                t.baseline_y = Some(f32::from(y) + shift);
                t.bounds.y0 = f32::from(y) + shift - size * 0.2;
                t.bounds.y1 = f32::from(y) + shift + size * 0.8;
                t
            };
            let texts = [
                positioned("Alpha", 10., 25., 10., 0.),
                positioned("Beta", 40., 20., 10., 0.),
                positioned("max", 60.2, 10., 7., delta),
                positioned("v", 72., 4., 7., delta),
                positioned("=B", 78., 14., 10., 0.),
            ];
            let spaces = [
                positioned(" ", 35., 5., 10., 0.),
                positioned(" ", 70.2, 1.8, 7., delta),
                positioned(" ", 35., 5., 10., 20.),
                positioned(" ", 70.2, 1.8, 7., -delta),
            ];
            let mut row = texts.iter().collect::<Vec<_>>();
            if reverse { row.reverse(); }
            let kept = with_interior_spaces(&spaces, row);
            prop_assert_eq!(kept.iter().map(|t| t.text.as_str()).collect::<String>(), "Alpha Betamax v=B");
        }
    }

    proptest! {
        /// 実SDKと同じ極薄の空白でも、emの有界な間隔と元の本文順を保持する。
        #[test]
        fn thin_spaces_keep_word_boundaries(ink in 0.001f32..0.05,gap in 0.01f32..1.0) {
            let left=fragment("a",10.0,6.0);let mut space=fragment(" ",16.0+gap,3.0);space.bounds.y1=space.bounds.y0+ink;
            let right=fragment("b",19.0+gap*2.0,6.0);let elements=[space];
            let row=with_interior_spaces(&elements,vec![&left,&right]);
            prop_assert_eq!(row.iter().map(|e| e.text.as_str()).collect::<String>(),"a b");
        }
    }
}
