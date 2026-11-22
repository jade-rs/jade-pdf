//! 表セルへ割り当てた PDF 文字断片から、読み順どおりのセル本文を組み立てる。
//!
//! 断片は文字単位でも単語単位でもよい。同じ行の断片は間隔から空白の要否を決め、
//! 行の右端がセル右端の近くで終わる行は折り返しとして次行へ連結し、
//! それ以外の行末は `\n`（セル内の段落区切り）として残す。

use jade_pdf2docx_parser::ParsedBounds;

/// セルへ割り当てた 1 つの文字断片。
#[derive(Debug, Clone)]
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

/// セル断片を読み順の本文へ組み立てる。
///
/// `cell_left`・`cell_right` はセルの左右端（pt）。左余白と同じ右余白を仮定し、
/// 次行の先頭語が前行の残り幅に収まらない場合だけ折り返しとして連結する。
pub(crate) fn compose_cell_text(
    fragments: &[CellFragment],
    cell_left: f32,
    cell_right: f32,
) -> String {
    let lines = group_lines(fragments);
    let padding = lines
        .iter()
        .map(|line| line.left - cell_left)
        .fold(f32::INFINITY, f32::min)
        .clamp(0.0, (cell_right - cell_left).max(0.0) / 4.0);
    let content_right = cell_right - padding;
    let mut output = String::new();
    let mut previous: Option<&CellLine<'_>> = None;
    let mut previous_text = String::new();
    for line in &lines {
        let text = line_text(line);
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
            }
        }
        output.push_str(&text);
        previous = Some(line);
        previous_text = text;
    }
    output
}

/// 数字・桁区切り・小数点だけで書かれた行か判定する。
fn is_mostly_numeric(text: &str) -> bool {
    let body = text.trim();
    !body.is_empty()
        && body.chars().all(|ch| {
            ch.is_ascii_digit()
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
    sorted.sort_by(|a, b| b.center_y().total_cmp(&a.center_y()));
    let mut lines: Vec<CellLine<'_>> = Vec::new();
    for fragment in sorted {
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
fn line_text(line: &CellLine<'_>) -> String {
    let mut text = String::new();
    let mut previous: Option<&CellFragment> = None;
    let mut pending_space = false;
    for fragment in &line.fragments {
        let body = fragment.text.trim();
        if let Some(previous) = previous {
            let gap = fragment.bounds.x0 - previous.bounds.x1;
            let size = previous.size().min(fragment.size());
            let wide_gap = gap > WORD_GAP_EM * size;
            let explicit = pending_space || fragment.text.starts_with(char::is_whitespace);
            if gap > TAB_GAP_EM * size {
                text.push('\t');
            } else if (wide_gap || explicit)
                && needs_word_space(text.chars().last(), body.chars().next())
            {
                text.push(' ');
            }
        }
        text.push_str(body);
        pending_space = fragment.text.ends_with(char::is_whitespace);
        previous = Some(fragment);
    }
    text
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
    let weights = chars
        .iter()
        .map(|ch| {
            if is_cjk(*ch) {
                1.0
            } else if ch.is_whitespace() {
                0.4
            } else {
                0.55
            }
        })
        .collect::<Vec<f32>>();
    let total = weights.iter().sum::<f32>().max(f32::EPSILON);
    let scale = (bounds.x1 - bounds.x0) / total;
    // edges[i] は i 文字目の左端の推定 x。
    let mut edges = Vec::with_capacity(chars.len() + 1);
    let mut x = bounds.x0;
    for weight in &weights {
        edges.push(x);
        x += weight * scale;
    }
    edges.push(bounds.x1);
    let em = fragment.size();
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
                x0: 90.0,
                y0: 0.0,
                x1: 290.0,
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
}
