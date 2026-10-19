//! 部分文字レイヤと領域OCRの一致を検証し、本文の二重化を防ぐ。
use super::*;
use jade_pdf2docx_parser::ParsedBounds;

/// 交差するnative文字を座標付きOCRの一行と照合し、一致行だけを省く。
/// 同じ行の連続した文字断片は統合し、食い違い・認識漏れはエラーにする。
pub(super) fn reconcile(
    parsed: &ParsedDocument,
    page: usize,
    region: ParsedBounds,
    lines: Vec<String>,
    positions: Option<Vec<ParsedBounds>>,
) -> Result<Vec<StructuredTextLine>, ConvertError> {
    let mut native = parsed.text_elements.iter().filter(|text| {
        text.page == page
            && jade_pdf2docx_parser::text_is_visible(parsed, text)
            && overlaps(text.bounds, region)
    });
    let Some(positions) = positions else {
        if native.next().is_some() {
            return Err(ambiguous(page));
        }
        return Ok(region_ocr::recognized_lines(region, lines));
    };
    let mut matched: BTreeMap<usize, Vec<&ParsedTextElement>> = BTreeMap::new();
    for text in native {
        let mut candidates = positions
            .iter()
            .enumerate()
            .filter(|(_, bounds)| overlaps(text.bounds, **bounds));
        let Some((index, _)) = candidates.next() else {
            return Err(ambiguous(page));
        };
        if candidates.next().is_some() {
            return Err(ambiguous(page));
        }
        matched.entry(index).or_default().push(text);
    }
    for (&index, row) in &mut matched {
        *row = native_spacing::with_interior_spaces(&parsed.text_elements, std::mem::take(row));
        if !matching_row(row, positions[index], &lines[index]) {
            return Err(ambiguous(page));
        }
    }
    Ok(lines
        .into_iter()
        .zip(positions)
        .enumerate()
        .filter(|(index, _)| !matched.contains_key(index))
        .map(|(_, (text, bounds))| region_ocr::structured_line(bounds, text))
        .collect())
}

/// nativeの行構築と同じ空白を保持し、連続する同一行だけを照合する。
fn matching_row(row: &mut [&ParsedTextElement], bounds: ParsedBounds, text: &str) -> bool {
    row.sort_by(|a, b| a.bounds.x0.total_cmp(&b.bounds.x0));
    let Some(first) = row.first() else {
        return false;
    };
    let mut union = first.bounds;
    let mut highest_baseline = first.bounds.y0;
    for pair in row.windows(2) {
        let [left, right] = pair else { unreachable!() };
        let gap = right.bounds.x0 - left.bounds.x1;
        let height = (left.bounds.y1 - left.bounds.y0)
            .abs()
            .min((right.bounds.y1 - right.bounds.y0).abs());
        // 別行・重複描画・離れた列は、文字列が一致しても結合しない。
        highest_baseline = highest_baseline.max(right.bounds.y0);
        union.y0 = union.y0.min(right.bounds.y0);
        if highest_baseline - union.y0 > TEXT_ROW_Y_TOLERANCE_PT || gap < 0.0 || gap > height * 0.8
        {
            return false;
        }
        union.x1 = union.x1.max(right.bounds.x1);
        union.y1 = union.y1.max(right.bounds.y1);
    }
    aligned(union, bounds)
        && row
            .iter()
            .enumerate()
            .flat_map(|(index, element)| row_fragment_text(&element.text, index, row.len()).chars())
            .eq(text.trim().chars())
}

/// 不明確な本文を公開せず、呼出元が再確認できるエラーを返す。
fn ambiguous(page: usize) -> ConvertError {
    ConvertError::Ocr(format!(
        "page {}: partial text layer requires matching positioned OCR lines",
        page + 1
    ))
}

/// 正の面積を持つ交差だけを認め、境界への接触は一致としない。
fn overlaps(a: ParsedBounds, b: ParsedBounds) -> bool {
    a.x0.min(a.x1).max(b.x0.min(b.x1)) < a.x0.max(a.x1).min(b.x0.max(b.x1))
        && a.y0.min(a.y1).max(b.y0.min(b.y1)) < a.y0.max(a.y1).min(b.y0.max(b.y1))
}

/// 推定文字矩形とOCRのink矩形のずれを許容しつつ、僅かな交差を除く。
fn aligned(a: ParsedBounds, b: ParsedBounds) -> bool {
    let width = a.x0.max(a.x1).min(b.x0.max(b.x1)) - a.x0.min(a.x1).max(b.x0.min(b.x1));
    let height = a.y0.max(a.y1).min(b.y0.max(b.y1)) - a.y0.min(a.y1).max(b.y0.min(b.y1));
    width >= (a.x1 - a.x0).abs().max((b.x1 - b.x0).abs()) * 0.5
        && height >= (a.y1 - a.y0).abs().min((b.y1 - b.y0).abs()) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 照合用に同じ高さ・書式のnative断片を作る。
    fn fragment(text: String, x: f32, y: f32, width: f32) -> ParsedTextElement {
        ParsedTextElement {
            page: 0,
            bounds: ParsedBounds {
                x0: x,
                y0: y,
                x1: x + width,
                y1: y + 12.0,
            },
            font_size: 12.0,
            font_name: Some("Helvetica".into()),
            fill_color: None,
            text,
        }
    }

    /// 左端のbaselineに近くても、行全体の差がnative行分割を超えたら拒否する。
    #[test]
    fn fragmented_baseline_span_must_fit_one_native_row() {
        let elements = [
            fragment("a".into(), 0.0, 10.0, 30.0),
            fragment("b".into(), 30.0, 13.0, 30.0),
            fragment("c".into(), 60.0, 7.0, 30.0),
        ];
        let mut row = elements.iter().collect::<Vec<_>>();
        assert!(!matching_row(
            &mut row,
            ParsedBounds {
                x0: 0.0,
                y0: 7.0,
                x1: 90.0,
                y1: 25.0
            },
            "abc"
        ));
    }

    /// 本文が一致しても重複描画・遠い列・別baselineを結合しない。
    #[test]
    fn fragmented_geometry_rejects_ambiguous_rows() {
        for (x, y) in [(20.0, 10.0), (50.0, 10.0), (30.0, 15.0)] {
            let elements = [
                fragment("a".into(), 0.0, 10.0, 30.0),
                fragment("b".into(), x, y, 30.0),
            ];
            assert!(!matching_row(
                &mut elements.iter().collect::<Vec<_>>(),
                ParsedBounds {
                    x0: 0.0,
                    y0: 10.0,
                    x1: x + 30.0,
                    y1: y + 12.0
                },
                "ab"
            ));
        }
    }

    proptest! {
        /// Unicode断片の分割数によらず本文と内部空白を保ち、文字不一致は拒否する。
        #[test]
        fn fragmented_rows_preserve_text_and_rendered_spacing(
            words in prop::collection::vec("[a-z日本語]{1,8}", 2..8),
            spaces in 0usize..3, reverse in any::<bool>(), spaces_on_right in any::<bool>(),
        ) {
            let expected = words.join(&" ".repeat(spaces));
            let mut x = 0.0;
            let elements = words.iter().enumerate().map(|(index, word)| {
                let leading = if spaces_on_right && index > 0 { spaces } else { 0 };
                let trailing = if !spaces_on_right && index + 1 < words.len() { spaces } else { 0 };
                let text = format!("{}{word}{}", " ".repeat(leading), " ".repeat(trailing));
                let width = text.chars().count() as f32 * 6.0;
                let element = fragment(text, x, 10.0, width);
                x += width;
                element
            }).collect::<Vec<_>>();
            let mut row = elements.iter().collect::<Vec<_>>();
            if reverse { row.reverse(); }
            let bounds = ParsedBounds { x0: 0.0, y0: 10.0, x1: x, y1: 22.0 };
            prop_assert!(matching_row(&mut row, bounds, &expected));
            let actual = structured_text_runs_from_row(&row, &[]).into_iter().map(|run| run.text).collect::<String>();
            prop_assert_eq!(actual.as_str(), expected.as_str());
            let mismatch = format!("{expected}!");
            prop_assert!(!matching_row(&mut row, bounds, &mismatch));
        }
        /// 同じ本文矩形は原点と向きに依存せず一致し、別行・僅かな交差は除く。
        #[test]
        fn reconciliation_geometry_preserves_position_identity(
            x in -1000i32..1000, y in -1000i32..1000,
            width in 4i32..200, height in 4i32..100,
            reverse_x in any::<bool>(), reverse_y in any::<bool>(),
        ) {
            let native = ParsedBounds { x0: x as f32, y0: y as f32, x1: (x + width) as f32, y1: (y + height) as f32 };
            let mut same = native;
            if reverse_x { std::mem::swap(&mut same.x0, &mut same.x1); }
            if reverse_y { std::mem::swap(&mut same.y0, &mut same.y1); }
            prop_assert!(overlaps(native, same) && aligned(native, same));
            let next_row = ParsedBounds { y0: (y + height) as f32, y1: (y + 2 * height) as f32, ..native };
            prop_assert!(!overlaps(native, next_row) && !aligned(native, next_row));
            let sliver = ParsedBounds { x0: (x + width - 1) as f32, x1: (x + 2 * width - 1) as f32, ..native };
            prop_assert!(overlaps(native, sliver) && !aligned(native, sliver));
        }
    }
}
