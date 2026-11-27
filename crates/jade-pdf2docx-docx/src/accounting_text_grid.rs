//! 二列の会計表にある分離した符号と金額を、同じセル内の原本位置へ戻す。
use super::*;

/// 罫線でなく繰り返す原本座標で照合する、二つの列原点の許容差。
const COLUMN_TOLERANCE: f32 = 2.;

/// 摘要と金額を描いた原本行。独立した符号は金額に付く。
struct Row<'a, 'source> {
    /// 摘要のセル。
    label: &'a TextGridCell<'source>,
    /// 金額セルの最初に別々に置かれた符号。
    sign: Option<&'a TextGridCell<'source>>,
    /// 元の金額（または見出し）のセル。
    value: &'a TextGridCell<'source>,
}

/// 独立列の見出しがなく、左右の字面を再現できる会計表だけを採用する。
pub(super) fn infer(
    parsed: &ParsedDocument,
    page: usize,
    cells: &[Vec<TextGridCell<'_>>],
) -> Option<PositionedTable> {
    let signs = cells
        .iter()
        .filter(|cells| matches!(cells.as_slice(), [_, sign, _] if detached_sign(&sign.text)));
    if signs.take(2).count() < 2 {
        return None;
    }
    let mut rows = cells
        .iter()
        .map(|cells| Row::from_cells(cells))
        .collect::<Option<Vec<_>>>()?;
    rows.sort_by(|a, b| b.label.y1.total_cmp(&a.label.y1));
    let header = usize::from(!amount(&rows.first()?.value.text));
    let body = rows.get(header..)?;
    let left = rows.iter().map(|row| row.label.x0).min_by(f32::total_cmp)?;
    let sign_left = body
        .iter()
        .filter_map(|row| row.sign.map(|sign| sign.x0))
        .min_by(f32::total_cmp)?;
    if !verified_columns(&rows, body, left, sign_left) {
        return None;
    }
    let page_width = parsed
        .pages
        .iter()
        .find(|p| p.page_number == page)
        .map(|p| parsed_page_display_size_points(p).0)?;
    let row_bounds = rows.iter().map(Row::bounds).collect::<Vec<_>>();
    let right = rows.iter().try_fold(sign_left, |right, row| {
        let label_end = estimated_end(parsed, row.label)?;
        let value_end = estimated_end(parsed, row.value)?;
        if label_end >= sign_left
            || row.sign.is_some_and(|sign| {
                estimated_end(parsed, sign).is_none_or(|end| end + 0.05 >= row.value.x0)
            })
        {
            return None;
        }
        Some(right.max(value_end + 0.1))
    })?;
    let (top, bottom) = (row_bounds.first()?.y1, row_bounds.last()?.y0);
    if ![left, right, top, bottom, page_width]
        .iter()
        .all(|v| v.is_finite())
        || left < 0.
        || right > page_width
    {
        return None;
    }
    let widths = [sign_left - left, right - sign_left]
        .into_iter()
        .map(|width| pdf_points_to_twips(width).filter(|w| *w > 0))
        .collect::<Option<Vec<_>>>()?;
    let heights = row_bounds
        .iter()
        .enumerate()
        .map(|(i, bounds)| {
            let min_height = bounds.y1 - bounds.y0;
            let height = row_bounds
                .get(i + 1)
                .map_or(min_height, |next| bounds.y1 - next.y1);
            pdf_points_to_twips_f32(height).filter(|h| *h > 0. && height >= min_height)
        })
        .collect::<Option<Vec<_>>>()?;
    let mut logical_rows = Vec::with_capacity(rows.len());
    let mut styled_rows = Vec::with_capacity(rows.len());
    for (row, bounds) in rows.iter().zip(row_bounds) {
        let label = &row.label.text;
        let value = row.sign.map_or_else(
            || row.value.text.clone(),
            |sign| format!("{}{}", sign.text, row.value.text),
        );
        let mut fragments =
            Vec::with_capacity(row.value.texts.len() + usize::from(row.sign.is_some()));
        if let Some(sign) = row.sign {
            fragments.extend_from_slice(&sign.texts);
        }
        fragments.extend_from_slice(&row.value.texts);
        styled_rows.push(vec![
            text_grid_cell_styles::aligned_accounting_cell(
                parsed,
                &row.label.texts,
                label,
                bounds.y1,
                left,
                None,
            )?,
            text_grid_cell_styles::aligned_accounting_cell(
                parsed,
                &fragments,
                &value,
                bounds.y1,
                sign_left,
                row.sign.map(|_| row.value.x0),
            )?,
        ]);
        logical_rows.push(vec![label.clone(), value]);
    }
    let table = PositionedTable {
        page,
        bounds: ParsedBounds {
            x0: left,
            y0: bottom,
            x1: right,
            y1: top,
        },
        allow_top_origin_text_y: false,
        stroke_width: 0.,
        font_name: None,
        font_embedded: false,
        embedded_face: None,
        font_size: None,
        column_widths_twips: widths,
        source_rule_grid: None,
        row_heights_twips: heights,
        legacy_source_grid_row_heights: None,
        table: OcrTable::new(logical_rows),
        cell_underlines: Vec::new(),
        cell_alignments: Vec::new(),
        docx_table: None,
        native_cells: Vec::new(),
        source_cell_runs: styled_rows,
        nested_tables: Vec::new(),
    };
    text_grid_ownership::preserves(parsed, &table).then_some(table)
}

impl<'a, 'source> Row<'a, 'source> {
    /// 摘要、任意の単独符号、金額の順であることを確認する。
    fn from_cells(cells: &'a [TextGridCell<'source>]) -> Option<Self> {
        let (label, sign, value) = match cells {
            [label, value] => (label, None, value),
            [label, sign, value]
                if detached_sign(&sign.text)
                    && sign.texts.len() == 1
                    && value.texts.len() == 1
                    && amount(&value.text) =>
            {
                (label, Some(sign), value)
            }
            _ => return None,
        };
        if cells.iter().any(|cell| {
            ![cell.x0, cell.x1, cell.y0, cell.y1]
                .iter()
                .all(|v| v.is_finite())
                || cell.x0 >= cell.x1
                || cell.y0 >= cell.y1
        }) {
            return None;
        }
        label
            .text
            .chars()
            .any(char::is_alphabetic)
            .then_some(Self { label, sign, value })
    }

    /// 表の行に収める原本の字面の上下端。
    fn bounds(&self) -> ParsedBounds {
        ParsedBounds {
            x0: self.label.x0,
            x1: self.value.x1,
            y0: self.label.y0.min(self.value.y0),
            y1: self.label.y1.max(self.value.y1),
        }
    }
}

/// 左右いずれかの金額揃えと二行以上の符号反復を、全文字を所有できる範囲で確かめる。
fn verified_columns(rows: &[Row<'_, '_>], body: &[Row<'_, '_>], left: f32, sign_left: f32) -> bool {
    let Some(first) = body.first() else {
        return false;
    };
    body.len() >= 3
        && body.iter().all(|row| amount(&row.value.text))
        && (body
            .iter()
            .all(|row| (row.value.x0 - first.value.x0).abs() <= COLUMN_TOLERANCE)
            || body
                .iter()
                .all(|row| (row.value.x1 - first.value.x1).abs() <= COLUMN_TOLERANCE))
        && rows.iter().all(|row| {
            row.label.x0 - left <= COLUMN_TOLERANCE
                && row.value.x0 >= sign_left
                && row.sign.is_none_or(|sign| {
                    sign.x0 - sign_left <= COLUMN_TOLERANCE
                        && (native_baseline::line_y(sign.texts[0])
                            - native_baseline::line_y(row.value.texts[0]))
                        .abs()
                            <= 1.
                })
        })
}

/// 金額の符号として扱う文字だけを、その原本の字形のまま認識する。
fn detached_sign(value: &str) -> bool {
    matches!(value, "-" | "+" | "−")
}

/// 数字・桁区切り・小数点を含む金額を認識し、孤立した符号や説明は対象外にする。
fn amount(value: &str) -> bool {
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit());
    let whole = match value.split_once('.') {
        Some((whole, fraction)) if digits(fraction) => whole,
        Some(_) => return false,
        None => value,
    };
    let mut groups = whole.split(',');
    let first = groups.next().unwrap_or_default();
    digits(first)
        && (!whole.contains(',') || first.len() <= 3)
        && groups.all(|group| group.len() == 3 && digits(group))
}

/// Wordの標準代替fontで、セルやタブを越えない保守的な水平方向の到達位置を返す。
fn estimated_end(parsed: &ParsedDocument, cell: &TextGridCell<'_>) -> Option<f32> {
    let size = cell.texts.iter().try_fold(0_f32, |max_size, text| {
        let name = text.font_name.as_deref()?;
        if parsed
            .embedded_fonts
            .iter()
            .any(|font| font.font_name == name)
        {
            return None;
        }
        let em = match name {
            // Courier Newの等幅600/1000emにfont unitの丸め余地を持たせる。
            "Courier" | "Courier-Bold" | "Courier-Oblique" | "Courier-BoldOblique" => 0.61,
            "Helvetica"
            | "Helvetica-Bold"
            | "Helvetica-Oblique"
            | "Helvetica-BoldOblique"
            | "Times-Roman"
            | "Times-Bold"
            | "Times-Italic"
            | "Times-BoldItalic" => 1.,
            _ => return None,
        };
        (text.font_size.is_finite()
            && text.font_size > 0.
            && text
                .text
                .chars()
                .all(|c| c.is_ascii_graphic() || c == ' ' || c == '−'))
        .then_some(max_size.max(text.font_size * em))
    })?;
    Some(
        cell.x1
            .max(cell.x0 + size * cell.text.chars().count() as f32),
    )
}
