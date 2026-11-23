//! 罫線で区切られていない、項目ごとの行が並んだ背の高い行を、文字の行ごとの行に分ける。
//!
//! 貸借対照表のような財務表は、見出しの下に横罫線を引かず、科目と金額の行を何十行も
//! 1 つの枠の中に並べる。罫線だけで行を作ると 1 つのセルに全行が入り、DOCX では科目の列と
//! 金額の列の行が別々に折り返してずれる。列をまたいで同じ高さに字が並び、多くの行に数値が
//! ある背の高い行は、文字の行の間で行を分ける（罫線は引かない）。複数行の文章のセルは、
//! 数値の行が少ないので分けない。

use super::*;

/// 分ける対象にする行に必要な文字の行の数。
const MIN_LINES: usize = 3;
/// 文字の行のうち、2 列以上に字がある行の最小の割合。
const MIN_ALIGNED_RATIO: f32 = 0.6;
/// 文字の行のうち、数値の字片を含む行の最小の割合。
const MIN_NUMERIC_RATIO: f32 = 0.5;

/// 数値（金額・数量）の字片か。
fn is_numeric_text(text: &str) -> bool {
    let body = text.trim().trim_start_matches(['△', '▲', '－', '-']);
    let digits = body.chars().filter(|ch| ch.is_ascii_digit()).count();
    digits > 0
        && body
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, ',' | '.' | ' '))
}

/// 1 つの行の範囲の文字の行（上から順）。各行は (下端, 上端, 字のある列の集合, 数値を含むか)。
fn text_lines_in_band(
    texts: &[&ParsedTextElement],
    xs: &[i32],
    bottom: f32,
    top: f32,
) -> Vec<(f32, f32, BTreeSet<usize>, bool)> {
    let mut lines: Vec<(f32, f32, BTreeSet<usize>, bool)> = Vec::new();
    let mut inside = texts
        .iter()
        .map(|text| (normalized_bounds(text.bounds), *text))
        .filter(|(bounds, _)| {
            let y = (bounds.y0 + bounds.y1) / 2.0;
            y > bottom && y < top
        })
        .collect::<Vec<_>>();
    inside.sort_by(|a, b| b.0.y1.total_cmp(&a.0.y1));
    for (bounds, text) in inside {
        let center_x = (bounds.x0 + bounds.x1) / 2.0;
        let Some(col) = xs
            .windows(2)
            .position(|pair| center_x >= pair[0] as f32 && center_x < pair[1] as f32)
        else {
            continue;
        };
        let center_y = (bounds.y0 + bounds.y1) / 2.0;
        let slack = 0.3 * (bounds.y1 - bounds.y0);
        let numeric = is_numeric_text(&text.text);
        match lines
            .iter_mut()
            .find(|line| center_y > line.0 - slack && center_y < line.1 + slack)
        {
            Some(line) => {
                line.0 = line.0.min(bounds.y0);
                line.1 = line.1.max(bounds.y1);
                line.2.insert(col);
                line.3 |= numeric;
            }
            None => lines.push((bounds.y0, bounds.y1, BTreeSet::from([col]), numeric)),
        }
    }
    lines.sort_by(|a, b| b.1.total_cmp(&a.1));
    lines
}

/// 背の高い行を文字の行ごとに分ける境界を加えた、行の境界（昇順）を返す。
pub(super) fn split_rows_at_text_lines(
    parsed: &ParsedDocument,
    page: usize,
    xs: &[i32],
    ys: &[i32],
) -> Vec<i32> {
    let (Some(left), Some(right)) = (xs.first(), xs.last()) else {
        return ys.to_vec();
    };
    if xs.len() < 3 {
        return ys.to_vec();
    }
    let texts = parsed
        .text_elements
        .iter()
        .filter(|text| text.page == page && !text.text.trim().is_empty())
        .filter(|text| {
            let bounds = normalized_bounds(text.bounds);
            let x = (bounds.x0 + bounds.x1) / 2.0;
            x > *left as f32 && x < *right as f32
        })
        .collect::<Vec<_>>();
    let mut out = ys.to_vec();
    for pair in ys.windows(2) {
        let (bottom, top) = (pair[0].min(pair[1]) as f32, pair[0].max(pair[1]) as f32);
        let lines = text_lines_in_band(&texts, xs, bottom, top);
        if lines.len() < MIN_LINES {
            continue;
        }
        let aligned = lines.iter().filter(|line| line.2.len() >= 2).count();
        let numeric = lines.iter().filter(|line| line.3).count();
        if (aligned as f32) < MIN_ALIGNED_RATIO * lines.len() as f32
            || (numeric as f32) < MIN_NUMERIC_RATIO * lines.len() as f32
        {
            continue;
        }
        for window in lines.windows(2) {
            // 上の行の下端と下の行の上端の中央で分ける（重なる行は分けない）。
            let (upper_bottom, lower_top) = (window[0].0, window[1].1);
            if upper_bottom > lower_top {
                out.push(((upper_bottom + lower_top) / 2.0).round() as i32);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 金額の字片は数値、科目は数値でない。
    #[test]
    fn recognizes_amounts() {
        assert!(is_numeric_text("479,063,388,032"));
        assert!(is_numeric_text("△ 803,217,166"));
        assert!(!is_numeric_text("現金預け金"));
        assert!(!is_numeric_text("第35条"));
    }
}
