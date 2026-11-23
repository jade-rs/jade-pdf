//! 原本gridへ割り当てた本文を、セルごとの表示行とstyle付き段落へ戻す。
use super::*;

/// 結合後の一セルについて、原本の原点と表示行を保持する。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Cell {
    /// セル左端のPDF座標。
    left: f32,
    /// セル上端のPDF座標。
    top: f32,
    /// 原本baselineでまとめた表示行。
    lines: Vec<StructuredTextLine>,
}

/// gridの表示順と結合セル範囲に従い、本文の欠落がない複数行セルだけを復元する。
pub(super) fn cells(
    parsed: &ParsedDocument,
    xs: &[i32],
    ys: &[i32],
    texts: &[Vec<Vec<ParsedTextElement>>],
    model: &DocxTableModel,
) -> Vec<Vec<Option<Cell>>> {
    model
        .rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            let mut col = 0;
            row.iter()
                .map(|cell| {
                    let start = col;
                    col += cell.grid_span.max(1);
                    let band = ys.len().checked_sub(row_index + 2)?;
                    let top = *ys.get(band + 1)? as f32;
                    let left = *xs.get(start)? as f32;
                    let selected = texts.get(row_index)?.get(start..col)?;
                    let mut elements = selected.iter().flatten().collect::<Vec<_>>();
                    if elements.is_empty()
                        || cell.vertical_merge == Some(DocxVerticalMerge::Continue)
                    {
                        return None;
                    }
                    let original = elements.iter().map(|t| t.text.as_str()).collect::<String>();
                    if characters(&original) != characters(&cell.text) {
                        return None;
                    }
                    elements.sort_by(|a, b| {
                        native_baseline::line_y(b)
                            .total_cmp(&native_baseline::line_y(a))
                            .then(a.bounds.x0.total_cmp(&b.bounds.x0))
                    });
                    let mut rows: Vec<Vec<&ParsedTextElement>> = Vec::new();
                    for element in elements {
                        if let Some(row) = rows.last_mut().filter(|row| {
                            (native_baseline::line_y(row[0]) - native_baseline::line_y(element))
                                .abs()
                                <= TEXT_ROW_Y_TOLERANCE_PT
                        }) {
                            row.push(element);
                        } else {
                            rows.push(vec![element]);
                        }
                    }
                    let lines = rows
                        .into_iter()
                        .filter_map(|row| {
                            let mut row =
                                native_spacing::with_interior_spaces(&parsed.text_elements, row);
                            row.sort_by(|a, b| a.bounds.x0.total_cmp(&b.bounds.x0));
                            let mut line = source_line(row, &parsed.graphics, *xs.last()? as f32)?;
                            line.alignment = ParagraphAlignment::Left;
                            mark_embedded_fonts_in_line(&mut line, &parsed.embedded_fonts);
                            Some(line)
                        })
                        .collect::<Vec<_>>();
                    (lines.len() > 1
                        && characters(
                            &lines
                                .iter()
                                .map(|line| line.text.as_str())
                                .collect::<String>(),
                        ) == characters(&original)
                        && lines.iter().all(|line| {
                            [line.top_y, line.left_x, line.right_x]
                                .iter()
                                .all(|v| v.is_finite())
                        }))
                    .then_some(Cell { left, top, lines })
                })
                .collect()
        })
        .collect()
}

/// 原点を字面へ移動しないセル行では、共通本文builderがtrimした原文の端の空白を戻す。
fn source_line(
    row: Vec<&ParsedTextElement>,
    graphics: &[ParsedGraphicElement],
    page_width: f32,
) -> Option<StructuredTextLine> {
    let first = (*row.first()?).text.as_str();
    let last = (*row.last()?).text.as_str();
    let prefix = &first[..first.len() - first.trim_start().len()];
    let suffix = &last[last.trim_end().len()..];
    let mut line = structured_text_line_from_segment(
        &TextRowSegment {
            texts: row,
            split_from_row: false,
        },
        graphics,
        page_width,
    )?;
    line.runs.first_mut()?.text.insert_str(0, prefix);
    line.runs.last_mut()?.text.push_str(suffix);
    line.text = line.runs.iter().map(|run| run.text.as_str()).collect();
    Some(line)
}

/// 表示順の復元を許しつつ、旧モデルにある非空白文字の欠落や重複を検出する。
fn characters(text: &str) -> BTreeMap<char, usize> {
    let mut counts = BTreeMap::new();
    for ch in text.chars().filter(|ch| !ch.is_whitespace()) {
        *counts.entry(ch).or_default() += 1;
    }
    counts
}

/// セル既定の余白と中央寄せを重ねず、原本の各表示行を編集可能な段落にする。
pub(super) fn table_cell(cell: &Cell) -> Option<TableCell> {
    let mut out = TableCell::new().vertical_align(VAlignType::Top);
    out.property = out
        .property
        .margin_top(0, WidthType::Dxa)
        .margin_bottom(0, WidthType::Dxa)
        .margin_left(0, WidthType::Dxa)
        .margin_right(0, WidthType::Dxa);
    let mut previous_bottom = cell.top;
    for line in &cell.lines {
        let before = pdf_points_to_twips((previous_bottom - line.top_y).max(0.0))?;
        let height = structured_line_max_font_size(line);
        let paragraph = paragraph_from_structured_line(line, cell.left, None, None).line_spacing(
            native_line_pitch::spacing(height, before.try_into().ok()?, 0)?,
        );
        out = out.add_paragraph(paragraph);
        previous_bottom = line.top_y - height;
    }
    Some(out)
}
