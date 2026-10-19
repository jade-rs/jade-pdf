//! 可視本文の間に独立して描かれた空白を、行復元とOCR照合で共有する。
use super::*;

/// 同一baselineの本文断片に挟まれ、連続する空白だけを行へ戻す。
pub(super) fn with_interior_spaces<'a>(
    elements: &'a [ParsedTextElement],
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
        .map(|text| text.bounds.y0)
        .fold(f32::INFINITY, f32::min);
    let high = row
        .iter()
        .map(|text| text.bounds.y0)
        .fold(f32::NEG_INFINITY, f32::max);
    row.extend(elements.iter().filter(|text| {
        let b = text.bounds;
        text.page == page
            && is_space(text)
            && [b.x0, b.x1, b.y0, b.y1]
                .iter()
                .all(|value| value.is_finite())
            && b.x0 >= left
            && b.x1 <= right
            && b.x0 < b.x1
            && b.y0 < b.y1
            && high.max(b.y0) - low.min(b.y0) <= TEXT_ROW_Y_TOLERANCE_PT
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
                let height = (pair[0].bounds.y1 - pair[0].bounds.y0)
                    .min(pair[1].bounds.y1 - pair[1].bounds.y0);
                gap >= 0.0 && gap <= height * 0.8
            })
        {
            kept.extend_from_slice(&row[start..index]);
        }
    }
    kept
}

/// 制御文字や空文字を含まない、表示上の空白だけの断片か判定する。
fn is_space(text: &ParsedTextElement) -> bool {
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

    /// 検証用に正立した文字矩形を作る。
    fn fragment(text: &str, x: f32, width: f32) -> ParsedTextElement {
        ParsedTextElement {
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
}
