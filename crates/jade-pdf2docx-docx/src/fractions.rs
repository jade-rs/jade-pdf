//! 分数（分子・横線・分母を上下に組んだもの）を、本文の行の中の「分子/分母」に戻す。
//!
//! PDF の分数は、行の上半分に小さな分子、下半分に分母を置き、間に短い横線を引く。
//! そのままでは分子と分母が本文の行とは別の行になり、「負担割合は とし」のように本文から
//! 抜け落ちる。lawrenceanum の kanpo-parser と同じく、短い横線を挟んで上下にそろった数値を
//! 分数とみなす。

use super::*;

/// 分数の数字（数字と小数点・桁区切り）だけの文字列か。
fn is_number(text: &str) -> bool {
    let body = text.trim();
    !body.is_empty()
        && body.chars().any(|ch| ch.is_ascii_digit())
        && body
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ','))
}

/// 分子と分母の間の短い横線（罫線か細い塗りつぶし）の範囲。
fn bars(parsed: &ParsedDocument, page: usize) -> Vec<ParsedBounds> {
    let lines = parsed
        .graphics
        .iter()
        .filter(|graphic| graphic.page == page)
        .map(|graphic| normalized_bounds(graphic.bounds))
        .filter(is_horizontal_line);
    let fills = parsed
        .fills
        .iter()
        .filter(|fill| fill.page == page)
        .map(|fill| normalized_bounds(fill.bounds))
        .filter(|bounds| bounds.y1 - bounds.y0 <= 1.5 && bounds.x1 - bounds.x0 >= 2.0);
    lines
        .chain(fills)
        .filter(|bounds| bounds.x1 - bounds.x0 <= 60.0)
        .collect()
}

/// 同じ高さで字間を空けずに続く数字の文字片のまとまり（字ごとに分かれた数字を 1 つにする）。
struct Number {
    /// 文字片の番号（左から順）。
    members: Vec<usize>,
    /// まとまりの範囲。
    bounds: ParsedBounds,
    /// 数字の文字列。
    text: String,
    /// 文字の大きさ。
    font_size: f32,
    /// ページ番号。
    page: usize,
}

/// 数字の文字片を、同じ高さで続くまとまりにする。
fn numbers(parsed: &ParsedDocument) -> Vec<Number> {
    // 数字の間の小数点・桁区切りの文字片もまとまりに含める（数の一部を分子と取り違えない）。
    let is_part = |text: &str| is_number(text) || matches!(text.trim(), "." | ",");
    let mut indices = (0..parsed.text_elements.len())
        .filter(|&index| is_part(&parsed.text_elements[index].text))
        .collect::<Vec<_>>();
    indices.sort_by(|&a, &b| {
        let (a, b) = (&parsed.text_elements[a], &parsed.text_elements[b]);
        a.page.cmp(&b.page).then(
            normalized_bounds(a.bounds)
                .x0
                .total_cmp(&normalized_bounds(b.bounds).x0),
        )
    });
    let mut out: Vec<Number> = Vec::new();
    for index in indices {
        let text = &parsed.text_elements[index];
        let bounds = normalized_bounds(text.bounds);
        let size = (bounds.y1 - bounds.y0).max(1.0);
        if let Some(number) = out.iter_mut().find(|number| {
            number.page == text.page
                && (number.font_size - text.font_size).abs() <= 0.5
                && (number.bounds.y0 - bounds.y0).abs() <= 0.5 * size
                && bounds.x0 >= number.bounds.x0
                && bounds.x0 - number.bounds.x1 <= 0.25 * size
        }) {
            number.members.push(index);
            number.text.push_str(text.text.trim());
            number.bounds = ParsedBounds {
                x0: number.bounds.x0,
                y0: number.bounds.y0.min(bounds.y0),
                x1: number.bounds.x1.max(bounds.x1),
                y1: number.bounds.y1.max(bounds.y1),
            };
            continue;
        }
        out.push(Number {
            members: vec![index],
            bounds,
            text: text.text.trim().to_string(),
            font_size: text.font_size,
            page: text.page,
        });
    }
    out.retain(|number| is_number(&number.text));
    out
}

/// 分数を見つけ、分子の文字片を本文の行の中の「分子/分母」に書き換える。
pub(super) fn join(parsed: &mut ParsedDocument) {
    let numbers = numbers(parsed);
    let mut remove = BTreeSet::new();
    let mut used_bars: Vec<(usize, ParsedBounds)> = Vec::new();
    let mut used = BTreeSet::new();
    let mut rewrites = Vec::new();
    for (top_index, top) in numbers.iter().enumerate() {
        if used.contains(&top_index) {
            continue;
        }
        let top_bounds = top.bounds;
        let size = (top_bounds.y1 - top_bounds.y0).max(1.0);
        let center = (top_bounds.x0 + top_bounds.x1) / 2.0;
        let Some((bottom_index, bottom)) = numbers.iter().enumerate().find(|(index, bottom)| {
            let bounds = bottom.bounds;
            *index != top_index
                && !used.contains(index)
                && bottom.page == top.page
                && ((bounds.x0 + bounds.x1) / 2.0 - center).abs() <= 0.5 * size
                && bounds.y1 <= top_bounds.y0 + 0.2 * size
                && top_bounds.y0 - bounds.y1 <= 0.8 * size
                && (bottom.font_size - top.font_size).abs() <= 0.5
        }) else {
            continue;
        };
        let bottom_bounds = bottom.bounds;
        let bar_y = (top_bounds.y0 + bottom_bounds.y1) / 2.0;
        let width = (top_bounds.x1 - top_bounds.x0).max(bottom_bounds.x1 - bottom_bounds.x0);
        let Some(bar) = bars(parsed, top.page).into_iter().find(|bar| {
            (bar.y0 - bar_y).abs() <= 0.5 * size
                && bar.x0 <= center
                && bar.x1 >= center
                // 横線は分子・分母の広い方とほぼ同じ幅（下線や表の罫線は数より広い・狭い）。
                && bar.x1 - bar.x0 >= width - 0.5
                && bar.x1 - bar.x0 <= width + size
        }) else {
            continue;
        };
        // 分数の上下の中央にかかる本文の行の高さに置く（なければ分子と分母を合わせた範囲）。
        // 分子・分母は本文より小さく組み、本文の行の上下にまたがる（表の中で下線の下に
        // 合計を置いた数値の列を分数と取り違えないよう、本文の行が無ければ分数としない）。
        let Some(line) = parsed
            .text_elements
            .iter()
            .filter(|text| text.page == top.page && top.font_size <= 0.85 * text.font_size)
            .map(|text| (normalized_bounds(text.bounds), text.font_size))
            .find(|(bounds, line_size)| {
                let left = top_bounds.x0.min(bottom_bounds.x0);
                let right = top_bounds.x1.max(bottom_bounds.x1);
                bounds.y0 <= bar_y
                    && bounds.y1 >= bar_y
                    // 分子・分母は本文の行の高さに収まる（表の上下の行の数ではない）
                    && top_bounds.y1 > bounds.y1 - 0.2 * size
                    && top_bounds.y1 <= bounds.y1 + size
                    && bottom_bounds.y0 < bounds.y0 + 0.2 * size
                    && bottom_bounds.y0 >= bounds.y0 - size
                    // 本文の字のすぐ隣に置く
                    && (bounds.x1 <= left + 0.5 && left - bounds.x1 <= 1.5 * line_size
                        || bounds.x0 >= right - 0.5 && bounds.x0 - right <= 1.5 * line_size)
            })
            .map(|(bounds, _)| bounds)
        else {
            continue;
        };
        let bounds = ParsedBounds {
            x0: top_bounds.x0.min(bottom_bounds.x0),
            x1: top_bounds.x1.max(bottom_bounds.x1),
            y0: line.y0,
            y1: line.y1,
        };
        rewrites.push((
            top.members[0],
            format!("{}/{}", top.text, bottom.text),
            bounds,
        ));
        remove.extend(top.members.iter().skip(1).copied());
        remove.extend(bottom.members.iter().copied());
        used.insert(top_index);
        used.insert(bottom_index);
        used_bars.push((top.page, bar));
    }
    for (index, text, bounds) in rewrites {
        let element = &mut parsed.text_elements[index];
        element.text = text;
        element.bounds = bounds;
        element.baseline_y = None;
    }
    let mut index = 0;
    parsed.text_elements.retain(|_| {
        let keep = !remove.contains(&index);
        index += 1;
        keep
    });
    let is_used = |page: usize, bounds: ParsedBounds| {
        used_bars.iter().any(|(bar_page, bar)| {
            *bar_page == page
                && (bar.x0 - bounds.x0).abs() < 0.01
                && (bar.y0 - bounds.y0).abs() < 0.01
                && (bar.x1 - bounds.x1).abs() < 0.01
        })
    };
    parsed
        .graphics
        .retain(|graphic| !is_used(graphic.page, normalized_bounds(graphic.bounds)));
    parsed
        .fills
        .retain(|fill| !is_used(fill.page, normalized_bounds(fill.bounds)));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト用の文字片。
    fn text(body: &str, x0: f32, y0: f32, x1: f32, size: f32) -> ParsedTextElement {
        ParsedTextElement {
            baseline_y: None,
            page: 0,
            bounds: ParsedBounds {
                x0,
                y0,
                x1,
                y1: y0 + size,
            },
            font_size: size,
            font_name: None,
            fill_color: None,
            text: body.to_string(),
        }
    }

    /// テスト用の解析結果。
    fn document(text_elements: Vec<ParsedTextElement>, bar: ParsedBounds) -> ParsedDocument {
        ParsedDocument {
            quality: QualityReport::default(),
            path: std::path::PathBuf::new(),
            byte_len: 0,
            header: "%PDF-1.4".into(),
            version: jade_pdf2docx_parser::PdfVersion { major: 1, minor: 4 },
            page_count: 1,
            pages: Vec::new(),
            graphics: vec![ParsedGraphicElement {
                page: 0,
                bounds: bar,
                stroke_width: 0.5,
                dashed: false,
            }],
            fills: Vec::new(),
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
            text_elements,
        }
    }

    /// 本文の行の上下にまたがる小さな分子・横線・分母は「分子/分母」になる。
    #[test]
    fn stacked_numbers_with_a_bar_become_a_fraction() {
        let mut parsed = document(
            vec![
                text("負担割合は", 63.0, 758.2, 123.0, 12.0),
                text("7", 127.7, 764.8, 131.9, 8.0),
                text("5", 131.9, 764.8, 136.1, 8.0),
                text("100", 125.6, 751.3, 138.2, 8.0),
                text("とする", 140.0, 758.2, 176.0, 12.0),
            ],
            ParsedBounds {
                x0: 125.6,
                y0: 763.0,
                x1: 138.2,
                y1: 763.0,
            },
        );
        join(&mut parsed);
        let texts = parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["負担割合は", "75/100", "とする"]);
        assert!(parsed.graphics.is_empty());
    }

    /// 表の下線の上下の数値（同じ大きさの本文の行が無い）は分数にしない。
    #[test]
    fn underlined_amount_above_another_is_not_a_fraction() {
        let mut parsed = document(
            vec![
                text("2,512", 100.0, 120.0, 120.0, 8.0),
                text("12,962", 96.0, 108.0, 120.0, 8.0),
            ],
            ParsedBounds {
                x0: 96.0,
                y0: 119.0,
                x1: 120.0,
                y1: 119.0,
            },
        );
        join(&mut parsed);
        assert_eq!(parsed.text_elements.len(), 2);
        assert_eq!(parsed.graphics.len(), 1);
    }
}
