//! 原本gridへ割り当てた本文を、セルごとの表示行とstyle付き段落へ戻す。
use super::*;

/// 結合後の一セルについて、原本の原点と表示行を保持する。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Cell {
    /// 原本文字の背後の確認済み塗りがセル全体を覆う場合だけ保持するRGB。
    background: Option<[u8; 3]>,
    /// セル左端のPDF座標。
    left: f32,
    /// セル右端。原本の文字が越える場合は一行の水平原点を補正しない。
    right: f32,
    /// セル上端のPDF座標。
    top: f32,
    /// 原本gridが割り当てたセル下端。段落列が収まらないときは旧配置へ戻す。
    bottom: f32,
    /// 原本baselineでまとめた表示行。
    lines: Vec<StructuredTextLine>,
    /// 単一行セルの既存本文と完全に一致する、描画順のstyle付きrun。
    single_line_runs: Vec<StructuredTextRun>,
    /// 複数行の配置が収まらない場合も、旧本文と完全一致するrunの書式は保持する。
    fallback_runs: Vec<StructuredTextRun>,
    /// 原本の数値群で確認した右寄せ。分割済みなら一行でも元の上端と余白を使う。
    logical_row_right_anchor: Option<bool>,
    /// 一行の独立した文字群へだけ設定する、セル左端からの確認済みタブ位置。
    source_tabs: Option<Vec<Option<usize>>>,
}

/// 従来の整数gridを用いるprivate結合回帰の入口を保持する。
#[cfg(test)]
pub(super) fn cells(
    parsed: &ParsedDocument,
    xs: &[i32],
    ys: &[i32],
    texts: &[Vec<Vec<ParsedTextElement>>],
    model: &DocxTableModel,
) -> Vec<Vec<Option<Cell>>> {
    cells_impl(parsed, xs, ys, texts, model, false, None)
}

/// セル所有は既存整数gridで固定し、確認済み原本境界だけを行内配置へ渡す。
pub(super) fn cells_with_geometry(
    parsed: &ParsedDocument,
    xs: &[i32],
    ys: &[i32],
    texts: &[Vec<Vec<ParsedTextElement>>],
    model: &DocxTableModel,
    geometry: Option<&fractional_table_rules::Grid>,
) -> Vec<Vec<Option<Cell>>> {
    cells_impl(parsed, xs, ys, texts, model, false, geometry)
}

/// 全非空セルの本文とfontを確認し、未確認の代替字幅を原本の狭い寸法へ押し込まない。
pub(super) fn fractional_geometry_is_safe(
    cells: &[Vec<Option<Cell>>],
    model: &DocxTableModel,
) -> bool {
    cells.len() == model.rows.len()
        && cells.iter().zip(&model.rows).all(|(cells, row)| {
            cells.len() == row.len()
                && cells.iter().zip(row).all(|(cell, source)| {
                    if source.text.trim().is_empty()
                        || source.vertical_merge == Some(DocxVerticalMerge::Continue)
                    {
                        return true;
                    }
                    let Some(cell) = cell else { return false };
                    !cell.lines.is_empty()
                        && line_spacings(cell)
                            .is_some_and(|mut spacings| spacings.all(|s| s.is_some()))
                        && cell.lines.iter().all(|line| {
                            line.runs.iter().all(|run| {
                                run.text.trim().is_empty()
                                    || (run.style.font_embedded
                                        && run.style.embedded_face.is_some())
                                    || compatible_ascii_metrics(run, line, cell)
                            })
                        })
                })
        })
}

/// Standard14 Helveticaの単純ASCII・無変換の文字高だけを既知の代替経路として許す。
fn compatible_ascii_metrics(
    run: &StructuredTextRun,
    line: &StructuredTextLine,
    cell: &Cell,
) -> bool {
    let style = &run.style;
    style.font_name.as_deref() == Some("Helvetica")
        && !style.font_embedded
        && style.fill_stroke_width.is_none()
        && style.font_weight.is_none_or(|weight| weight <= 500)
        && style.font_size.is_finite()
        && style.font_size > 0.
        && (style.font_size * 2.).fract() == 0.
        && run.text.bytes().all(|c| (b' '..=b'~').contains(&c))
        && line.left_x >= cell.left
        && line.right_x <= cell.right
}

/// 全罫線が独立した閉じた枠と確認済みのセルでは、枠を下線とせず小さい添字行を復元する。
pub(super) fn boxed_cells(
    parsed: &ParsedDocument,
    xs: &[i32],
    ys: &[i32],
    texts: &[Vec<Vec<ParsedTextElement>>],
    model: &DocxTableModel,
) -> Vec<Vec<Option<Cell>>> {
    cells_impl(parsed, xs, ys, texts, model, true, None)
}

/// 元のgridに従うセル復元を共有し、添字の復元は呼出側の確認済み範囲に限定する。
fn cells_impl(
    parsed: &ParsedDocument,
    xs: &[i32],
    ys: &[i32],
    texts: &[Vec<Vec<ParsedTextElement>>],
    model: &DocxTableModel,
    closed_boxes: bool,
    geometry: Option<&fractional_table_rules::Grid>,
) -> Vec<Vec<Option<Cell>>> {
    let family = layout_space::family(&parsed.embedded_fonts, &parsed.text_elements);
    let page = texts
        .iter()
        .flatten()
        .flatten()
        .next()
        .map(|text| text.page);
    let legacy_graphics = page
        .map(|page| table_rule_topology::page(parsed, page))
        .unwrap_or_default();
    // 添字復元を許す呼出元は全graphicが閉じた枠と確認済みであり、枠の下辺は文字下線ではない。
    let styling_graphics = if closed_boxes {
        &[][..]
    } else {
        legacy_graphics.as_slice()
    };
    // 一行・複数行・旧配置のいずれも、確認済みgridと同じ端点・高さの線を文字下線にしない。
    let text_graphics = styling_graphics
        .iter()
        .filter(|graphic| !is_grid_border(graphic, xs, ys))
        .cloned()
        .collect::<Vec<_>>();
    let edge_width = legacy_graphics
        .iter()
        .filter(|g| {
            Some(g.page) == page
                && is_grid_border(g, xs, ys)
                && g.stroke_width.is_finite()
                && g.stroke_width > 0.
        })
        .map(|g| g.stroke_width)
        .fold(0., f32::max);
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
                    if cell.vertical_merge == Some(DocxVerticalMerge::Continue) {
                        return None;
                    }
                    let row_span = if cell.vertical_merge == Some(DocxVerticalMerge::Restart) {
                        vertical_merge_row_span(model, row_index, start, model.rows.len())
                    } else {
                        1
                    };
                    // 段落の所有は結合矩形全体。継続先が異なる横幅を持つ不正なモデルでは推測しない。
                    for next in row_index + 1..row_index.checked_add(row_span)? {
                        let continued = docx_model_cell_at_column(model, next, start)?;
                        if continued.grid_span.max(1) != cell.grid_span.max(1) {
                            return None;
                        }
                    }
                    let band = ys.len().checked_sub(row_index + 2)?;
                    let bottom_band = ys
                        .len()
                        .checked_sub(row_index.checked_add(row_span)?.checked_add(1)?)?;
                    let top = geometry.map_or(*ys.get(band + 1)? as f32, |g| g.ys[band + 1]);
                    let bottom =
                        geometry.map_or(*ys.get(bottom_band)? as f32, |g| g.ys[bottom_band]);
                    let left = geometry.map_or(*xs.get(start)? as f32, |g| g.xs[start]);
                    let right = geometry.map_or(*xs.get(col)? as f32, |g| g.xs[col]);
                    let selected = texts
                        .get(row_index..row_index.checked_add(row_span)?)?
                        .iter()
                        .map(|row| row.get(start..col))
                        .collect::<Option<Vec<_>>>()?;
                    let elements = selected
                        .iter()
                        .flat_map(|row| row.iter().flatten())
                        .collect::<Vec<_>>();
                    if cell.text.trim().is_empty() {
                        let background = source_cell_background::empty_color(
                            parsed,
                            page?,
                            ParsedBounds {
                                x0: left,
                                y0: bottom,
                                x1: right,
                                y1: top,
                            },
                            edge_width,
                        )?;
                        return Some(Cell {
                            background: Some(background),
                            left,
                            right,
                            top,
                            bottom,
                            lines: Vec::new(),
                            single_line_runs: Vec::new(),
                            fallback_runs: Vec::new(),
                            logical_row_right_anchor: None,
                            source_tabs: None,
                        });
                    }
                    if elements.is_empty() {
                        return None;
                    }
                    let original = elements.iter().map(|t| t.text.as_str()).collect::<String>();
                    if characters(&original) != characters(&cell.text) {
                        return None;
                    }
                    let background = source_cell_background::color(
                        &elements,
                        ParsedBounds {
                            x0: left,
                            y0: bottom,
                            x1: right,
                            y1: top,
                        },
                        edge_width,
                    );
                    let rows = source_rows(elements, closed_boxes);
                    // 同じ字形・fontでも別断片の位置を混同せず、確認済み行内の基線差だけを渡す。
                    let offsets = rows
                        .iter()
                        .flat_map(|row| row.iter().copied().zip(inline_baseline::offsets(row)))
                        .filter_map(|(element, offset)| {
                            offset.map(|value| (std::ptr::from_ref(element), value))
                        })
                        .collect::<BTreeMap<_, _>>();
                    let single_physical_line =
                        rows.len() == 1 && row_span == 1 && cell.grid_span == 1;
                    let mut source_tabs = None;
                    let lines = rows
                        .into_iter()
                        .filter_map(|row| {
                            let mut row =
                                native_spacing::with_interior_spaces(&parsed.text_elements, row);
                            row.sort_by(|a, b| a.bounds.x0.total_cmp(&b.bounds.x0));
                            let mut line =
                                source_line(row.clone(), &text_graphics, *xs.last()? as f32)?;
                            line.alignment = ParagraphAlignment::Left;
                            mark_embedded_fonts_in_line(&mut line, &parsed.embedded_fonts, &family);
                            if single_physical_line && !closed_boxes {
                                source_tabs = source_cell_tab_stops::positions(
                                    &row,
                                    &line,
                                    left,
                                    right,
                                    &parsed.embedded_fonts,
                                );
                            }
                            Some(line)
                        })
                        .collect::<Vec<_>>();
                    (!lines.is_empty()
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
                    .then(|| Cell {
                        background,
                        logical_row_right_anchor: None,
                        source_tabs,
                        left,
                        right,
                        top,
                        bottom,
                        single_line_runs: if closed_boxes && lines.len() == 1 {
                            lines[0].runs.clone()
                        } else if lines.len() == 1 {
                            legacy_runs(
                                parsed,
                                selected.iter().flat_map(|row| row.iter().flatten()),
                                &cell.text,
                                &text_graphics,
                                &offsets,
                            )
                        } else {
                            Vec::new()
                        },
                        fallback_runs: if lines.len() > 1 {
                            legacy_runs(
                                parsed,
                                selected.iter().flat_map(|row| row.iter().flatten()),
                                &cell.text,
                                &text_graphics,
                                &offsets,
                            )
                        } else {
                            Vec::new()
                        },
                        lines,
                    })
                })
                .collect()
        })
        .collect()
}

/// 元の行分割を保ち、確認済み局所基線だけから添字を同じセル内の行へ戻す。
pub(super) fn source_rows(
    mut elements: Vec<&ParsedTextElement>,
    scripts: bool,
) -> Vec<Vec<&ParsedTextElement>> {
    elements.sort_by(|a, b| {
        native_baseline::line_y(b)
            .total_cmp(&native_baseline::line_y(a))
            .then(a.bounds.x0.total_cmp(&b.bounds.x0))
    });
    let mut rows: Vec<Vec<&ParsedTextElement>> = Vec::new();
    for element in elements {
        if let Some(row) = rows.last_mut().filter(|row| {
            (native_baseline::line_y(row[0]) - native_baseline::line_y(element)).abs()
                <= TEXT_ROW_Y_TOLERANCE_PT
        }) {
            row.push(element);
        } else {
            rows.push(vec![element]);
        }
    }
    if scripts {
        inline_script_rows::recover(&mut rows, None);
    }
    rows.retain(|row| !row.is_empty());
    for row in &mut rows {
        row.sort_by(|a, b| a.bounds.x0.total_cmp(&b.bounds.x0));
    }
    rows
}

/// 既存のセル文字列・断片間空白と完全一致する場合だけ、原本runのstyleを対応させる。
pub(super) fn legacy_runs<'a>(
    parsed: &ParsedDocument,
    selected: impl IntoIterator<Item = &'a ParsedTextElement>,
    expected: &str,
    graphics: &[ParsedGraphicElement],
    offsets: &BTreeMap<*const ParsedTextElement, i32>,
) -> Vec<StructuredTextRun> {
    let mut text = String::new();
    let mut runs = Vec::new();
    for element in selected {
        let start = text.len();
        append_cell_fragment(&mut text, element.text.trim());
        if text.len() == start {
            continue;
        }
        let Some(mut run) = structured_text_runs_from_row(&[element], graphics).pop() else {
            return Vec::new();
        };
        run.text = text[start..].to_string();
        run.style.baseline_offset = offsets.get(&std::ptr::from_ref(element)).copied();
        run.style.font_embedded = run.style.font_name.as_deref().is_some_and(|name| {
            parsed
                .embedded_fonts
                .iter()
                .any(|font| font.font_name == name)
        });
        // 旧本文へ補った区切り空白は、原本の字面にだけある下線を延長しない。
        let gap = run.text.len() - element.text.trim().len();
        if run.style.underlined && gap > 0 {
            let mut separator = run.clone();
            separator.text = run.text[..gap].to_string();
            separator.style.underlined = false;
            separator.style.baseline_offset = None;
            runs.push(separator);
            run.text = element.text.trim().to_string();
        }
        runs.push(run);
    }
    if text == expected { runs } else { Vec::new() }
}

/// 単一行の文字列と配置を保ち、各runのstyleと原本emの行boxを使う。
pub(super) fn single_line_paragraph(cell: &Cell) -> Option<Paragraph> {
    if cell.single_line_runs.is_empty() {
        return None;
    }
    let height = structured_line_max_font_size(cell.lines.first()?);
    if pdf_points_to_twips(height)? > pdf_points_to_twips(cell.top - cell.bottom)? {
        return None;
    }
    let spacing = native_line_pitch::spacing(height, 0, 0)?;
    Some(
        cell.single_line_runs
            .iter()
            .fold(Paragraph::new(), |p, run| p.add_run(cell_run(run)))
            .line_spacing(spacing),
    )
}

/// 複数行boxの配置が失敗しても、照合済みの旧本文を原本runと最大em高の一段落へ保つ。
pub(super) fn fallback_paragraph(cell: &Cell) -> Option<Paragraph> {
    if cell.fallback_runs.is_empty() {
        return None;
    }
    let height = cell.fallback_runs.iter().try_fold(0.0f32, |height, run| {
        pdf_font_size_to_half_points(run.style.font_size)?;
        Some(height.max(run.style.font_size))
    })?;
    if pdf_points_to_twips(height)? > pdf_points_to_twips(cell.top - cell.bottom)? {
        return None;
    }
    let spacing = native_line_pitch::spacing(height, 0, 0)?;
    Some(
        cell.fallback_runs
            .iter()
            .fold(Paragraph::new(), |p, run| p.add_run(cell_run(run)))
            .line_spacing(spacing),
    )
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
    if cell.lines.is_empty()
        || (cell.lines.len() < 2
            && cell.logical_row_right_anchor.is_none()
            && cell.source_tabs.is_none())
    {
        return None;
    }
    let spacings = line_spacings(cell)?;
    let mut out = TableCell::new().vertical_align(VAlignType::Top);
    out.property = out
        .property
        .margin_top(0, WidthType::Dxa)
        .margin_bottom(0, WidthType::Dxa)
        .margin_left(0, WidthType::Dxa)
        .margin_right(0, WidthType::Dxa);
    let right_anchors = cell
        .logical_row_right_anchor
        .map(|right| vec![right])
        .unwrap_or_else(|| {
            native_numeric_alignment::right_anchors(&cell.lines, cell.left, cell.right)
        });
    for (index, (line, spacing)) in cell.lines.iter().zip(spacings).enumerate() {
        let spacing = spacing?;
        let paragraph =
            if let Some(tabs) = cell.source_tabs.as_ref().filter(|_| cell.lines.len() == 1) {
                source_cell_tab_stops::paragraph(&line.runs, tabs)
            } else {
                line.runs
                    .iter()
                    .fold(Paragraph::new(), |p, run| p.add_run(cell_run(run)))
            };
        let mut paragraph = position_structured_paragraph(paragraph, line, cell.left, None, None)
            .line_spacing(spacing);
        if right_anchors[index] && cell.source_tabs.is_none() {
            let right = pdf_points_to_indent_twips(cell.right - line.right_x)?;
            paragraph =
                paragraph
                    .align(AlignmentType::Right)
                    .indent(Some(0), None, Some(right), None);
        }
        out = out.add_paragraph(paragraph);
    }
    Some(out)
}

/// 原本セルでも既存の代替字幅を使い、埋め込みfontと確定した字形配置は上書きしない。
pub(super) fn cell_run(source: &StructuredTextRun) -> Run {
    let mut run = docx_run_from_structured_run(source);
    if !source.style.font_embedded
        && source.style.placement.is_none()
        && let Some(stretch) = source
            .style
            .font_name
            .as_deref()
            .and_then(docx_font_stretch_from_pdf_font_name)
    {
        run = run.stretch(stretch);
    }
    run
}

#[cfg(test)]
mod fallback_width_tests {
    use super::*;
    use std::io::{Cursor, Read};

    /// 共通run builderを実際のWord XMLへ通し、幅の重複・上書きを検査する。
    fn output(embedded: bool, placement: Option<positioned_type1::Placement>) -> String {
        let source = StructuredTextRun {
            text: "Owned".into(),
            style: StructuredTextStyle {
                font_name: Some("MS-PGothic".into()),
                font_embedded: embedded,
                placement,
                ..StructuredTextStyle::default()
            },
        };
        let bytes = pack_docx_to_bytes(
            Docx::new().add_paragraph(Paragraph::new().add_run(cell_run(&source))),
            &[],
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut text = String::new();
        zip.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    }

    /// 確認済み埋め込みfontには、代替family用の補正を適用しない。
    #[test]
    fn embedded_source_font_does_not_gain_fallback_stretch() {
        assert!(!output(true, None).contains("<w:w "));
    }

    /// 明示のglyph幅・字間は代替字幅の92%で上書きしない。
    #[test]
    fn verified_glyph_placement_is_not_overwritten() {
        let xml = output(
            false,
            Some(positioned_type1::Placement {
                stretch: 83,
                spacing: 7,
                logical_space: false,
            }),
        );
        assert!(xml.contains("<w:w w:val=\"83\""));
        assert!(!xml.contains("<w:w w:val=\"92\""));
        assert!(xml.contains("<w:spacing w:val=\"7\""));
    }
}

/// 原本と全文照合したセルの背景色を、Wordセルの最終構築へ渡す。
pub(super) fn background(cell: &Cell) -> Option<[u8; 3]> {
    cell.background
}

/// 容量検査と累積丸めを、段落IDを生成しない遅延iteratorとして実際のbuilderと共有する。
fn line_spacings(cell: &Cell) -> Option<impl Iterator<Item = Option<LineSpacing>> + '_> {
    let capacity = u64::try_from(pdf_points_to_twips(cell.top - cell.bottom)?).ok()?;
    Some(line_spacings_with_capacity(cell, capacity))
}

/// 罫線中心の補正後も原本段落が収まるか、実際の出力と同じ累積丸めで確認する。
pub(super) fn fits_height_twips(cell: &Cell, height: f32) -> bool {
    height.is_finite()
        && height > 0.
        && height <= i32::MAX as f32
        && line_spacings_with_capacity(cell, height as u64).all(|spacing| spacing.is_some())
}

/// セルの原点を保ち、呼出元が確認した容量で行間iteratorを作る。
fn line_spacings_with_capacity(
    cell: &Cell,
    capacity: u64,
) -> impl Iterator<Item = Option<LineSpacing>> + '_ {
    let mut rounding = cell_line_rounding::Accumulator::default();
    let mut previous_bottom = cell.top;
    cell.lines.iter().map(move |line| {
        let before = (previous_bottom - line.top_y).max(0.0);
        let height = structured_line_max_font_size(line);
        let (before, line_twips, used) = rounding.line(before, height)?;
        if used > capacity {
            return None;
        }
        previous_bottom = line.top_y - height;
        native_line_pitch::spacing_twips(line_twips, before, 0)
    })
}

/// style対応を確認済みの一行だけ、既存段落を作り直さず水平原点の相対量を返す。
pub(super) fn single_line_horizontal_indent(cell: &Cell) -> Option<i32> {
    if cell.lines.len() != 1 || cell.single_line_runs.is_empty() {
        return None;
    }
    let line = &cell.lines[0];
    if !line.right_x.is_finite() || line.right_x > cell.right || line.right_x < line.left_x {
        return None;
    }
    pdf_points_to_indent_twips(line.left_x - cell.left)
}

#[cfg(test)]
#[path = "native_table_merge_tests.rs"]
mod merged_tests;

#[path = "native_table_logical_rows.rs"]
mod logical_rows;

/// 横結合した見出しを維持し、確認できた本文の反復行だけを編集可能なWord行へ分ける。
pub(super) fn refine_logical_rows(table: PositionedTable) -> PositionedTable {
    logical_rows::refine(table)
}

/// 別々に量子化する横辺・縦辺の端点差は1ptまで、高さは従来の半point以内でgridと照合する。
fn is_grid_border(graphic: &ParsedGraphicElement, xs: &[i32], ys: &[i32]) -> bool {
    let b = normalized_bounds(graphic.bounds);
    [b.x0, b.x1, b.y0, b.y1].iter().all(|v| v.is_finite())
        && is_horizontal_line(&b)
        && xs.iter().any(|x| (b.x0 - *x as f32).abs() <= 1.0)
        && xs.iter().any(|x| (b.x1 - *x as f32).abs() <= 1.0)
        && ys.iter().any(|y| (b.y0 - *y as f32).abs() <= 0.5)
}
