//! 表セルへ割り当てた PDF 文字断片から、読み順どおりのセル本文を組み立てる。
//!
//! 断片は文字単位でも単語単位でもよい。同じ行の断片は間隔から空白の要否を決め、
//! 行の右端がセル右端の近くで終わる行は折り返しとして次行へ連結し、
//! それ以外の行末は `\n`（セル内の段落区切り）として残す。

use jade_pdf2docx_parser::ParsedBounds;

/// セルへ割り当てた 1 つの文字断片。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CellFragment {
    /// 断片の外接矩形（PDF 座標、y 上向き）。
    pub(crate) bounds: ParsedBounds,
    /// 抽出した文字列。前後の空白は語間の手がかりとして保持する。
    pub(crate) text: String,
    /// 断片の font size（pt）。0 以下の場合は矩形の高さで代用する。
    pub(crate) font_size: f32,
}

impl CellFragment {
    /// 行判定・空白判定に使う実効的な文字サイズ。
    fn size(&self) -> f32 {
        let height = (self.bounds.y1 - self.bounds.y0).abs();
        if self.font_size.is_finite() && self.font_size > 0.0 {
            self.font_size
        } else {
            height.max(1.0)
        }
    }

    /// 縦方向の中心。
    fn center_y(&self) -> f32 {
        (self.bounds.y0 + self.bounds.y1) / 2.0
    }
}

/// 同じ行にまとめた断片列。
struct CellLine<'a> {
    /// x 順に並べた断片。
    fragments: Vec<&'a CellFragment>,
    /// 行の左端。
    left: f32,
    /// 行の右端。
    right: f32,
    /// 行の下端。
    bottom: f32,
    /// 行の上端。
    top: f32,
}

/// 同じ行とみなす縦方向の重なり率（小さい方の高さに対する比）。
const SAME_LINE_OVERLAP: f32 = 0.5;
/// 語間の空白とみなす水平間隔（em）。
const WORD_GAP_EM: f32 = 0.2;
/// 行内でこの em 数を超えて離れた断片の間はタブ（`\t`）で区切る。
const TAB_GAP_EM: f32 = 1.5;
/// 折り返し判定で、次行の先頭語が収まったはずかを比べる際の許容幅（em）。
const WRAP_TOLERANCE_EM: f32 = 0.3;

/// セル内のタブ位置（PDF の x 座標、pt）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CellTabStop {
    /// タブで揃える位置（右揃えなら語の右端、左揃えなら語の左端）。
    pub(crate) x: f32,
    /// 次の語を右揃え（数値）にするか。
    pub(crate) right: bool,
}

/// 組み立てたセル本文と、`\n` で区切った各段落のタブ位置。
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ComposedCell {
    /// セル本文（`\n` は段落区切り、`\t` はタブ）。
    pub(crate) text: String,
    /// 段落ごとのタブ位置。
    pub(crate) tab_stops: Vec<Vec<CellTabStop>>,
}

/// セル断片を読み順の本文へ組み立てる。
#[cfg(test)]
pub(crate) fn compose_cell_text(
    fragments: &[CellFragment],
    cell_left: f32,
    cell_right: f32,
) -> String {
    compose_cell(fragments, cell_left, cell_right).text
}

/// セル断片を読み順の本文とタブ位置へ組み立てる。
///
/// `cell_left`・`cell_right` はセルの左右端（pt）。左余白と同じ右余白を仮定し、
/// 次行の先頭語が前行の残り幅に収まらない場合だけ折り返しとして連結する。
pub(crate) fn compose_cell(
    fragments: &[CellFragment],
    cell_left: f32,
    cell_right: f32,
) -> ComposedCell {
    // 字間調整で語の間を大きく空けた文字片は、語ごとの位置を推定して分ける。
    let words = fragments
        .iter()
        .flat_map(split_at_wide_spaces)
        .collect::<Vec<_>>();
    let mut sorted = words
        .iter()
        .filter(|fragment| !fragment.text.trim().is_empty())
        .collect::<Vec<_>>();
    let lines = group_sorted_lines(&mut sorted);
    let padding = lines
        .iter()
        .map(|line| line.left - cell_left)
        .fold(f32::INFINITY, f32::min)
        .clamp(0.0, (cell_right - cell_left).max(0.0) / 4.0);
    let content_right = cell_right - padding;
    let mut output = String::new();
    let mut tab_stops: Vec<Vec<CellTabStop>> = Vec::new();
    let mut previous: Option<&CellLine<'_>> = None;
    let mut previous_text = String::new();
    for line in &lines {
        let (text, stops) = line_text(line);
        if text.is_empty() {
            continue;
        }
        if let Some(previous) = previous {
            let size = line_size(previous);
            let first = first_word_width(line, text.chars().next());
            let space = if needs_word_space(output.chars().last(), text.chars().next()) {
                size * 0.25
            } else {
                0.0
            };
            // 項目と数値を離して並べた行や数値だけの行は、右端に届いていても折り返しではない。
            let tabular = previous_text.contains('\t') || is_mostly_numeric(&previous_text);
            let wrapped = !tabular
                && previous.right + space + first > content_right + WRAP_TOLERANCE_EM * size;
            if wrapped {
                if space > 0.0 {
                    output.push(' ');
                }
            } else {
                output.push('\n');
                tab_stops.push(Vec::new());
            }
        } else {
            tab_stops.push(Vec::new());
        }
        if let Some(paragraph) = tab_stops.last_mut() {
            paragraph.extend(stops);
        }
        output.push_str(&text);
        previous = Some(line);
        previous_text = text;
    }
    ComposedCell {
        text: output,
        tab_stops,
    }
}

/// 数字・桁区切り・小数点だけで書かれた行か判定する。
///
/// 数字を含み、数字以外は桁区切り・小数点・符号か、ToUnicode のない字形の私用領域の記号
/// （小数点などが私用領域で抽出される）だけの場合に数値とみなす。
fn is_mostly_numeric(text: &str) -> bool {
    let body = text.trim();
    !body.is_empty()
        && body
            .chars()
            .any(|ch| ch.is_ascii_digit() || ('０'..='９').contains(&ch))
        && body.chars().all(|ch| {
            ('\u{E000}'..='\u{F8FF}').contains(&ch)
                || ch.is_ascii_digit()
                || ('０'..='９').contains(&ch)
                || matches!(
                    ch,
                    ',' | '.' | '，' | '．' | '%' | '△' | '▲' | '-' | '－' | ' '
                )
        })
}

/// セル断片が並ぶ行数を返す。
pub(crate) fn cell_line_count(fragments: &[CellFragment]) -> usize {
    group_lines(fragments).len()
}

/// 行頭の語の幅。和文は 1 文字、欧文は語間の空白までを語とみなす。
fn first_word_width(line: &CellLine<'_>, first_char: Option<char>) -> f32 {
    let Some(first) = line.fragments.first() else {
        return 0.0;
    };
    let char_width = |fragment: &CellFragment| {
        let count = fragment.text.trim().chars().count().max(1) as f32;
        (fragment.bounds.x1 - fragment.bounds.x0).max(0.0) / count
    };
    if first_char.is_some_and(is_cjk) {
        return char_width(first);
    }
    let body = first.text.trim_start();
    if let Some(end) = body.find(char::is_whitespace) {
        return char_width(first) * body[..end].chars().count() as f32;
    }
    let mut right = first.bounds.x1;
    if first.text.ends_with(char::is_whitespace) {
        return right - first.bounds.x0;
    }
    for fragment in line.fragments.iter().skip(1) {
        let gap = fragment.bounds.x0 - right;
        if gap > WORD_GAP_EM * fragment.size() || fragment.text.starts_with(char::is_whitespace) {
            break;
        }
        let body = fragment.text.trim();
        if let Some(end) = body.find(char::is_whitespace) {
            right = fragment.bounds.x0 + char_width(fragment) * body[..end].chars().count() as f32;
            break;
        }
        right = fragment.bounds.x1;
        if fragment.text.ends_with(char::is_whitespace) {
            break;
        }
    }
    right - first.bounds.x0
}

/// 断片を上から下の行へまとめる。
fn group_lines(fragments: &[CellFragment]) -> Vec<CellLine<'_>> {
    let mut sorted = fragments
        .iter()
        .filter(|fragment| !fragment.text.trim().is_empty())
        .collect::<Vec<_>>();
    group_sorted_lines(&mut sorted)
}

/// 文字片を上から下の行へまとめる（呼び出し側が所有する文字片の参照列）。
fn group_sorted_lines<'a>(sorted: &mut Vec<&'a CellFragment>) -> Vec<CellLine<'a>> {
    sorted.sort_by(|a, b| b.center_y().total_cmp(&a.center_y()));
    let mut lines: Vec<CellLine<'_>> = Vec::new();
    for fragment in sorted.drain(..) {
        let (bottom, top) = (
            fragment.bounds.y0.min(fragment.bounds.y1),
            fragment.bounds.y0.max(fragment.bounds.y1),
        );
        let target = lines.iter_mut().find(|line| {
            let overlap = top.min(line.top) - bottom.max(line.bottom);
            let smaller = (top - bottom).min(line.top - line.bottom).max(0.1);
            overlap / smaller >= SAME_LINE_OVERLAP
        });
        match target {
            Some(line) => {
                line.fragments.push(fragment);
                line.left = line.left.min(fragment.bounds.x0);
                line.right = line.right.max(fragment.bounds.x1);
                line.bottom = line.bottom.min(bottom);
                line.top = line.top.max(top);
            }
            None => lines.push(CellLine {
                fragments: vec![fragment],
                left: fragment.bounds.x0,
                right: fragment.bounds.x1,
                bottom,
                top,
            }),
        }
    }
    for line in &mut lines {
        line.fragments
            .sort_by(|a, b| a.bounds.x0.total_cmp(&b.bounds.x0));
    }
    lines.sort_by(|a, b| b.top.total_cmp(&a.top));
    lines
}

/// 行内の代表文字サイズ（最大値）。
fn line_size(line: &CellLine<'_>) -> f32 {
    line.fragments
        .iter()
        .map(|fragment| fragment.size())
        .fold(1.0, f32::max)
}

/// 1 行の断片を間隔に応じて連結する。
fn line_text(line: &CellLine<'_>) -> (String, Vec<CellTabStop>) {
    let mut text = String::new();
    let mut previous: Option<&CellFragment> = None;
    let mut pending_space = false;
    // タブの後の語の（左端, 右端, 本文）。
    let mut segments: Vec<(f32, f32, String)> = Vec::new();
    for fragment in &line.fragments {
        let body = fragment.text.trim();
        if let Some(previous) = previous {
            let gap = fragment.bounds.x0 - previous.bounds.x1;
            let size = previous.size().min(fragment.size());
            let wide_gap = gap > WORD_GAP_EM * size;
            let explicit = pending_space || fragment.text.starts_with(char::is_whitespace);
            if gap > TAB_GAP_EM * size {
                text.push('\t');
                segments.push((fragment.bounds.x0, fragment.bounds.x1, String::new()));
            } else if (wide_gap || explicit)
                && needs_word_space(text.chars().last(), body.chars().next())
            {
                text.push(' ');
                if let Some(segment) = segments.last_mut() {
                    segment.2.push(' ');
                }
            }
        }
        text.push_str(body);
        if let Some(segment) = segments.last_mut() {
            segment.1 = segment.1.max(fragment.bounds.x1);
            segment.2.push_str(body);
        }
        pending_space = fragment.text.ends_with(char::is_whitespace);
        previous = Some(fragment);
    }
    let stops = segments
        .into_iter()
        .map(|(left, right, body)| {
            if is_mostly_numeric(&body) {
                CellTabStop {
                    x: right,
                    right: true,
                }
            } else {
                CellTabStop {
                    x: left,
                    right: false,
                }
            }
        })
        .collect();
    (text, stops)
}

/// 2 文字の間に語間の空白が必要か判定する（和文同士・和文と記号の間は不要）。
fn needs_word_space(left: Option<char>, right: Option<char>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => !is_cjk(left) && !is_cjk(right),
        _ => false,
    }
}

/// 和文の文字（かな・漢字・全角記号）か判定する。
fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x3000..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

/// 空白を含む文字片の幅が字の公称幅の合計より大きく、空白 1 つあたりの空きが
/// 語間を超える場合、空白の位置で語ごとの文字片へ分ける。
///
/// 位置は字の公称幅（全角 1 em、半角 0.55 em）で推定し、余りの幅を空白へ配分する。
fn split_at_wide_spaces(fragment: &CellFragment) -> Vec<CellFragment> {
    let body = fragment.text.trim();
    let chars = body.chars().collect::<Vec<_>>();
    let spaces = chars.iter().filter(|ch| ch.is_whitespace()).count();
    if spaces == 0 {
        return vec![fragment.clone()];
    }
    let em = fragment.size();
    let width = |ch: &char| if is_cjk(*ch) { em } else { 0.55 * em };
    let glyphs = chars
        .iter()
        .filter(|ch| !ch.is_whitespace())
        .map(width)
        .sum::<f32>();
    let span = fragment.bounds.x1 - fragment.bounds.x0;
    let gap = (span - glyphs) / spaces as f32;
    if gap <= TAB_GAP_EM * em {
        return vec![fragment.clone()];
    }
    let mut pieces = Vec::new();
    let mut x = fragment.bounds.x0;
    let mut word = String::new();
    let mut word_x0 = x;
    for ch in chars {
        if ch.is_whitespace() {
            if !word.is_empty() {
                pieces.push(CellFragment {
                    bounds: ParsedBounds {
                        x0: word_x0,
                        x1: x,
                        ..fragment.bounds
                    },
                    text: std::mem::take(&mut word),
                    font_size: fragment.font_size,
                });
            }
            x += gap;
            word_x0 = x;
        } else {
            word.push(ch);
            x += width(&ch);
        }
    }
    if !word.is_empty() {
        pieces.push(CellFragment {
            bounds: ParsedBounds {
                x0: word_x0,
                x1: x.min(fragment.bounds.x1).max(word_x0),
                ..fragment.bounds
            },
            text: word,
            font_size: fragment.font_size,
        });
    }
    pieces
}

/// 列境界をまたぐ 1 つのテキスト要素を、境界ごとの断片へ分ける。
///
/// 文字ごとの位置は分からないため、全角を 1、半角を 0.55 とした字幅の比で推定する。
/// 境界の近く（3 em 以内）に空白があればそこで分け、なければ推定位置で分ける。
pub(crate) fn split_across_boundaries(
    fragment: &CellFragment,
    boundaries: &[f32],
) -> Vec<CellFragment> {
    let bounds = fragment.bounds;
    let inside = boundaries
        .iter()
        .copied()
        .filter(|boundary| *boundary > bounds.x0 + 1.0 && *boundary < bounds.x1 - 1.0)
        .collect::<Vec<_>>();
    let chars = fragment.text.chars().collect::<Vec<_>>();
    if inside.is_empty() || chars.len() < 2 {
        return vec![fragment.clone()];
    }
    // 字は公称幅（全角 1 em、半角 0.55 em）とし、矩形の残りの幅を空白へ配分する。
    // 空白がない、または字の公称幅の合計が矩形より広い場合は比例配分にする。
    let em = fragment.size();
    let nominal = chars
        .iter()
        .map(|ch| {
            if ch.is_whitespace() {
                0.0
            } else if is_cjk(*ch) {
                em
            } else {
                0.55 * em
            }
        })
        .collect::<Vec<f32>>();
    let span = bounds.x1 - bounds.x0;
    let glyphs = nominal.iter().sum::<f32>();
    let spaces = chars.iter().filter(|ch| ch.is_whitespace()).count();
    let widths = if spaces > 0 && glyphs < span {
        let extra = (span - glyphs) / spaces as f32;
        chars
            .iter()
            .zip(&nominal)
            .map(|(ch, width)| if ch.is_whitespace() { extra } else { *width })
            .collect::<Vec<_>>()
    } else {
        let total = nominal.iter().sum::<f32>().max(f32::EPSILON);
        nominal
            .iter()
            .zip(&chars)
            .map(|(width, ch)| {
                let width = if ch.is_whitespace() { 0.4 * em } else { *width };
                width * span / total
            })
            .collect::<Vec<_>>()
    };
    // edges[i] は i 文字目の左端の推定 x。
    let mut edges = Vec::with_capacity(chars.len() + 1);
    let mut x = bounds.x0;
    for width in &widths {
        edges.push(x);
        x += width;
    }
    edges.push(bounds.x1);
    let mut cuts = Vec::new();
    for boundary in inside {
        let space = (0..chars.len())
            .filter(|index| chars[*index].is_whitespace())
            .map(|index| {
                (
                    index,
                    ((edges[index] + edges[index + 1]) / 2.0 - boundary).abs(),
                )
            })
            .filter(|(_, distance)| *distance <= 3.0 * em)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index);
        let cut = space.unwrap_or_else(|| {
            (1..chars.len())
                .min_by(|a, b| {
                    (edges[*a] - boundary)
                        .abs()
                        .total_cmp(&(edges[*b] - boundary).abs())
                })
                .unwrap_or(1)
        });
        if !cuts.iter().any(|(existing, _)| *existing == cut) {
            cuts.push((cut, boundary));
        }
    }
    cuts.sort_by_key(|(cut, _)| *cut);
    let mut pieces = Vec::new();
    let mut start = 0usize;
    let mut left_limit = f32::NEG_INFINITY;
    for (cut, right_limit) in cuts
        .into_iter()
        .chain(std::iter::once((chars.len(), f32::INFINITY)))
    {
        let text = chars[start..cut].iter().collect::<String>();
        if !text.trim().is_empty() {
            let first = start
                + chars[start..cut]
                    .iter()
                    .take_while(|ch| ch.is_whitespace())
                    .count();
            let last = cut
                - chars[start..cut]
                    .iter()
                    .rev()
                    .take_while(|ch| ch.is_whitespace())
                    .count();
            // 推定位置がずれても、分けた境界の間（同じ列）に収める。
            let x0 = edges[first].clamp(left_limit, right_limit);
            let x1 = edges[last].clamp(left_limit, right_limit);
            let (x0, x1) = if x1 - x0 < 0.5 {
                let left = left_limit.max(bounds.x0);
                let right = right_limit.min(bounds.x1);
                (left, right)
            } else {
                (x0, x1)
            };
            pieces.push(CellFragment {
                bounds: ParsedBounds {
                    x0,
                    y0: bounds.y0,
                    x1,
                    y1: bounds.y1,
                },
                text: text.trim().to_string(),
                font_size: fragment.font_size,
            });
        }
        start = cut;
        left_limit = right_limit;
    }
    pieces
}

/// セル内の文字範囲から、横配置と縦位置を推定する。
///
/// 左右（上下）の余白がほぼ等しければ中央、片側だけ狭ければその側へ寄せる。
/// 文字がセル幅いっぱいに広がる場合は左寄せとする。縦位置は、上下の余白の差が
/// 小さい場合や余白の合計が 1 行分に満たない場合は中央とする。
pub(crate) fn infer_cell_alignment(
    text: ParsedBounds,
    cell: ParsedBounds,
) -> (
    crate::ParagraphAlignment,
    Option<crate::CellVerticalAlignment>,
) {
    use crate::{CellVerticalAlignment, ParagraphAlignment};
    let width = (cell.x1 - cell.x0).max(0.0);
    let left = (text.x0 - cell.x0).max(0.0);
    let right = (cell.x1 - text.x1).max(0.0);
    let horizontal = if width <= 0.0 || (left < 0.15 * width && right < 0.15 * width) {
        ParagraphAlignment::Left
    } else if (left - right).abs() <= (0.08 * width).max(2.0) {
        ParagraphAlignment::Center
    } else if right < left / 2.0 {
        ParagraphAlignment::Right
    } else {
        ParagraphAlignment::Left
    };
    let height = (cell.y1 - cell.y0).max(0.0);
    let line = (text.y1 - text.y0).max(1.0);
    let top = (cell.y1 - text.y1).max(0.0);
    let bottom = (text.y0 - cell.y0).max(0.0);
    // 文字矩形の取り方（ベースライン基準か字面か）で 1 行分程度ずれるため、その差までは中央とみなす。
    let tolerance = (0.2 * height).max(line.min(12.0));
    let vertical = if height <= 0.0 || top + bottom < line || (top - bottom).abs() <= tolerance {
        CellVerticalAlignment::Center
    } else if top < bottom {
        CellVerticalAlignment::Top
    } else {
        CellVerticalAlignment::Bottom
    };
    (horizontal, Some(vertical))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 文字ずつの断片を作る。
    fn chars(text: &str, x0: f32, y0: f32, advance: f32, size: f32) -> Vec<CellFragment> {
        text.chars()
            .enumerate()
            .filter(|(_, ch)| *ch != ' ')
            .map(|(index, ch)| CellFragment {
                bounds: ParsedBounds {
                    x0: x0 + advance * index as f32,
                    y0,
                    x1: x0 + advance * (index as f32 + 1.0),
                    y1: y0 + size,
                },
                text: ch.to_string(),
                font_size: size,
            })
            .collect()
    }

    /// 文字単位の断片を空白なしで語に戻し、語間だけ空白を入れる。
    #[test]
    fn joins_character_fragments_into_words() {
        let fragments = chars("Item 1-0", 100.0, 700.0, 6.0, 10.0);
        assert_eq!(compose_cell_text(&fragments, 0.0, 200.0), "Item 1-0");
    }

    /// Pdfium が語末に付けた空白も語間として使う。
    #[test]
    fn uses_trailing_space_marker_as_word_gap() {
        let fragments = vec![
            CellFragment {
                bounds: ParsedBounds {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 20.0,
                    y1: 10.0,
                },
                text: "Name ".into(),
                font_size: 10.0,
            },
            CellFragment {
                bounds: ParsedBounds {
                    x0: 20.5,
                    y0: 0.0,
                    x1: 40.0,
                    y1: 10.0,
                },
                text: "list".into(),
                font_size: 10.0,
            },
        ];
        assert_eq!(compose_cell_text(&fragments, 0.0, 200.0), "Name list");
    }

    /// セル右端まで届いた行は折り返しとして連結し、和文には空白を入れない。
    #[test]
    fn joins_soft_wrapped_japanese_lines() {
        let mut fragments = chars("これは長い説明文で", 0.0, 100.0, 10.0, 10.0);
        fragments.extend(chars("す。", 0.0, 86.0, 10.0, 10.0));
        assert_eq!(
            compose_cell_text(&fragments, 0.0, 95.0),
            "これは長い説明文です。"
        );
    }

    /// 右端より手前で終わる行は段落区切りとして残す。
    #[test]
    fn keeps_hard_line_breaks() {
        let mut fragments = chars("line one", 0.0, 100.0, 6.0, 10.0);
        fragments.extend(chars("line two", 0.0, 86.0, 6.0, 10.0));
        assert_eq!(
            compose_cell_text(&fragments, 0.0, 200.0),
            "line one\nline two"
        );
    }

    /// 欧文の折り返しでは語間の空白を補う。
    #[test]
    fn joins_soft_wrapped_latin_lines_with_space() {
        let mut fragments = chars("alpha beta", 0.0, 100.0, 6.0, 10.0);
        fragments.extend(chars("gamma", 0.0, 86.0, 6.0, 10.0));
        assert_eq!(compose_cell_text(&fragments, 0.0, 65.0), "alpha beta gamma");
    }

    /// 右端に届かなくても、次行の先頭語が収まる余白を残した行末は改行として残す。
    #[test]
    fn keeps_breaks_when_next_word_would_fit() {
        let mut fragments = chars("line one", 4.0, 100.0, 6.0, 10.0);
        fragments.extend(chars("line two", 4.0, 86.0, 6.0, 10.0));
        fragments.extend(chars("line three", 4.0, 72.0, 6.0, 10.0));
        assert_eq!(
            compose_cell_text(&fragments, 0.0, 92.0),
            "line one\nline two\nline three"
        );
    }

    /// 抽出順が乱れていても上から下・左から右で読む。
    #[test]
    fn orders_fragments_by_position() {
        let mut fragments = chars("second", 0.0, 86.0, 6.0, 10.0);
        fragments.extend(chars("first", 0.0, 100.0, 6.0, 10.0));
        fragments.reverse();
        assert_eq!(compose_cell_text(&fragments, 0.0, 200.0), "first\nsecond");
    }

    /// 空白で区切られた複数列の文字列を、各列境界の近くの空白で分ける。
    #[test]
    fn splits_text_crossing_column_rules_at_spaces() {
        let fragment = CellFragment {
            bounds: ParsedBounds {
                x0: 91.0,
                y0: 0.0,
                x1: 251.0,
                y1: 8.0,
            },
            text: "１ 小学校費 教職員数 普通態容補正".into(),
            font_size: 8.0,
        };
        let pieces = split_across_boundaries(&fragment, &[87.0, 155.0, 199.0, 419.0]);
        let texts = pieces
            .iter()
            .map(|piece| piece.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["１ 小学校費", "教職員数", "普通態容補正"]);
        let center = (pieces[1].bounds.x0 + pieces[1].bounds.x1) / 2.0;
        assert!(center > 155.0 && center < 199.0, "{:?}", pieces[1].bounds);
    }

    /// 境界をまたがない要素はそのまま返す。
    #[test]
    fn keeps_fragment_inside_one_column() {
        let fragment = CellFragment {
            bounds: ParsedBounds {
                x0: 10.0,
                y0: 0.0,
                x1: 50.0,
                y1: 8.0,
            },
            text: "inside".into(),
            font_size: 8.0,
        };
        assert_eq!(split_across_boundaries(&fragment, &[0.0, 60.0]).len(), 1);
    }

    /// 項目と右端の数値を離して並べた行は、タブで区切って行ごとに残す。
    #[test]
    fn separates_far_apart_label_and_number_with_tab() {
        let mut fragments = chars("１級地", 0.0, 100.0, 8.0, 8.0);
        fragments.extend(chars("1.061", 80.0, 100.0, 4.0, 8.0));
        fragments.extend(chars("２級地", 0.0, 88.0, 8.0, 8.0));
        fragments.extend(chars("1.049", 80.0, 88.0, 4.0, 8.0));
        assert_eq!(
            compose_cell_text(&fragments, 0.0, 101.0),
            "１級地\t1.061\n２級地\t1.049"
        );
    }

    /// 余白の偏りから横配置と縦位置を推定する。
    #[test]
    fn infers_alignment_from_cell_margins() {
        use crate::{CellVerticalAlignment, ParagraphAlignment};
        let cell = ParsedBounds {
            x0: 0.0,
            y0: 0.0,
            x1: 100.0,
            y1: 60.0,
        };
        let at = |x0: f32, x1: f32, y0: f32, y1: f32| ParsedBounds { x0, y0, x1, y1 };
        assert_eq!(
            infer_cell_alignment(at(3.0, 30.0, 45.0, 55.0), cell),
            (ParagraphAlignment::Left, Some(CellVerticalAlignment::Top))
        );
        assert_eq!(
            infer_cell_alignment(at(40.0, 60.0, 25.0, 35.0), cell),
            (
                ParagraphAlignment::Center,
                Some(CellVerticalAlignment::Center)
            )
        );
        assert_eq!(
            infer_cell_alignment(at(70.0, 97.0, 3.0, 13.0), cell),
            (
                ParagraphAlignment::Right,
                Some(CellVerticalAlignment::Bottom)
            )
        );
    }

    /// 字間調整で大きく空けた語の間は、1 つの文字片でもタブにする。
    #[test]
    fn wide_spaces_inside_one_fragment_become_tabs() {
        let fragment = CellFragment {
            bounds: ParsedBounds {
                x0: 211.0,
                y0: 456.0,
                x1: 371.0,
                y1: 464.0,
            },
            text: "４級地 0".into(),
            font_size: 8.0,
        };
        assert_eq!(compose_cell_text(&[fragment], 199.0, 419.0), "４級地\t0");
    }

    /// タブ位置は、数値なら右端に右揃え、それ以外は左端に左揃えで返す。
    #[test]
    fn tab_stops_follow_pdf_positions() {
        let mut fragments = chars("label", 0.0, 100.0, 4.0, 8.0);
        fragments.extend(chars("1.061", 60.0, 100.0, 4.0, 8.0));
        fragments.extend(chars("note", 100.0, 100.0, 4.0, 8.0));
        let composed = compose_cell(&fragments, 0.0, 130.0);
        assert_eq!(composed.text, "label\t1.061\tnote");
        assert_eq!(
            composed.tab_stops,
            vec![vec![
                CellTabStop {
                    x: 80.0,
                    right: true
                },
                CellTabStop {
                    x: 100.0,
                    right: false
                }
            ]]
        );
    }
}
