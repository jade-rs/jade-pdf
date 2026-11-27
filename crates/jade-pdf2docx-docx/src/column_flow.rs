//! 採用済みの実段組に、行順と改段位置を一致させる。
use crate::{DetectedTextColumnLayout, StructuredTextLine};

/// 実段組の本文だけを左段から右段へ並べ、全幅の前後行と元の行データを保つ。
pub(super) fn ordered_lines<'a>(
    lines: &'a [StructuredTextLine],
    layout: Option<&DetectedTextColumnLayout>,
) -> Vec<&'a StructuredTextLine> {
    let mut ordered = lines.iter().collect::<Vec<_>>();
    if let Some(layout) = layout {
        ordered[layout.column_start_index()..layout.column_end_index()].sort_by(|a, b| {
            layout
                .column_index(a)
                .cmp(&layout.column_index(b))
                .then(b.top_y.total_cmp(&a.top_y))
                .then(a.left_x.total_cmp(&b.left_x))
        });
    }
    ordered
}
