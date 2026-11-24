//! 文字を削除せず、全source pageで確認できる装飾付き連番だけをfooterの候補として返す。
use super::*;
use docx_rs::BuildXML;

/// 装飾と書式を照合するための番号。比較用の空白除外を出力本文へ適用しない。
#[derive(Debug, PartialEq, Eq)]
struct Label {
    /// 原文のASCII数字から、overflowせず読めた値。
    number: u32,
    /// 装飾文字と、数値部分を一役へまとめた実Word書式。
    style: Vec<(char, Vec<u8>)>,
}

/// 連続する全ページで位置・装飾・書式・連番が確認できる場合だけ、各原本行をそのまま選ぶ。
pub(super) fn select(
    parsed: &ParsedDocument,
    pages: &[StructuredTextPage],
) -> Vec<(usize, StructuredTextLine)> {
    if pages.len() < 2
        || pages
            .windows(2)
            .any(|p| p[0].page.checked_add(1) != Some(p[1].page))
    {
        return Vec::new();
    }
    let mut selected = Vec::new();
    let mut first: Option<Label> = None;
    for (index, page) in pages.iter().enumerate() {
        let Some(geometry) = parsed.pages.iter().find(|p| p.page_number == page.page) else {
            return Vec::new();
        };
        if geometry.rotation.rem_euclid(360) != 0 || parsed_page_size_twips(geometry).is_none() {
            return Vec::new();
        }
        let (width, height) = parsed_page_display_size_points(geometry);
        let mut candidates = page
            .lines
            .iter()
            .filter(|line| eligible(line, width, height))
            .filter_map(|line| label(line).map(|label| (line, label)));
        let Some((line, matched)) = candidates.next() else {
            return Vec::new();
        };
        if candidates.next().is_some() {
            return Vec::new();
        }
        if let Some(first) = &first {
            let expected = u32::try_from(index)
                .ok()
                .and_then(|i| first.number.checked_add(i));
            if expected != Some(matched.number) || first.style != matched.style {
                return Vec::new();
            }
        } else {
            first = Some(matched);
        }
        selected.push((page.page, line.clone()));
    }
    selected
}

/// 下端6%以内の中央行だけを対象にし、本文位置・回転・不正な尺度をfooterへ推測しない。
fn eligible(line: &StructuredTextLine, width: f32, height: f32) -> bool {
    let em = structured_line_max_font_size(line);
    [width, height, line.top_y, line.left_x, line.right_x]
        .iter()
        .all(|v| v.is_finite())
        && width > 0.
        && height > 0.
        && em > 0.
        && line
            .runs
            .iter()
            .all(|r| r.style.font_size.is_finite() && r.style.font_size > 0.)
        && line.alignment == ParagraphAlignment::Center
        && line.left_x >= 0.
        && line.right_x <= width
        && line.left_x < line.right_x
        && line.top_y > em
        && line.top_y <= height * 0.06
        && ((line.left_x + line.right_x) / 2. - width / 2.).abs() <= width * 0.08
}

/// em dash・horizontal barで挟まれたASCII番号だけを読み、符号・別の文字・overflowを拒否する。
fn label(line: &StructuredTextLine) -> Option<Label> {
    let mut chars = line.text.chars().filter(|c| !c.is_whitespace());
    let first = chars.next()?;
    if !matches!(first, '—' | '―') {
        return None;
    }
    let (mut number, mut saw_digit) = (0u32, false);
    loop {
        let c = chars.next()?;
        if c == first {
            if !saw_digit || chars.next().is_some() {
                return None;
            }
            break;
        }
        if !c.is_ascii_digit() {
            return None;
        }
        number = number.checked_mul(10)?.checked_add(c.to_digit(10)?)?;
        saw_digit = true;
    }
    let mut style: Vec<(char, Vec<u8>)> = Vec::new();
    for run in &line.runs {
        let properties = docx_run_from_structured_run(run).run_property.build();
        for c in run.text.chars().filter(|c| !c.is_whitespace()) {
            let role = if c.is_ascii_digit() { '\0' } else { c };
            if !style
                .last()
                .is_some_and(|p| p.0 == role && p.1 == properties)
            {
                style.push((role, properties.clone()));
            }
        }
    }
    Some(Label { number, style })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 所有する行だけを作り、認識後も元の文字と書式を変更しないことを確認する。
    fn line(text: &str) -> StructuredTextLine {
        StructuredTextLine {
            top_y: 25.,
            left_x: 280.,
            right_x: 330.,
            alignment: ParagraphAlignment::Center,
            runs: vec![StructuredTextRun {
                text: text.into(),
                style: StructuredTextStyle::default(),
            }],
            split_from_row: false,
            text: text.into(),
        }
    }

    /// 比較時の空白や桁数は、元の表記とrunの構造を変更しない。
    #[test]
    fn label_key_preserves_raw_whitespace_and_allows_digit_count_changes() {
        let a = line("―　9　―");
        let b = line("― 10 ―");
        assert_eq!(label(&a).unwrap().style, label(&b).unwrap().style);
        assert_eq!(label(&a).unwrap().number, 9);
        assert_eq!(label(&b).unwrap().number, 10);
        assert_eq!(a.text, "―　9　―");
        assert_eq!(a.runs[0].text, a.text);
    }

    /// 一端だけの線、数式のminus、語尾付きの値、巨大な数値はページ番号とみなさない。
    #[test]
    fn ambiguous_or_overflowing_labels_are_rejected() {
        for text in [
            "―15",
            "15―",
            "−15−",
            "―15th―",
            "―+15―",
            "―99999999999999999999999999―",
            "―15—",
        ] {
            assert!(label(&line(text)).is_none(), "{text}");
        }
    }

    /// 本文領域・端寄せ・非有限の座標やsizeを中央footerとみなさない。
    #[test]
    fn only_finite_centered_bottom_geometry_is_eligible() {
        let original = line("―15―");
        assert!(eligible(&original, 612., 792.));
        let mut invalid = original.clone();
        invalid.top_y = 100.;
        assert!(!eligible(&invalid, 612., 792.));
        let mut invalid = original.clone();
        invalid.left_x = 50.;
        invalid.right_x = 100.;
        assert!(!eligible(&invalid, 612., 792.));
        let mut invalid = original.clone();
        invalid.runs[0].style.font_size = f32::NAN;
        assert!(!eligible(&invalid, 612., 792.));
        assert!(!eligible(&original, f32::INFINITY, 792.));
    }
}
