//! 独立した実線の閉じた枠が反復する場合だけ、空白帯を保った物理gridへ対応させる。
use super::*;

/// 閉じた枠の角を比較する座標誤差。本文の近接や欠けた辺の補完には使わない。
const EDGE_EPSILON: f32 = 0.01;

/// 元の各辺が完全な黒い長方形だけを、同じ列配置が反復する一つの表として復元する。
pub(super) fn infer(
    parsed: &ParsedDocument,
    page: usize,
    graphics: &[ParsedGraphicElement],
) -> Option<PositionedTable> {
    if !(24..=4096).contains(&graphics.len()) {
        return None;
    }
    let mut boxes = split_graphics_into_components_with_tolerance(graphics, 0.)
        .iter()
        .map(|component| closed_box(component))
        .collect::<Option<Vec<_>>>()?;
    let stroke = graphics.first()?.stroke_width;
    if !stroke.is_finite() || stroke <= 0. || graphics.iter().any(|g| g.stroke_width != stroke) {
        return None;
    }
    boxes.sort_by(|a, b| b.y1.total_cmp(&a.y1).then(a.x0.total_cmp(&b.x0)));
    let mut rows: Vec<Vec<ParsedBounds>> = Vec::new();
    for bounds in boxes {
        if let Some(row) = rows
            .last_mut()
            .filter(|row| same(row[0].y0, bounds.y0) && same(row[0].y1, bounds.y1))
        {
            row.push(bounds);
        } else {
            rows.push(vec![bounds]);
        }
    }
    let columns = rows.first()?;
    if !(3..=128).contains(&rows.len())
        || !(2..=16).contains(&columns.len())
        || columns.windows(2).any(|p| p[0].x1 >= p[1].x0)
        || rows.iter().any(|row| {
            row.len() != columns.len()
                || row
                    .iter()
                    .zip(columns)
                    .any(|(a, b)| !same(a.x0, b.x0) || !same(a.x1, b.x1))
        })
        || rows.windows(2).any(|p| {
            let gap = p[0][0].y0 - p[1][0].y1;
            gap <= 0. || gap > (p[0][0].y1 - p[0][0].y0).min(p[1][0].y1 - p[1][0].y0)
        })
    {
        return None;
    }
    let xs = edges(columns.iter().flat_map(|b| [b.x0, b.x1]))?;
    let ys = edges(rows.iter().flat_map(|r| [r[0].y0, r[0].y1]))?;
    let width = xs.len() - 1;
    let height = ys.len() - 1;
    let bounds = ParsedBounds {
        x0: xs[0] as f32,
        x1: *xs.last()? as f32,
        y0: ys[0] as f32,
        y1: *ys.last()? as f32,
    };
    let mut texts = vec![vec![Vec::<ParsedTextElement>::new(); width]; height];
    for text in parsed
        .text_elements
        .iter()
        .filter(|t| t.page == page && !t.text.trim().is_empty())
    {
        let b = text.bounds;
        if ![b.x0, b.x1, b.y0, b.y1].iter().all(|n| n.is_finite()) || b.x1 <= b.x0 || b.y1 <= b.y0 {
            return None;
        }
        let cx = (b.x0 + b.x1) / 2.;
        let cy = (b.y0 + b.y1) / 2.;
        if b.x1 <= bounds.x0 || b.x0 >= bounds.x1 || b.y1 <= bounds.y0 || b.y0 >= bounds.y1 {
            continue;
        }
        let col = interval(cx, &xs)?;
        let row = height - 1 - interval(cy, &ys)?;
        // 実データ行の外・縦の空白帯・複数の物理セルに跨る文字は所有が一意でない。
        if !rows.iter().any(|r| r[0].y0 <= cy && cy <= r[0].y1)
            || b.x0 < xs[col] as f32 - EDGE_EPSILON
            || b.x1 > xs[col + 1] as f32 + EDGE_EPSILON
            || b.y0 < ys[height - row - 1] as f32 - EDGE_EPSILON
            || b.y1 > ys[height - row] as f32 + EDGE_EPSILON
        {
            return None;
        }
        texts[row][col].push(text.clone());
    }
    // 空欄のある枠も保持する。反復の根拠は、各行に二つ以上の値と、各枠列に二行以上の値。
    let mut populated_columns = vec![0; columns.len()];
    for row in &rows {
        let mut populated = 0;
        for (index, b) in row.iter().enumerate() {
            let col = interval((b.x0 + b.x1) / 2., &xs)?;
            let band = height - 1 - interval((b.y0 + b.y1) / 2., &ys)?;
            if !texts[band][col].is_empty() {
                populated += 1;
                populated_columns[index] += 1;
            }
        }
        if populated < 2 {
            return None;
        }
    }
    if populated_columns.iter().any(|count| *count < 2) {
        return None;
    }
    let cells = texts
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| {
                    native_table_text::source_rows(cell.iter().collect(), true)
                        .iter()
                        .map(|line| {
                            let mut s = String::new();
                            for t in line {
                                append_cell_fragment(&mut s, t.text.trim());
                            }
                            s
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let model = DocxTableModel {
        rows: cells
            .iter()
            .enumerate()
            .map(|(row, values)| {
                values
                    .iter()
                    .enumerate()
                    .map(|(col, text)| {
                        let b = ParsedBounds {
                            x0: xs[col] as f32,
                            x1: xs[col + 1] as f32,
                            y0: ys[height - row - 1] as f32,
                            y1: ys[height - row] as f32,
                        };
                        DocxTableCellModel {
                            text: text.clone(),
                            underlined: false,
                            alignment: ParagraphAlignment::Left,
                            vertical_alignment: None,
                            grid_span: 1,
                            vertical_merge: None,
                            width_twips: pdf_points_to_twips(b.x1 - b.x0),
                            borders: Some(borders(b, graphics, stroke)),
                        }
                    })
                    .collect()
            })
            .collect(),
    };
    let native_cells = native_table_text::boxed_cells(parsed, &xs, &ys, &texts, &model);
    let fonts = texts
        .iter()
        .flatten()
        .flatten()
        .filter_map(|t| t.font_name.as_deref());
    let font_name = common_usable_pdf_font_name(fonts);
    let font = font_name
        .as_deref()
        .and_then(|name| parsed.embedded_fonts.iter().find(|f| f.font_name == name));
    Some(PositionedTable {
        page,
        bounds,
        stroke_width: stroke,
        font_name: font_name.clone(),
        font_embedded: font.is_some(),
        embedded_face: font.and_then(|f| embedded_face::Face::read(&f.bytes)),
        font_size: common_usable_pdf_font_size(
            texts.iter().flatten().flatten().map(|t| t.font_size),
        ),
        column_widths_twips: table_column_widths_twips(&xs),
        row_heights_twips: native_table_rows::heights(&ys, 0.),
        table: OcrTable::new(cells),
        cell_underlines: vec![vec![false; width]; height],
        cell_alignments: vec![vec![ParagraphAlignment::Left; width]; height],
        docx_table: Some(model),
        native_cells,
        nested_tables: Vec::new(),
        cell_vertical_alignments: Vec::new(),
        row_heights_pt: Vec::new(),
        row_line_counts: Vec::new(),
        cell_padding_pt: None,
        cell_font_sizes: Vec::new(),
        cell_tab_stops: Vec::new(),
        cell_fragments: Vec::new(),
        cell_shading: Vec::new(),
    })
}

/// 四本の完全な辺だけからなる連結成分を確認し、開いた枠や部分罫線を拒否する。
fn closed_box(graphics: &[ParsedGraphicElement]) -> Option<ParsedBounds> {
    if graphics.len() != 4 {
        return None;
    }
    let b = graphics_bounds(graphics)?;
    if b.x1 <= b.x0 || b.y1 <= b.y0 || ![b.x0, b.x1, b.y0, b.y1].iter().all(|n| n.is_finite()) {
        return None;
    }
    let expected = [
        ParsedBounds {
            x0: b.x0,
            x1: b.x1,
            y0: b.y0,
            y1: b.y0,
        },
        ParsedBounds {
            x0: b.x0,
            x1: b.x1,
            y0: b.y1,
            y1: b.y1,
        },
        ParsedBounds {
            x0: b.x0,
            x1: b.x0,
            y0: b.y0,
            y1: b.y1,
        },
        ParsedBounds {
            x0: b.x1,
            x1: b.x1,
            y0: b.y0,
            y1: b.y1,
        },
    ];
    let mut seen = [false; 4];
    for g in graphics {
        let g = normalized_bounds(g.bounds);
        let i = expected.iter().position(|e| {
            same(g.x0, e.x0) && same(g.x1, e.x1) && same(g.y0, e.y0) && same(g.y1, e.y1)
        })?;
        if seen[i] {
            return None;
        }
        seen[i] = true;
    }
    Some(b)
}

/// 同じ原本座標だけを同一辺とみなし、欠けた線を延長しない。
fn same(a: f32, b: f32) -> bool {
    (a - b).abs() <= EDGE_EPSILON
}

/// 既存gridのpoint量子化で違う辺が潰れる場合は表復元を見送る。
fn edges(values: impl Iterator<Item = f32>) -> Option<Vec<i32>> {
    let mut v = values.collect::<Vec<_>>();
    v.sort_by(f32::total_cmp);
    v.dedup_by(|a, b| same(*a, *b));
    if v.iter().any(|n| {
        !n.is_finite() || *n < i32::MIN as f32 || *n >= i32::MAX as f32 || !same(*n, n.round())
    }) {
        return None;
    }
    let out = v.iter().map(|n| n.round() as i32).collect::<Vec<_>>();
    out.windows(2).all(|p| p[0] < p[1]).then_some(out)
}

/// 境界上の中心を隣のセルへ丸めず、開区間内で一意の物理帯だけを返す。
fn interval(value: f32, edges: &[i32]) -> Option<usize> {
    edges
        .windows(2)
        .position(|p| value > p[0] as f32 && value < p[1] as f32)
}

/// 現存する一つの原本辺がセルの一辺を覆う場合だけ黒罫線を付ける。
fn borders(cell: ParsedBounds, graphics: &[ParsedGraphicElement], stroke: f32) -> DocxCellBorders {
    let horizontal = |y: f32| {
        graphics.iter().any(|g| {
            let b = normalized_bounds(g.bounds);
            same(b.y0, b.y1)
                && same(b.y0, y)
                && b.x0 <= cell.x0 + EDGE_EPSILON
                && b.x1 >= cell.x1 - EDGE_EPSILON
        })
    };
    let vertical = |x: f32| {
        graphics.iter().any(|g| {
            let b = normalized_bounds(g.bounds);
            same(b.x0, b.x1)
                && same(b.x0, x)
                && b.y0 <= cell.y0 + EDGE_EPSILON
                && b.y1 >= cell.y1 - EDGE_EPSILON
        })
    };
    DocxCellBorders {
        top: horizontal(cell.y1),
        left: vertical(cell.x0),
        bottom: horizontal(cell.y0),
        right: vertical(cell.x1),
        size_eighths_pt: table_border_size_for_pdf_rule(stroke),
        dashed: [false; 4],
    }
}
