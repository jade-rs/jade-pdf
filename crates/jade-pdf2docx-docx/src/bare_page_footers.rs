//! 全ページの原本位置・書式・連番が一致した裸の数字だけを、失われていたfooterへ戻す。
use super::*;
use docx_rs::BuildXML;

/// 最初の原本ページで確認した番号・Word書式・寸法とfooter原点。
struct Reference {
    /// 最初の番号。後続は元ページ数だけ増える場合に限る。
    number: u32,
    /// 数字の役として正規化した実際のWord書式。
    style: Vec<Vec<u8>>,
    /// 同じ表示幅のページだけを組として扱う。
    width: f32,
    /// 同じ表示高のページだけを組として扱う。
    height: f32,
    /// 原本の下端からのbaseline距離。
    edge: f32,
    /// 原本のページ中央からの水平差。
    offset: f32,
}

/// 三ページ以上の全出現を照合し、旧本文で除外される数字を原本のfooterへ一度ずつ戻す。
pub(super) fn select(
    parsed: &ParsedDocument,
    pages: &[StructuredTextPage],
) -> Vec<(usize, StructuredTextLine)> {
    recover(parsed, pages).unwrap_or_default()
}

/// 寸法・原点・fontが一つでも不明なページを推測せず、全出現が確認できた場合だけ返す。
fn recover(
    parsed: &ParsedDocument,
    pages: &[StructuredTextPage],
) -> Option<Vec<(usize, StructuredTextLine)>> {
    // 密な実段組ではfooterだけでも既存の改ページが変わるため、全体の組を昇格しない。
    if parsed.page_count < 3
        || pages.len() != parsed.page_count
        || text_lines_by_page_from_pdf_text_elements(parsed, &[])
            .iter()
            .any(|page| source_prose_columns::has_dense_rows(parsed, page))
    {
        return None;
    }
    let space_family = layout_space::family(&parsed.embedded_fonts, &parsed.text_elements);
    let mut result = Vec::new();
    let mut first: Option<Reference> = None;
    for (index, page) in pages.iter().enumerate() {
        if page.page != index {
            return None;
        }
        let geometry = parsed.pages.iter().find(|p| p.page_number == page.page)?;
        if geometry.rotation.rem_euclid(360) != 0 || parsed_page_size_twips(geometry).is_none() {
            return None;
        }
        let (width, height) = parsed_page_display_size_points(geometry);
        let rows = native_table_text::source_rows(
            parsed
                .text_elements
                .iter()
                .filter(|text| {
                    text.page == page.page
                        && !text.text.is_empty()
                        && text.bounds.y0 >= 0.0
                        && text.bounds.y1 <= height * 0.12
                })
                .collect(),
            false,
        );
        let mut candidates = Vec::new();
        for row in rows {
            let Some(start) = row.iter().position(|text| !text.text.trim().is_empty()) else {
                continue;
            };
            let end = row.iter().rposition(|text| !text.text.trim().is_empty())? + 1;
            let mut line = structured_text_line_from_segment(
                &TextRowSegment {
                    texts: row[start..end].to_vec(),
                    split_from_row: false,
                },
                &parsed.graphics,
                width,
            )?;
            let Some(number) = number(&line, width, height) else {
                continue;
            };
            if !confirmed_upright(&row[start..end]) || overlaps_decoration(parsed, page.page, &line)
            {
                return None;
            }
            mark_embedded_fonts_in_line(&mut line, &parsed.embedded_fonts, &space_family);
            candidates.push((number, line));
        }
        if candidates.len() != 1 {
            return None;
        }
        let (number, line) = candidates.pop()?;
        let style = style(&line);
        let edge = line.top_y - structured_line_max_font_size(&line);
        let offset = (line.left_x + line.right_x) / 2.0 - width / 2.0;
        if let Some(reference) = &first {
            if reference.number.checked_add(u32::try_from(index).ok()?) != Some(number)
                || reference.style != style
                || (width - reference.width).abs() > 0.1
                || (height - reference.height).abs() > 0.1
                || (edge - reference.edge).abs() > 1.0
                || (offset - reference.offset).abs() > 1.0
            {
                return None;
            }
        } else {
            first = Some(Reference {
                number,
                style,
                width,
                height,
                edge,
                offset,
            });
        }
        result.push((page.page, line));
    }
    Some(result)
}

/// 数字の向きとem矩形が原本から確認できる場合だけ、水平footerへの変換を許す。
fn confirmed_upright(texts: &[&ParsedTextElement]) -> bool {
    texts
        .iter()
        .filter(|text| !text.text.trim().is_empty())
        .all(|text| {
            text.baseline_y.is_some_and(|baseline| {
                baseline.is_finite()
                    && (baseline - text.bounds.y0).abs() <= 0.01
                    && (text.bounds.y1 - text.bounds.y0 - text.font_size).abs() <= 0.01
            })
        })
}

/// 中央の下端領域にある有限の数字だけを読み、内部の空白や符号を消して番号にしない。
fn number(line: &StructuredTextLine, width: f32, height: f32) -> Option<u32> {
    let em = structured_line_max_font_size(line);
    if ![width, height, line.top_y, line.left_x, line.right_x, em]
        .iter()
        .all(|v| v.is_finite())
        || width <= 0.0
        || height <= 0.0
        || em <= 0.0
        || line.top_y <= em
        || line.top_y > height * 0.12
        || line.left_x < 0.0
        || line.right_x > width
        || line.left_x >= line.right_x
        || ((line.left_x + line.right_x) / 2.0 - width / 2.0).abs() > em.max(2.0)
        || line
            .runs
            .iter()
            .any(|r| pdf_font_size_to_half_points(r.style.font_size).is_none())
        || line.text.is_empty()
        || !line.text.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    line.text.parse().ok()
}

/// 数字ごとの字数やrun境界を同じ役として扱い、実際のWord書式の変化は保持する。
fn style(line: &StructuredTextLine) -> Vec<Vec<u8>> {
    let mut result = Vec::new();
    for run in &line.runs {
        let properties = docx_run_from_structured_run(run).run_property.build();
        if result.last() != Some(&properties) {
            result.push(properties);
        }
    }
    result
}

/// 背景や画像と重なる文字を単独footerへ移し、原本の可視性を変える推測を避ける。
fn overlaps_decoration(parsed: &ParsedDocument, page: usize, line: &StructuredTextLine) -> bool {
    let bottom = line.top_y - structured_line_max_font_size(line);
    parsed
        .graphics
        .iter()
        .filter(|g| g.page == page)
        .map(|g| g.bounds)
        .chain(
            parsed
                .image_elements
                .iter()
                .filter(|i| i.page == page)
                .map(|i| i.bounds),
        )
        .map(normalized_bounds)
        .any(|b| {
            b.x0 <= line.right_x && b.x1 >= line.left_x && b.y0 <= line.top_y && b.y1 >= bottom
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 向きが未確認の数字を、有限の矩形と連番だけで水平footerとして確定しない。
    #[test]
    fn footer_candidates_require_confirmed_upright_source_baselines() {
        let mut parsed = ParsedDocument {
            quality: Default::default(),
            path: "unverified-footer-direction.pdf".into(),
            byte_len: 1,
            header: "%PDF-1.7".into(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 7 },
            page_count: 4,
            pages: Vec::new(),
            graphics: Vec::new(),
            text_elements: Vec::new(),
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
        };
        let mut pages = Vec::new();
        for page in 0..4 {
            parsed.pages.push(ParsedPage {
                page_number: page,
                width: 612.0,
                height: 792.0,
                rotation: 0,
            });
            let text = format!("Body{page}");
            pages.push(StructuredTextPage {
                page,
                lines: vec![StructuredTextLine {
                    top_y: 712.0,
                    left_x: 72.0,
                    right_x: 150.0,
                    alignment: ParagraphAlignment::Left,
                    runs: vec![StructuredTextRun {
                        text: text.clone(),
                        style: Default::default(),
                    }],
                    split_from_row: false,
                    text: text.clone(),
                }],
            });
            parsed.text_elements.push(ParsedTextElement {
                source_background: None,
                fill_stroke_width: None,
                page,
                text,
                font_size: 12.0,
                font_name: Some("Helvetica".into()),
                font_weight: None,
                fill_color: None,
                baseline_y: Some(700.0),
                bounds: ParsedBounds {
                    x0: 72.0,
                    x1: 150.0,
                    y0: 700.0,
                    y1: 712.0,
                },
            });
            parsed.text_elements.push(ParsedTextElement {
                source_background: None,
                fill_stroke_width: None,
                page,
                text: (page + 1).to_string(),
                font_size: 11.04,
                font_name: Some("Helvetica".into()),
                font_weight: None,
                fill_color: None,
                baseline_y: None,
                bounds: ParsedBounds {
                    x0: 303.0,
                    x1: 309.0,
                    y0: 52.0,
                    y1: 63.04,
                },
            });
        }
        assert!(
            select(&parsed, &pages).is_empty(),
            "a source bounding box does not verify horizontal writing"
        );
    }
}
