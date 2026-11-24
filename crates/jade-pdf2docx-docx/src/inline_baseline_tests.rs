//! 確認済みbaselineの小さい上付き文字を、別の本文行へ送らない回帰。
use super::*;

/// 実PDFを保存せず、上付きの数字と独立した次の本文行を持つ解析結果を作る。
fn formula_source() -> ParsedDocument {
    let mut parsed = source(2, &[]);
    parsed.page_count = 1;
    parsed.pages.truncate(1);
    parsed.text_elements = [
        ("A", 50., 56., 700., 10.),
        ("2", 56.2, 60.2, 704., 7.),
        ("=B", 62., 76., 700., 10.),
        ("Next line", 50., 100., 680., 10.),
    ]
    .into_iter()
    .map(|(text, left, right, baseline, size)| ParsedTextElement {
        baseline_y: Some(baseline),
        page: 0,
        bounds: ParsedBounds {
            x0: left,
            x1: right,
            y0: baseline - size * 0.2,
            y1: baseline + size * 0.8,
        },
        font_size: size,
        font_name: Some("Helvetica".into()),
        fill_color: None,
        text: text.into(),
    })
    .collect();
    parsed
}

/// 本文の行を増やさず、元のfont sizeを保ったまま上付き位置を出力する。
#[test]
fn raised_source_glyph_stays_with_its_base_line() {
    let parsed = formula_source();
    let pages = text_lines_by_page_from_pdf_text_elements(&parsed, &[]);
    assert_eq!(pages.len(), 1);
    assert_eq!(
        pages[0].lines.len(),
        2,
        "raised source glyph must stay with its base line"
    );
    assert_eq!(pages[0].lines[0].text, "A2=B");
    assert_eq!(pages[0].lines[1].text, "Next line");
    let (xml, _) = package_source(parsed);
    assert!(
        xml.contains("<w:position w:val=\"8\""),
        "source4pt baseline offset requires8halfpoints without font shrink"
    );
}
