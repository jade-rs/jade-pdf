//! 確認済みの正立glyphだけを有界な行へ戻し、previewの文字単位の重複除去を防ぐ。
use crate::{ParsedTextElement, decode::DecodeBudget};

/// 従来の断片は借用し、位置付きType1行だけを最大1024文字の作業領域で連結する。
pub(super) fn visit(
    budget: &DecodeBudget,
    elements: &[ParsedTextElement],
    mut candidate: impl FnMut(&str) -> Option<bool>,
) -> Option<()> {
    let mut row = String::new();
    let mut chars = 0usize;
    let mut previous: Option<&ParsedTextElement> = None;
    for element in elements {
        if budget.is_stopped() {
            return None;
        }
        let upright = element.baseline_y.is_some_and(f32::is_finite);
        let derived = element
            .font_name
            .as_deref()
            .is_some_and(|name| name.starts_with("JadeT1-"));
        let logical_space =
            previous.is_some() && element.font_name.is_none() && element.text == " ";
        let group = upright && (derived || logical_space);
        let adjacent = previous.is_none_or(|prior| {
            prior.page == element.page
                && prior.baseline_y == element.baseline_y
                && element.bounds.x0 >= prior.bounds.x0
                && element.bounds.x0 - prior.bounds.x1
                    <= prior.font_size.max(element.font_size) * 0.8
        });
        if !group || !adjacent {
            if !row.is_empty() && !candidate(&row)? {
                return Some(());
            }
            row.clear();
            chars = 0;
            previous = None;
        }
        if !group {
            if !candidate(&element.text)? {
                return Some(());
            }
            continue;
        }
        let text = if row.is_empty() {
            element.text.trim_start()
        } else {
            &element.text
        };
        let end = text
            .char_indices()
            .nth(1024 - chars)
            .map_or(text.len(), |(offset, _)| offset);
        let part = &text[..end];
        crate::text_result_storage::extend(budget, &mut row, part)?;
        chars += part.chars().count();
        previous = Some(element);
        if chars == 1024 {
            if !candidate(&row)? {
                return Some(());
            }
            row.clear();
            chars = 0;
            previous = None;
        }
    }
    if !row.is_empty() {
        candidate(&row)?;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseLimits, ParsedBounds};
    use proptest::prelude::*;

    /// 既存IRを模した正立Type1 glyphを、入力側の所有領域に作る。
    fn glyph(text: &str, x: usize) -> ParsedTextElement {
        ParsedTextElement {
            fill_stroke_width: None,
            font_weight: None,
            baseline_y: Some(80.),
            page: 0,
            bounds: ParsedBounds {
                x0: x as f32,
                x1: x as f32 + 1.,
                y0: 80.,
                y1: 90.,
            },
            font_size: 10.,
            font_name: Some("JadeT1-owned".into()),
            fill_color: None,
            text: text.into(),
        }
    }

    /// 1+2+4+8の有界な作業領域だけを予約し、1 byte不足で候補を公開しない。
    #[test]
    fn row_workspace_has_an_inclusive_shared_budget() {
        let input = (0..8).map(|i| glyph("f", i)).collect::<Vec<_>>();
        for limit in [14, 15] {
            let budget = DecodeBudget::new(&ParseLimits {
                max_total_metadata_workspace_bytes: limit,
                ..Default::default()
            });
            let mut calls = 0;
            let result = visit(&budget, &input, |row| {
                calls += 1;
                assert_eq!(row, "ffffffff");
                Some(true)
            });
            assert_eq!(result.is_some(), limit == 15);
            assert_eq!(calls, usize::from(limit == 15));
            assert_eq!(budget.check().is_ok(), limit == 15);
        }
    }

    /// 同じbudgetで作業領域を作り直しても、初回の容量を消費した状態を引き継ぐ。
    #[test]
    fn row_workspace_recreation_is_cumulative() {
        let input = (0..8).map(|i| glyph("f", i)).collect::<Vec<_>>();
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: 29,
            ..Default::default()
        });
        assert!(visit(&budget, &input, |_| Some(true)).is_some());
        assert!(visit(&budget, &input, |_| panic!("不足した行を公開しない")).is_none());
        assert!(budget.is_stopped());
        assert!(visit(&budget, &input, |_| panic!("停止後に候補へ進まない")).is_none());
    }

    proptest! {
        /// glyph分割数によらずUTF-8境界を守り、最大1024文字で行作業領域を止める。
        #[test]
        fn unicode_rows_are_bounded_without_dropping_repeated_characters(count in 1usize..1500) {
            let input=(0..count).map(|i|glyph("文𝄞",i)).collect::<Vec<_>>();
            let expected="文𝄞".repeat(count).chars().take(1024).collect::<String>();
            let budget=DecodeBudget::new(&ParseLimits {max_total_metadata_workspace_bytes:8192,..Default::default()});
            let mut calls=0;
            let result=visit(&budget,&input,|row| {calls+=1;assert_eq!(row,expected);Some(false)});
            prop_assert!(result.is_some());prop_assert_eq!(calls,1);prop_assert!(budget.check().is_ok());
        }
    }
}
