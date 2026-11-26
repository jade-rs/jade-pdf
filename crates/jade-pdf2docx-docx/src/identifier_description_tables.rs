//! 多数の機械的な識別子と説明が対応する反復列だけを、通常の二段組から区別する。
use super::*;

/// 同じ二列の原点から組み直し、原本の文字を一対ずつ引き取った行。
struct Row {
    /// 識別子と説明の原本文字列。
    text: Vec<String>,
    /// 同じ文字列へ対応する元のrunと位置。
    cells: Vec<text_grid_cell_styles::Cell>,
    /// 元の字面を包含する矩形。
    bounds: ParsedBounds,
    /// 原本で確認した最大em高。
    height: f32,
}

/// 従来の表候補がないページで、連続するコード説明行を局所的に採用する。
pub(super) fn infer(parsed: &ParsedDocument, page: usize) -> Option<PositionedTable> {
    if parsed.image_elements.iter().any(|image| image.page == page) {
        return None;
    }
    let rows = native_table_text::source_rows(
        parsed
            .text_elements
            .iter()
            .filter(|text| text.page == page && !text.text.is_empty())
            .collect(),
        false,
    );
    let mut anchors = Vec::<(f32, f32, usize)>::new();
    for row in &rows {
        let visible = row
            .iter()
            .copied()
            .filter(|text| !text.text.trim().is_empty())
            .collect::<Vec<_>>();
        let Some(cells) = merge_text_row_into_cells(&visible).filter(|cells| cells.len() == 2)
        else {
            continue;
        };
        if self::row(parsed, row, cells[0].x0, cells[1].x0).is_none() {
            continue;
        }
        if let Some(anchor) = anchors.iter_mut().find(|(left, right, _)| {
            (cells[0].x0 - *left).abs() <= 1.0 && (cells[1].x0 - *right).abs() <= 1.0
        }) {
            anchor.2 += 1;
        } else {
            anchors.push((cells[0].x0, cells[1].x0, 1));
        }
    }
    let (left, right, support) = anchors.into_iter().max_by_key(|a| a.2)?;
    if support < 6 || right - left < 48.0 {
        return None;
    }
    let mut groups = Vec::<Vec<Row>>::new();
    let mut current = Vec::<Row>::new();
    for source in &rows {
        let candidate = row(parsed, source, left, right);
        let continues = candidate.as_ref().is_some_and(|next| {
            current.last().is_none_or(|previous| {
                let pitch = previous.bounds.y1 - next.bounds.y1;
                pitch >= previous.height * 0.8 && pitch <= previous.height.max(next.height) * 2.0
            })
        });
        if !continues && !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
        if let Some(candidate) = candidate {
            current.push(candidate);
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    let rows = groups
        .into_iter()
        .filter(|rows| {
            rows.len() >= 6
                && rows
                    .iter()
                    .filter(|row| row.text[0].chars().any(|c| c.is_ascii_digit() || c == '_'))
                    .count()
                    * 2
                    >= rows.len()
        })
        .max_by_key(Vec::len)?;
    let bounds = rows.iter().fold(rows[0].bounds, |b, row| ParsedBounds {
        x0: b.x0.min(row.bounds.x0),
        y0: b.y0.min(row.bounds.y0),
        x1: b.x1.max(row.bounds.x1),
        y1: b.y1.max(row.bounds.y1),
    });
    let widths = [right - left, bounds.x1 - right]
        .into_iter()
        .map(|v| pdf_points_to_twips(v).filter(|v| *v > 0))
        .collect::<Option<Vec<_>>>()?;
    let heights = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let pitch = rows
                .get(i + 1)
                .map_or(row.height, |next| row.bounds.y1 - next.bounds.y1);
            pdf_points_to_twips_f32(pitch).filter(|v| *v > 0.0)
        })
        .collect::<Option<Vec<_>>>()?;
    let table = PositionedTable {
        page,
        bounds,
        allow_top_origin_text_y: false,
        stroke_width: 0.0,
        font_name: None,
        font_embedded: false,
        embedded_face: None,
        font_size: None,
        column_widths_twips: widths,
        row_heights_twips: heights,
        table: OcrTable::new(rows.iter().map(|row| row.text.clone()).collect()),
        cell_underlines: Vec::new(),
        cell_alignments: Vec::new(),
        docx_table: None,
        native_cells: Vec::new(),
        source_cell_runs: rows.into_iter().map(|row| row.cells).collect(),
        nested_tables: Vec::new(),
    };
    text_grid_ownership::preserves(parsed, &table).then_some(table)
}

/// 二つの列原点と非交差の字面を確認し、各セルの文字を原本の字間で復元する。
fn row(
    parsed: &ParsedDocument,
    source: &[&ParsedTextElement],
    left: f32,
    right: f32,
) -> Option<Row> {
    if (source.first()?.bounds.x0 - left).abs() > 1.0 {
        return None;
    }
    let split = source
        .iter()
        .position(|text| text.bounds.x0 >= right - 1.0)?;
    if split == 0
        || (source.get(split)?.bounds.x0 - right).abs() > 1.0
        || source[..split].iter().any(|text| text.bounds.x1 > right)
    {
        return None;
    }
    let top = source
        .iter()
        .map(|text| text.bounds.y1)
        .fold(f32::NEG_INFINITY, f32::max);
    let height = source
        .iter()
        .map(|text| text.font_size)
        .fold(0.0f32, f32::max);
    if !height.is_finite() || height <= 0.0 {
        return None;
    }
    let (key, key_cell) =
        text_grid_cell_styles::source_cell(parsed, source[..split].to_vec(), top)?;
    let (value, value_cell) =
        text_grid_cell_styles::source_cell(parsed, source[split..].to_vec(), top)?;
    if !identifier(&key) || !description(&value) {
        return None;
    }
    let bounds = ParsedBounds {
        x0: left,
        x1: source
            .iter()
            .map(|text| text.bounds.x1)
            .fold(right, f32::max),
        y0: top - height,
        y1: top,
    };
    Some(Row {
        text: vec![key, value],
        cells: vec![key_cell, value_cell],
        bounds,
        height,
    })
}

/// 空白のない短い大文字コードだけを識別子として扱い、通常の文章を除外する。
fn identifier(value: &str) -> bool {
    let value = value.trim();
    (2..=32).contains(&value.len())
        && value.starts_with(|c: char| c.is_ascii_uppercase())
        && value
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// 通常語を含む複数語の説明を要求し、左右に単なる数値やコードが並ぶ候補を除く。
fn description(value: &str) -> bool {
    value.split_whitespace().count() >= 2 && value.chars().any(|c| c.is_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SDKが数字の途中で分けた原本断片を、空白を含めず同じbaselineへ並べる。
    fn fragmented_codes() -> ParsedDocument {
        let mut parsed = ParsedDocument {
            quality: Default::default(),
            path: "owned-fragmented-identifiers.pdf".into(),
            byte_len: 1,
            header: "%PDF-1.7".into(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
            page_count: 1,
            pages: Vec::new(),
            graphics: Vec::new(),
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
            text_elements: Vec::new(),
        };
        for index in 0..6 {
            let y = 700.0 - index as f32 * 14.0;
            for (text, x0, x1) in [
                ("FIELD_202".to_string(), 54.0, 104.0),
                (index.to_string(), 104.1, 110.1),
                (format!("Original description {index}"), 234.0, 450.0),
            ] {
                parsed.text_elements.push(ParsedTextElement {
                    source_background: None,
                    fill_stroke_width: None,
                    page: 0,
                    text,
                    font_size: 10.0,
                    font_name: Some("Helvetica".into()),
                    font_weight: None,
                    fill_color: None,
                    baseline_y: Some(y),
                    bounds: ParsedBounds {
                        x0,
                        x1,
                        y0: y,
                        y1: y + 10.0,
                    },
                });
            }
        }
        parsed
    }

    /// 座標の接するコード断片を旧文字列の補間空白で拒否せず、同じ列の根拠にする。
    #[test]
    fn repeated_anchor_uses_geometric_identifier_text_not_legacy_fragment_spaces() {
        let table = infer(&fragmented_codes(), 0).expect("six geometrically complete code rows");
        let expected = (0..6)
            .map(|i| vec![format!("FIELD_202{i}"), format!("Original description {i}")])
            .collect::<Vec<_>>();
        assert_eq!(table.table.rows, expected);
    }

    /// 原本座標を照合した上半分の表へ、下の注記を上下反転して取り込まない。
    #[test]
    fn verified_bottom_origin_table_does_not_own_a_mirrored_lower_note() {
        let mut parsed = fragmented_codes();
        parsed.pages.push(ParsedPage {
            page_number: 0,
            width: 612.0,
            height: 792.0,
            rotation: 0,
        });
        let mut note = parsed.text_elements[0].clone();
        note.text = "Independent lower note.".into();
        note.baseline_y = Some(100.0);
        note.bounds = ParsedBounds {
            x0: 54.0,
            x1: 200.0,
            y0: 100.0,
            y1: 110.0,
        };
        parsed.text_elements.push(note.clone());
        let table = infer(&parsed, 0)
            .expect("the unrelated lower note must not reject the verified code table");
        assert_eq!(table.table.row_count(), 6);
        assert!(!text_element_is_in_table(&parsed, &note, &table));
    }

    /// 説明の語間が独立したSDK要素でも、変数の描画断片と区別して空白を保持する。
    #[test]
    fn explicit_space_elements_separate_description_words() {
        let mut parsed = fragmented_codes();
        let mut elements = Vec::new();
        for row in parsed.text_elements.chunks(3) {
            elements.extend_from_slice(&row[..2]);
            for (text, x0, x1) in [
                ("Original".to_string(), 234.0, 270.0),
                (" ".to_string(), 270.0, 273.0),
                (
                    row[2].text.strip_prefix("Original ").unwrap().to_string(),
                    273.0,
                    450.0,
                ),
            ] {
                let mut element = row[2].clone();
                element.text = text;
                element.bounds.x0 = x0;
                element.bounds.x1 = x1;
                elements.push(element);
            }
        }
        parsed.text_elements = elements;
        let table = infer(&parsed, 0).expect("explicit spaces are source text, not absent cells");
        assert_eq!(table.table.rows[0][1], "Original description 0");
    }

    /// コード候補の中の明示的な空白を取り去って、別の識別子へ正規化しない。
    #[test]
    fn explicit_space_inside_identifier_is_not_removed_to_accept_a_table() {
        let mut parsed = fragmented_codes();
        let mut elements = Vec::new();
        for row in parsed.text_elements.chunks(3) {
            elements.push(row[0].clone());
            let mut space = row[1].clone();
            space.text = " ".into();
            space.bounds.x0 = 104.0;
            space.bounds.x1 = 107.0;
            elements.push(space);
            let mut digit = row[1].clone();
            digit.bounds.x0 = 107.0;
            digit.bounds.x1 = 113.0;
            elements.push(digit);
            elements.push(row[2].clone());
        }
        parsed.text_elements = elements;
        assert!(infer(&parsed, 0).is_none());
    }
}
