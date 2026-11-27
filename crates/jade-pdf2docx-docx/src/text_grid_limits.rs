//! 反復列の表推定が扱える原本断片だけを収集する。
use super::ParsedTextElement;

/// 長文ページを反復列だけで巨大表に誤分類しないための断片数境界。
const FRAGMENT_LIMIT: usize = 120;

/// 反復列から表を推定しない行数境界。
const ROW_LIMIT: usize = 25;

/// 反復列から表を推定できる断片群を構築する。
pub(super) fn fragments<'a>(
    texts: impl Iterator<Item = &'a ParsedTextElement>,
) -> Option<Vec<&'a ParsedTextElement>> {
    let texts: Vec<_> = texts.take(FRAGMENT_LIMIT).collect();
    (texts.len() < FRAGMENT_LIMIT).then_some(texts)
}

/// 座標の昇順または降順で、次の断片が属しうる最後の行だけを比較する。
pub(super) fn sorted_rows(texts: Vec<&ParsedTextElement>) -> Option<Vec<Vec<&ParsedTextElement>>> {
    let mut rows: Vec<Vec<&ParsedTextElement>> = Vec::new();
    for text in texts {
        if let Some(row) = rows
            .last_mut()
            .filter(|row| (row[0].bounds.y0 - text.bounds.y0).abs() <= 4.0)
        {
            row.push(text);
        } else {
            if rows.len() == ROW_LIMIT - 1 {
                return None;
            }
            rows.push(vec![text]);
        }
    }
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jade_pdf2docx_parser::ParsedBounds;

    /// 比較用の原本断片。断片数以外の属性は上限の判定に使わない。
    fn source_text() -> ParsedTextElement {
        ParsedTextElement {
            page: 0,
            text: "owned fragment".into(),
            bounds: ParsedBounds {
                x0: 20.,
                y0: 30.,
                x1: 60.,
                y1: 40.,
            },
            baseline_y: Some(30.),
            font_size: 10.,
            font_name: None,
            font_weight: None,
            fill_stroke_width: None,
            fill_color: None,
            source_background: None,
        }
    }

    /// 既存の120断片境界を保ち、拒否確定後に文書の残りを走査しない。
    #[test]
    fn dense_page_stops_at_the_first_rejected_count() {
        let text = source_text();
        assert_eq!(
            fragments(std::iter::repeat_n(&text, 119)).unwrap().len(),
            119
        );
        let mut examined = 0;
        let dense = std::iter::repeat_n(&text, 1000).inspect(|_| examined += 1);
        assert!(fragments(dense).is_none());
        assert_eq!(examined, 120);
    }

    /// 昇順と降順のいずれでも、先頭の断片を基準に4ptまで同じ行へ入れる。
    #[test]
    fn row_limit_uses_the_same_seed_and_distance_in_both_orders() {
        let mut texts: Vec<_> = (0..25)
            .flat_map(|r| [r as f32 * 10., r as f32 * 10. + 4.])
            .map(|y| {
                let mut text = source_text();
                text.bounds.y0 = y;
                text
            })
            .collect();
        for reverse in [false, true] {
            if reverse {
                texts.reverse();
            }
            let rows = sorted_rows(texts[..48].iter().collect()).unwrap();
            assert_eq!(rows.len(), 24);
            assert!(rows.iter().all(|row| row.len() == 2));
            assert!(sorted_rows(texts[..49].iter().collect()).is_none());
        }
    }
}
