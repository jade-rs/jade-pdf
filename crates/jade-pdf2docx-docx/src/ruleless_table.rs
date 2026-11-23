//! 罫線のない表（または外枠だけの表）を、文字の並びから見つける。
//!
//! 連続する 3 行以上で、全行に共通する空き（列の切れ目）があり、多くの行が 2 列以上に
//! 文字を持つ範囲を表とみなす。本文の段組などを表にしないよう、セルの文字数が短いこと、
//! どの列も複数の行で使われていることも条件にする。

use super::*;

/// 同じ行の語の間で、列の切れ目とみなす空きの最小幅（em）。語間（約 0.25 em）より広い。
const COLUMN_GAP_EM: f32 = 0.6;
/// 表の行どうしの最大の間隔（行の高さに対する倍率）。
const ROW_GAP_RATIO: f32 = 2.5;
/// 表とみなす最小の行数。
const MIN_ROWS: usize = 3;
/// セルの平均文字数の上限（長い文を並べた本文の段組を除く）。
const MAX_AVERAGE_CELL_CHARS: f32 = 24.0;

/// 1 行分の文字片。
struct TextLine<'a> {
    /// x 順の文字片。
    texts: Vec<&'a ParsedTextElement>,
    /// 行の下端。
    bottom: f32,
    /// 行の上端。
    top: f32,
    /// 行の文字の高さ（中央値）。
    size: f32,
    /// 行が属する区画（段の番号と、囲む枠の番号）。
    band: (usize, Option<usize>),
}

impl TextLine<'_> {
    /// 字間調整で語の間を空けた文字片も語ごとに分けた、行の語の範囲。
    fn words(&self) -> Vec<table_cell_text::CellFragment> {
        self.texts
            .iter()
            .flat_map(|text| {
                table_cell_text::split_at_wide_spaces(&table_cell_text::CellFragment {
                    bounds: normalized_bounds(text.bounds),
                    text: text.text.clone(),
                    font_size: text.font_size,
                })
            })
            .collect()
    }

    /// 列の切れ目とみなす空きで分けた語の塊の範囲。
    fn segments(&self) -> Vec<(f32, f32)> {
        let mut segments: Vec<(f32, f32)> = Vec::new();
        for word in self.words() {
            let bounds = word.bounds;
            match segments.last_mut() {
                Some(last) if bounds.x0 - last.1 < COLUMN_GAP_EM * self.size => {
                    last.1 = last.1.max(bounds.x1);
                }
                _ => segments.push((bounds.x0, bounds.x1)),
            }
        }
        segments
    }
}

/// ページの罫線のない表を探す。`covered` は既に表になった範囲で、そこにかかる行は使わない。
pub(super) fn infer_ruleless_tables(
    parsed: &ParsedDocument,
    covered: &[PositionedTable],
) -> Vec<PositionedTable> {
    let pages = parsed
        .text_elements
        .iter()
        .map(|text| text.page)
        .collect::<BTreeSet<_>>();
    let mut tables = Vec::new();
    for page in pages {
        let lines = page_lines(parsed, page, covered);
        let mut start = 0usize;
        while start < lines.len() {
            // 2 列以上の行で始まり、表の途中では 1 列だけの行（小見出し）を 2 行まで含める。
            if lines[start].segments().len() < 2 {
                start += 1;
                continue;
            }
            let mut end = start;
            let mut last_multi = start;
            while end < lines.len()
                && (end == start
                    || lines[end - 1].band == lines[end].band
                        && lines[end - 1].bottom - lines[end].top
                            <= ROW_GAP_RATIO * lines[end].size.max(lines[end - 1].size))
                && (lines[end].segments().len() >= 2 || end - last_multi <= 2)
            {
                if lines[end].segments().len() >= 2 {
                    last_multi = end;
                }
                end += 1;
            }
            let end = last_multi + 1;
            if end - start >= MIN_ROWS
                && let Some(table) = table_from_lines(parsed, page, &lines[start..end])
            {
                tables.push(table);
            }
            // 条件を満たさない連続も、途中から切り出して表にはしない。
            start = end.max(start + 1);
        }
    }
    tables
}

/// 表にかからない文字片を、上から下の行にまとめる。
fn page_lines<'a>(
    parsed: &'a ParsedDocument,
    page: usize,
    covered: &[PositionedTable],
) -> Vec<TextLine<'a>> {
    let mut texts = parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page && !text.text.trim().is_empty())
        .filter(|text| {
            !covered
                .iter()
                .any(|table| text_element_is_in_table(parsed, text, table))
        })
        // 縦書きの文字片（ページの見出しなど）は縦に長く、行の範囲を広げてしまうので除く。
        .filter(|text| {
            let bounds = normalized_bounds(text.bounds);
            let (width, height) = (bounds.x1 - bounds.x0, bounds.y1 - bounds.y0);
            !(height > 3.0 * width && height > 2.0 * text.font_size.max(1.0))
        })
        .collect::<Vec<_>>();
    texts.sort_by(|a, b| {
        normalized_bounds(b.bounds)
            .y1
            .total_cmp(&normalized_bounds(a.bounds).y1)
    });
    // 段の区切り線をまたいで行をまとめない。
    // 罫線で囲んだ枠の内と外も別の区画にする（枠外の見出しなどを表に混ぜない）。
    let edges = column_bands::column_band_edges(parsed, page, covered);
    let partial = column_bands::partial_band_rules(parsed, page, covered);
    let frames = column_bands::ruled_frames(parsed, page);
    let band = |text: &ParsedTextElement| {
        let bounds = normalized_bounds(text.bounds);
        let (x, y) = ((bounds.x0 + bounds.x1) / 2.0, (bounds.y0 + bounds.y1) / 2.0);
        (
            column_bands::band_of(x, &edges) + column_bands::partial_band_of(x, y, &partial),
            column_bands::frame_of(x, y, &frames),
        )
    };
    let mut lines: Vec<TextLine<'a>> = Vec::new();
    for text in texts {
        let bounds = normalized_bounds(text.bounds);
        let center = (bounds.y0 + bounds.y1) / 2.0;
        let slack = 0.3 * (bounds.y1 - bounds.y0);
        match lines.iter_mut().find(|line| {
            center > line.bottom - slack && center < line.top + slack && line.band == band(text)
        }) {
            Some(line) => {
                line.texts.push(text);
                line.bottom = line.bottom.min(bounds.y0);
                line.top = line.top.max(bounds.y1);
            }
            None => lines.push(TextLine {
                texts: vec![text],
                bottom: bounds.y0,
                top: bounds.y1,
                size: 0.0,
                band: band(text),
            }),
        }
    }
    for line in &mut lines {
        line.texts.sort_by(|a, b| {
            normalized_bounds(a.bounds)
                .x0
                .total_cmp(&normalized_bounds(b.bounds).x0)
        });
        let mut sizes = line
            .texts
            .iter()
            .map(|text| {
                text.font_size
                    .max(normalized_bounds(text.bounds).y1 - normalized_bounds(text.bounds).y0)
            })
            .collect::<Vec<_>>();
        sizes.sort_by(f32::total_cmp);
        line.size = sizes[sizes.len() / 2].max(1.0);
    }
    lines.sort_by(|a, b| a.band.cmp(&b.band).then(b.top.total_cmp(&a.top)));
    lines
}

/// 行の範囲から、全行に共通する列の切れ目を求めて表を作る。条件を満たさなければ `None`。
fn table_from_lines(
    parsed: &ParsedDocument,
    page: usize,
    lines: &[TextLine<'_>],
) -> Option<PositionedTable> {
    let size = lines.iter().map(|line| line.size).fold(0.0, f32::max);
    let mut intervals = lines
        .iter()
        .flat_map(TextLine::segments)
        .collect::<Vec<_>>();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let left = intervals.first()?.0;
    let mut boundaries = Vec::new();
    let mut reach = intervals[0].1;
    for &(start, end) in &intervals[1..] {
        if start - reach >= COLUMN_GAP_EM * size {
            boundaries.push((start + reach) / 2.0);
        }
        reach = reach.max(end);
    }
    let right = reach;
    if boundaries.is_empty() {
        return None;
    }
    let pad = 0.25 * size;
    let mut xs = vec![left - pad];
    xs.extend(boundaries.iter().copied());
    xs.push(right + pad);
    let rows = lines.len();
    // 各列がほとんどの行で使われ、各行が 2 列以上を使い、セルが短いこと。
    let mut fragments;
    loop {
        let cols = xs.len() - 1;
        fragments = vec![vec![Vec::<table_cell_text::CellFragment>::new(); cols]; rows];
        for (row, line) in lines.iter().enumerate() {
            for word in line.words() {
                let center = (word.bounds.x0 + word.bounds.x1) / 2.0;
                let col = xs
                    .windows(2)
                    .position(|pair| center >= pair[0] && center < pair[1])?;
                fragments[row][col].push(word);
            }
        }
        // 金額の左に置いた負の数の印（△）だけの列は、右の金額の列の一部とする。
        match sign_column(&fragments) {
            Some(col) => {
                xs.remove(col + 1);
            }
            None => break,
        }
    }
    let cols = xs.len() - 1;
    if cols < 2 {
        return None;
    }
    let filled = |row: usize, col: usize| !fragments[row][col].is_empty();
    // 6 割以上の行が 2 列以上を使い、どの列も 2 行以上かつ 2 割以上の行で使われること。
    let multi_column_rows = (0..rows)
        .filter(|row| (0..cols).filter(|col| filled(*row, *col)).count() >= 2)
        .count();
    if multi_column_rows * 5 < rows * 3 {
        return None;
    }
    if (0..cols).any(|col| {
        let used = (0..rows).filter(|row| filled(*row, col)).count();
        used < 2 || used * 5 < rows
    }) {
        return None;
    }
    // 行の境界は行間の中央、外側は半行分あける。
    let mut ys = vec![lines[0].top + pad];
    for pair in lines.windows(2) {
        ys.push((pair[0].bottom + pair[1].top) / 2.0);
    }
    ys.push(lines[rows - 1].bottom - pad);
    let mut cells = vec![vec![String::new(); cols]; rows];
    let mut characters = 0usize;
    for row in 0..rows {
        for col in 0..cols {
            cells[row][col] =
                table_cell_text::compose_cell(&fragments[row][col], xs[col], xs[col + 1]).text;
            characters += cells[row][col].chars().count();
        }
    }
    let filled_cells = (0..rows)
        .flat_map(|row| (0..cols).map(move |col| (row, col)))
        .filter(|(row, col)| filled(*row, *col))
        .count();
    if characters as f32 / filled_cells.max(1) as f32 > MAX_AVERAGE_CELL_CHARS {
        return None;
    }
    // 行の多い範囲は、数値の多い表（財務表など）だけを表にし、長文の段組は表にしない。
    let numeric_cells = cells
        .iter()
        .flatten()
        .filter(|cell| is_numeric_cell(cell))
        .count();
    if text_grid_page_is_dense_article_like(0, rows) && numeric_cells * 10 < filled_cells * 3 {
        return None;
    }
    let bounds = ParsedBounds {
        x0: xs[0],
        y0: ys[rows],
        x1: xs[cols],
        y1: ys[0],
    };
    let frame = enclosing_frame(parsed, page, bounds);
    let model_rows = (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| DocxTableCellModel {
                    text: cells[row][col].clone(),
                    underlined: false,
                    // 数値のセルは右揃え（財務表などの桁を揃える）。
                    alignment: if is_numeric_cell(&cells[row][col]) {
                        ParagraphAlignment::Right
                    } else {
                        ParagraphAlignment::Left
                    },
                    vertical_alignment: Some(CellVerticalAlignment::Center),
                    grid_span: 1,
                    vertical_merge: None,
                    width_twips: None,
                    borders: Some(DocxCellBorders {
                        top: frame && row == 0,
                        left: frame && col == 0,
                        bottom: frame && row + 1 == rows,
                        right: frame && col + 1 == cols,
                        size_eighths_pt: 4,
                    }),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let font_size = {
        let mut sizes = lines.iter().map(|line| line.size).collect::<Vec<_>>();
        sizes.sort_by(f32::total_cmp);
        sizes.get(sizes.len() / 2).copied()
    };
    Some(PositionedTable {
        page,
        bounds,
        stroke_width: 0.5,
        font_name: common_usable_pdf_font_name(
            lines
                .iter()
                .flat_map(|line| line.texts.iter())
                .filter_map(|text| text.font_name.as_deref()),
        ),
        font_embedded: false,
        font_size: font_size.filter(|size| (4.0..=96.0).contains(size)),
        column_widths_twips: xs
            .windows(2)
            .filter_map(|pair| pdf_points_to_twips(pair[1] - pair[0]))
            .collect(),
        table: OcrTable::new(cells),
        cell_underlines: vec![vec![false; cols]; rows],
        cell_alignments: vec![vec![ParagraphAlignment::Left; cols]; rows],
        cell_vertical_alignments: Vec::new(),
        docx_table: Some(DocxTableModel { rows: model_rows }),
        nested_tables: Vec::new(),
        // 行の高さは PDF の行の間隔を下限にし、代替フォントの行の高さに合わせて伸ばせるようにする。
        row_heights_pt: ys.windows(2).map(|pair| pair[0] - pair[1]).collect(),
        row_line_counts: vec![2; rows],
        cell_padding_pt: Some(pad),
        cell_font_sizes: Vec::new(),
        cell_tab_stops: Vec::new(),
        cell_fragments: Vec::new(),
        cell_shading: Vec::new(),
    })
}

/// 表の範囲をすぐ外側で囲む矩形（4 辺の罫線）があるか判定する。
fn enclosing_frame(parsed: &ParsedDocument, page: usize, bounds: ParsedBounds) -> bool {
    let (horizontal, vertical) = grid_rules_from_graphics(
        &parsed
            .graphics
            .iter()
            .filter(|graphic| graphic.page == page)
            .cloned()
            .collect::<Vec<_>>(),
    );
    let near = |rules: &[table_grid::Rule], low: f32, high: f32, from: f32, to: f32| {
        rules.iter().any(|rule| {
            rule.at >= low
                && rule.at <= high
                && rule.start.min(rule.end) <= from + 2.0
                && rule.start.max(rule.end) >= to - 2.0
        })
    };
    let margin = 12.0;
    near(
        &horizontal,
        bounds.y1 - 1.0,
        bounds.y1 + margin,
        bounds.x0,
        bounds.x1,
    ) && near(
        &horizontal,
        bounds.y0 - margin,
        bounds.y0 + 1.0,
        bounds.x0,
        bounds.x1,
    ) && near(
        &vertical,
        bounds.x0 - margin,
        bounds.x0 + 1.0,
        bounds.y0,
        bounds.y1,
    ) && near(
        &vertical,
        bounds.x1 - 1.0,
        bounds.x1 + margin,
        bounds.y0,
        bounds.y1,
    )
}

/// 金額の前に付ける符号だけの字か（負の数の印 △・▲、正負の符号）。
fn is_sign(text: &str) -> bool {
    matches!(text, "△" | "▲" | "－" | "-" | "−" | "＋" | "+")
}

/// 符号だけが入り、その行の右の列が数値の列を探す（右の列と合わせて 1 つの金額の列にする）。
///
/// 財務表では負の金額の印「△」を金額の列の左端にそろえて組むので、字間から列を分けると
/// 印だけの列ができる（lawrenceanum の kanpo-pdf が △ を右の数に付けるのと同じ考え）。
fn sign_column(fragments: &[Vec<Vec<table_cell_text::CellFragment>>]) -> Option<usize> {
    let cols = fragments.first()?.len();
    let text = |row: usize, col: usize| {
        fragments[row][col]
            .iter()
            .map(|fragment| fragment.text.trim())
            .collect::<String>()
    };
    (0..cols.saturating_sub(1)).find(|&col| {
        let mut used = 0;
        let all_signs = (0..fragments.len()).all(|row| {
            let sign = text(row, col);
            if sign.is_empty() {
                return true;
            }
            used += 1;
            is_sign(&sign) && is_numeric_cell(&text(row, col + 1))
        });
        all_signs && used > 0
    })
}

/// 文字の半分以上が数字のセルか判定する。
fn is_numeric_cell(cell: &str) -> bool {
    let body = cell.trim();
    let digits = body
        .chars()
        .filter(|ch| ch.is_ascii_digit() || ('０'..='９').contains(ch))
        .count();
    !body.is_empty() && digits * 2 >= body.chars().filter(|ch| !ch.is_whitespace()).count()
}
