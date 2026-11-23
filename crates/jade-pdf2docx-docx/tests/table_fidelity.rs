//! 罫線表の DOCX 変換で、セル本文・列・行高が PDF の見た目を保つことを確認する。
use std::io::Write;

use flate2::{Compression, write::ZlibEncoder};
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;

mod support;
use support::docx_assert::read_document_xml;
use support::paths::temp_file_path;

/// 非圧縮 content stream の 1 ページ PDF（300x200pt、等幅の Courier）を作る。
///
/// `/Im1` として 2x2 の白い RGB 画像、`/Im2` として黒い画素を含む画像も参照できる。
fn pdf_with_content(content: &str) -> Vec<u8> {
    let image = |pixels: &[u8]| {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(pixels).unwrap();
        let data = encoder.finish().unwrap();
        let mut object = format!(
            "<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
            data.len()
        )
        .into_bytes();
        object.extend_from_slice(&data);
        object.extend_from_slice(b"\nendstream");
        object
    };
    let mut inked = vec![255u8; 12];
    inked[0] = 0;
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 6 0 R /Im2 7 0 R >> >> >>".to_vec(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()).into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_vec(),
        image(&[255; 12]),
        image(&inked),
    ];
    let mut source = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        source.extend_from_slice(object);
        source.extend_from_slice(b"\nendobj\n");
    }
    let xref = source.len();
    source.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        source.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    source.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    source
}

/// PDF を変換して document.xml を返す。
fn convert(name: &str, content: &str) -> String {
    let bytes =
        convert_minimum_pdf_bytes_to_docx_bytes(format!("{name}.pdf"), &pdf_with_content(content))
            .expect("conversion must succeed");
    let path = temp_file_path(name, "docx");
    std::fs::write(&path, bytes).unwrap();
    let xml = read_document_xml(&path);
    let _ = std::fs::remove_file(path);
    xml
}

/// 1 文字ずつ配置した文字列の content stream 断片。
fn per_char(text: &str, x: f32, y: f32, advance: f32) -> String {
    text.chars()
        .enumerate()
        .filter(|(_, ch)| *ch != ' ')
        .map(|(index, ch)| {
            format!(
                "BT /F1 10 Tf 1 0 0 1 {:.1} {y} Tm ({ch}) Tj ET",
                x + advance * index as f32
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 2 列 2 行の罫線（上段 20pt、下段 40pt）。
const TWO_BY_TWO_RULES: &str = "0.5 w 20 160 m 220 160 l S 20 140 m 220 140 l S 20 100 m 220 100 l S \
     20 100 m 20 160 l S 120 100 m 120 160 l S 220 100 m 220 160 l S";

/// 文字単位で配置したセル本文を、空白で分断せず語として戻す。
#[test]
fn per_character_cell_text_is_joined_into_words() {
    let content = format!(
        "{TWO_BY_TWO_RULES} {} {} {} {}",
        per_char("Item one", 25.0, 146.0, 6.0),
        per_char("Qty", 125.0, 146.0, 6.0),
        per_char("Box", 25.0, 120.0, 6.0),
        per_char("12", 125.0, 120.0, 6.0),
    );
    let xml = convert("table_fidelity_per_char", &content);
    assert!(
        xml.contains(">Item one<"),
        "cell text must be one word sequence: {xml}"
    );
    assert_eq!(
        xml.matches("<w:gridCol ").count(),
        2,
        "ruled columns must not be split at glyph gaps: {xml}"
    );
}

/// 行ごとの高さを保ち、複数行のセルを含む行は内容に合わせて伸ばせるようにする。
#[test]
fn row_heights_follow_each_pdf_row() {
    let content = format!(
        "{TWO_BY_TWO_RULES} BT /F1 10 Tf 1 0 0 1 25 146 Tm (Head) Tj ET \
         BT /F1 10 Tf 1 0 0 1 25 126 Tm (alpha) Tj ET BT /F1 10 Tf 1 0 0 1 25 112 Tm (beta) Tj ET \
         BT /F1 10 Tf 1 0 0 1 125 126 Tm (x) Tj ET"
    );
    let xml = convert("table_fidelity_row_heights", &content);
    assert!(
        xml.contains(r#"<w:trHeight w:val="410" w:hRule="exact""#),
        "20pt single-line row (plus thin-rule bleed) must stay exact: {xml}"
    );
    assert!(
        xml.contains(r#"<w:trHeight w:val="810" w:hRule="atLeast""#),
        "40pt multi-line row (plus thin-rule bleed) must be at least its PDF height: {xml}"
    );
    let alpha = xml.find(">alpha<").expect("alpha");
    let beta = xml.find(">beta<").expect("beta");
    assert!(
        xml[alpha..beta].contains("</w:p>"),
        "short cell lines must stay separate paragraphs: {xml}"
    );
}

/// 本文のあるページ全面に敷かれた白一色の画像は、本文を押し下げないよう出力しない。
#[test]
fn blank_page_background_image_is_dropped() {
    let xml = convert(
        "table_fidelity_blank_background",
        "q 300 0 0 200 0 0 cm /Im1 Do Q BT /F1 10 Tf 1 0 0 1 20 150 Tm (Body text) Tj ET",
    );
    assert!(xml.contains(">Body text<"), "body must remain: {xml}");
    assert!(
        !xml.contains("<w:drawing>"),
        "blank full-page background must not become a picture: {xml}"
    );
}

/// 描画のある全面画像は残す。
#[test]
fn inked_page_image_is_kept() {
    let xml = convert(
        "table_fidelity_inked_background",
        "q 300 0 0 200 0 0 cm /Im2 Do Q BT /F1 10 Tf 1 0 0 1 20 150 Tm (Body text) Tj ET",
    );
    assert!(xml.contains("<w:drawing>"), "inked image must stay: {xml}");
}

/// 上下の外枠を描かない表も、揃った縦罫線の端を外枠として表にする。
#[test]
fn table_without_top_and_bottom_rules_is_detected() {
    let content = "0.5 w 20 60 m 20 160 l S 120 60 m 120 160 l S 220 60 m 220 160 l S 20 110 m 220 110 l S \
        BT /F1 10 Tf 1 0 0 1 25 140 Tm (A1) Tj ET BT /F1 10 Tf 1 0 0 1 125 140 Tm (B1) Tj ET \
        BT /F1 10 Tf 1 0 0 1 25 80 Tm (A2) Tj ET BT /F1 10 Tf 1 0 0 1 125 80 Tm (B2) Tj ET";
    let xml = convert("table_fidelity_open_edges", content);
    assert_eq!(
        xml.matches("<w:tbl>").count(),
        1,
        "open table must be one table: {xml}"
    );
    assert_eq!(
        xml.matches("<w:tr>").count(),
        2,
        "rows split by the inner rule: {xml}"
    );
    let first_cell = &xml[xml.find("<w:tc>").expect("cell")..];
    let borders = &first_cell[..first_cell.find("</w:tcBorders>").expect("borders")];
    assert!(
        !borders.contains("<w:top w:val=\"single\""),
        "missing top rule must not be drawn: {borders}"
    );
}

/// 横に並んだ 2 つの表は、縦に積まず枠なしの外側表の中へ並べる。
#[test]
fn side_by_side_tables_stay_side_by_side() {
    let grid = |x0: i32| {
        let (x1, x2) = (x0 + 50, x0 + 100);
        format!(
            "{x0} 100 m {x2} 100 l S {x0} 130 m {x2} 130 l S {x0} 160 m {x2} 160 l S \
             {x0} 100 m {x0} 160 l S {x1} 100 m {x1} 160 l S {x2} 100 m {x2} 160 l S \
             BT /F1 10 Tf 1 0 0 1 {} 140 Tm (L{x0}) Tj ET BT /F1 10 Tf 1 0 0 1 {} 110 Tm (R{x0}) Tj ET",
            x0 + 5,
            x1 + 5
        )
    };
    let content = format!("0.5 w {} {}", grid(20), grid(170));
    let xml = convert("table_fidelity_side_by_side", &content);
    assert_eq!(
        xml.matches("<w:tbl>").count(),
        3,
        "two tables nested in one layout table: {xml}"
    );
    let left = xml.find(">L20<").expect("left table text");
    let right = xml.find(">L170<").expect("right table text");
    assert!(left < right, "left table must come first: {xml}");
}

/// 縦罫線がなく横罫線だけの表も、文字の並びから列を分けて表にする。
#[test]
fn table_with_only_horizontal_rules_is_split_into_columns() {
    let rules = (0..4)
        .map(|row| {
            let y = 160 - row * 20;
            format!("20 {y} m 220 {y} l S")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let cell =
        |x: i32, y: i32, text: &str| format!("BT /F1 10 Tf 1 0 0 1 {x} {y} Tm ({text}) Tj ET");
    let content = format!(
        "0.5 w {rules} {} {} {} {} {} {} {} {} {}",
        cell(25, 146, "Year"),
        cell(95, 146, "Sales"),
        cell(165, 146, "Profit"),
        cell(25, 126, "2021"),
        cell(95, 126, "100"),
        cell(165, 126, "7"),
        cell(25, 106, "2022"),
        cell(95, 106, "200"),
        cell(165, 106, "14"),
    );
    let xml = convert("table_fidelity_horizontal_only", &content);
    assert_eq!(xml.matches("<w:tbl>").count(), 1, "{xml}");
    assert_eq!(
        xml.matches("<w:gridCol ").count(),
        3,
        "three text columns: {xml}"
    );
    assert!(xml.contains(">Sales<") && xml.contains(">Profit<"), "{xml}");
}

/// セル幅いっぱいの文字がある行は、代替フォントで折り返しても隠れないよう atLeast にする。
#[test]
fn nearly_full_cell_row_is_not_clipped() {
    let content = "0.5 w 20 160 m 120 160 l S 20 140 m 120 140 l S 20 120 m 120 120 l S \
        20 120 m 20 160 l S 70 120 m 70 160 l S 120 120 m 120 160 l S \
        BT /F1 10 Tf 1 0 0 1 21 146 Tm (ABCDEFGH) Tj ET BT /F1 10 Tf 1 0 0 1 75 146 Tm (x) Tj ET \
        BT /F1 10 Tf 1 0 0 1 25 126 Tm (ab) Tj ET BT /F1 10 Tf 1 0 0 1 75 126 Tm (y) Tj ET";
    let xml = convert("table_fidelity_full_cell", content);
    let first_row = &xml[xml.find("<w:tr>").expect("row")..];
    let first_row = &first_row[..first_row.find("</w:tr>").expect("row end")];
    assert!(
        first_row.contains(r#"w:hRule="atLeast""#),
        "full-width cell text row must be able to grow: {first_row}"
    );
    let second_row = &xml[xml.rfind("<w:tr>").expect("row")..];
    assert!(
        second_row.contains(r#"w:hRule="exact""#),
        "short single-line row stays exact: {second_row}"
    );
}

/// 子表を含むセルでは、子表の文字を親セルに重ねて出さず、子表の上下の本文を位置どおりに並べる。
#[test]
fn nested_table_text_is_not_duplicated_and_keeps_order() {
    let content = "0 180 m 280 180 l S 0 100 m 280 100 l S 0 0 m 280 0 l S \
        0 0 m 0 180 l S 140 0 m 140 180 l S 280 0 m 280 180 l S \
        BT /F1 10 Tf 1 0 0 1 52 150 Tm (Before) Tj ET BT /F1 10 Tf 1 0 0 1 192 150 Tm (After) Tj ET \
        BT /F1 10 Tf 1 0 0 1 12 84 Tm (Left note) Tj ET BT /F1 10 Tf 1 0 0 1 152 84 Tm (Right note) Tj ET \
        10 74 m 130 74 l S 10 52 m 130 52 l S 10 30 m 130 30 l S 10 30 m 10 74 l S 70 30 m 70 74 l S 130 30 m 130 74 l S \
        BT /F1 10 Tf 1 0 0 1 22 60 Tm (L1) Tj ET BT /F1 10 Tf 1 0 0 1 82 60 Tm (L2) Tj ET \
        BT /F1 10 Tf 1 0 0 1 12 12 Tm (Below) Tj ET \
        150 74 m 270 74 l S 150 52 m 270 52 l S 150 30 m 270 30 l S 150 30 m 150 74 l S 210 30 m 210 74 l S 270 30 m 270 74 l S \
        BT /F1 10 Tf 1 0 0 1 162 60 Tm (R1) Tj ET BT /F1 10 Tf 1 0 0 1 222 60 Tm (R2) Tj ET";
    let xml = convert("table_fidelity_nested_order", content);
    assert_eq!(xml.matches("<w:tbl>").count(), 3, "{xml}");
    assert_eq!(
        xml.matches(">L1<").count(),
        1,
        "nested text must appear once: {xml}"
    );
    let note = xml.find(">Left note<").expect("note");
    let inner = xml.find(">L1<").expect("inner");
    let below = xml.find(">Below<").expect("below");
    assert!(
        note < inner && inner < below,
        "text above, nested table, text below: {xml}"
    );
}

/// 見出し行の塗りつぶしは、セルの背景色として残す。
#[test]
fn header_row_fill_becomes_cell_shading() {
    let content = format!(
        "0.85 g 20 140 200 20 re f 0 g {TWO_BY_TWO_RULES} BT /F1 10 Tf 1 0 0 1 25 146 Tm (Head) Tj ET \
         BT /F1 10 Tf 1 0 0 1 125 146 Tm (Qty) Tj ET BT /F1 10 Tf 1 0 0 1 25 120 Tm (Box) Tj ET \
         BT /F1 10 Tf 1 0 0 1 125 120 Tm (12) Tj ET"
    );
    let xml = convert("table_fidelity_header_fill", &content);
    assert_eq!(
        xml.matches(r#"w:fill="D9D9D9""#).count(),
        2,
        "both header cells keep the gray fill: {xml}"
    );
}

/// 罫線のない表も、全行に共通する列の空きから表にし、数値は右揃えにする。
#[test]
fn ruleless_table_is_detected_from_aligned_columns() {
    let row = |y: i32, label: &str, value: &str| {
        format!(
            "BT /F1 10 Tf 1 0 0 1 20 {y} Tm ({label}) Tj ET BT /F1 10 Tf 1 0 0 1 160 {y} Tm ({value}) Tj ET"
        )
    };
    let content = [
        row(160, "Land", "128,138"),
        row(146, "Buildings", "165,012"),
        row(132, "Machinery", "1,706"),
        row(118, "Books", "21,527"),
    ]
    .join(" ");
    let xml = convert("table_fidelity_ruleless", &content);
    assert_eq!(xml.matches("<w:tbl>").count(), 1, "{xml}");
    assert_eq!(xml.matches("<w:tr>").count(), 4, "{xml}");
    assert!(
        xml.contains(r#"<w:jc w:val="right""#),
        "numbers right-aligned: {xml}"
    );
}

/// 長い文の段落は、列が揃って見えても表にしない。
#[test]
fn prose_lines_are_not_ruleless_tables() {
    let content = (0..5)
        .map(|index| {
            let y = 160 - index * 14;
            format!(
                "BT /F1 10 Tf 1 0 0 1 20 {y} Tm (This is a long sentence of prose text) Tj ET \
                 BT /F1 10 Tf 1 0 0 1 20 {} Tm () Tj ET",
                y
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let xml = convert("table_fidelity_prose", &content);
    assert!(!xml.contains("<w:tbl>"), "{xml}");
}

/// 横に並んだ表の外接矩形にかかる本文も、表に吸い込まずに本文として残す。
#[test]
fn text_beside_side_by_side_tables_is_kept() {
    let grid = |x0: i32, y0: i32| {
        let (x1, x2, y1, y2) = (x0 + 50, x0 + 100, y0 + 30, y0 + 60);
        format!(
            "{x0} {y0} m {x2} {y0} l S {x0} {y1} m {x2} {y1} l S {x0} {y2} m {x2} {y2} l S \
             {x0} {y0} m {x0} {y2} l S {x1} {y0} m {x1} {y2} l S {x2} {y0} m {x2} {y2} l S \
             BT /F1 10 Tf 1 0 0 1 {} {} Tm (A{x0}) Tj ET BT /F1 10 Tf 1 0 0 1 {} {} Tm (B{x0}) Tj ET",
            x0 + 5,
            y0 + 40,
            x1 + 5,
            y0 + 10
        )
    };
    let content = format!(
        "0.5 w {} {} BT /F1 10 Tf 1 0 0 1 25 175 Tm (Notice text) Tj ET",
        grid(20, 60),
        grid(170, 120)
    );
    let xml = convert("table_fidelity_beside_container", &content);
    assert!(
        xml.contains(">Notice text<"),
        "text above the left table must remain: {xml}"
    );
}

/// 先頭行の段落は次の行と同じページに置き、2 行目以降には付けない。
#[test]
fn first_table_row_keeps_with_the_next_row() {
    let content = format!(
        "{TWO_BY_TWO_RULES} BT /F1 10 Tf 1 0 0 1 25 146 Tm (Head) Tj ET \
         BT /F1 10 Tf 1 0 0 1 25 126 Tm (alpha) Tj ET BT /F1 10 Tf 1 0 0 1 125 126 Tm (x) Tj ET"
    );
    let xml = convert("table_fidelity_keep_next", &content);
    let head = xml.find(">Head<").expect("Head");
    let alpha = xml.find(">alpha<").expect("alpha");
    let paragraph_start = xml[..head]
        .rfind("<w:p>")
        .or_else(|| xml[..head].rfind("<w:p "))
        .expect("paragraph");
    assert!(
        xml[paragraph_start..head].contains("<w:keepNext"),
        "the first row must keep with the next row: {xml}"
    );
    let alpha_start = xml[..alpha].rfind("<w:p").expect("paragraph");
    assert!(
        !xml[alpha_start..alpha].contains("<w:keepNext"),
        "later rows must not keep with the next row: {xml}"
    );
}

/// 金額の列の左端にそろえた負の数の印（ここでは `-`）は、落とさず、印だけの列にもせず金額のセルに入れる。
#[test]
fn sign_column_joins_the_amount_column() {
    let row = |y: i32, label: &str, sign: &str, value: &str| {
        let sign = if sign.is_empty() {
            String::new()
        } else {
            format!("BT /F1 10 Tf 1 0 0 1 120 {y} Tm ({sign}) Tj ET ")
        };
        format!(
            "BT /F1 10 Tf 1 0 0 1 20 {y} Tm ({label}) Tj ET {sign}BT /F1 10 Tf 1 0 0 1 180 {y} Tm ({value}) Tj ET"
        )
    };
    let content = [
        row(160, "Deposits", "-", "2,900,000"),
        row(146, "Refunds", "", "2,900,000"),
        row(132, "Securities", "-", "2,000,000"),
        row(118, "Interest", "", "21,527"),
    ]
    .join(" ");
    let xml = convert("table_fidelity_sign_column", &content);
    assert_eq!(xml.matches("<w:tbl>").count(), 1, "{xml}");
    let first_row = &xml[xml.find("<w:tr>").unwrap()..xml.find("</w:tr>").unwrap()];
    assert_eq!(
        first_row.matches("<w:tc>").count(),
        2,
        "the sign must not get a column of its own: {xml}"
    );
    assert_eq!(
        xml.matches(">-").count() + xml.matches("> -").count(),
        2,
        "every sign must be kept: {xml}"
    );
}

/// 上半分の表と上下対称の位置にある本文は、表の中とみなして落とさない。
#[test]
fn text_mirrored_below_an_upper_table_is_kept() {
    let rules = "0.5 w 20 180 m 220 180 l S 20 150 m 220 150 l S 20 120 m 220 120 l S \
         20 120 m 20 180 l S 120 120 m 120 180 l S 220 120 m 220 180 l S";
    let content = format!(
        "{rules} BT /F1 10 Tf 1 0 0 1 25 166 Tm (Head) Tj ET BT /F1 10 Tf 1 0 0 1 125 166 Tm (Value) Tj ET \
         BT /F1 10 Tf 1 0 0 1 25 136 Tm (alpha) Tj ET BT /F1 10 Tf 1 0 0 1 125 136 Tm (beta) Tj ET \
         BT /F1 10 Tf 1 0 0 1 30 47 Tm (Mirrored body text) Tj ET"
    );
    let xml = convert("table_fidelity_mirrored_text", &content);
    assert!(
        xml.contains("Mirrored body text"),
        "body text must not be dropped as part of the table: {xml}"
    );
}

/// 上半分だけを縦罫線で 2 段に分けたページでは、同じ高さの左右の段の字を同じ行・表にしない。
#[test]
fn partial_height_column_rule_separates_columns() {
    let mut parts = vec!["0.5 w 150 100 m 150 195 l S".to_string()];
    for (index, y) in [180, 168, 156, 144, 132, 120].into_iter().enumerate() {
        parts.push(format!(
            "BT /F1 10 Tf 1 0 0 1 20 {y} Tm (L{index}) Tj ET BT /F1 10 Tf 1 0 0 1 100 {y} Tm ({index}11) Tj ET \
             BT /F1 10 Tf 1 0 0 1 160 {y} Tm (R{index}) Tj ET BT /F1 10 Tf 1 0 0 1 250 {y} Tm ({index}22) Tj ET"
        ));
    }
    parts.push("BT /F1 10 Tf 1 0 0 1 20 40 Tm (Footer line across the page) Tj ET".to_string());
    let xml = convert("table_fidelity_partial_column_rule", &parts.join(" "));
    for index in 0..6 {
        let left = xml.find(&format!(">L{index}")).expect("left text");
        // 同じ段落にも、同じ表の行にも入れない。
        let paragraph_end = left + xml[left..].find("</w:p>").expect("paragraph end");
        let row_end = xml[left..].find("</w:tr>").map(|end| left + end);
        let row_start = xml[..left].rfind("<w:tr>");
        let end = match (row_start, row_end) {
            (Some(start), Some(end)) if !xml[start..left].contains("</w:tr>") => end,
            _ => paragraph_end,
        };
        let start = match row_start {
            Some(start) if end != paragraph_end => start,
            _ => left,
        };
        assert!(
            !xml[start..end].contains(&format!("R{index}")),
            "left and right columns must stay apart: {xml}"
        );
    }
}

/// 破線で描いた罫線は、DOCX でも破線のセル罫線にする（実線の辺は実線のまま）。
#[test]
fn dashed_rules_become_dashed_cell_borders() {
    let content = "0.5 w 20 160 m 220 160 l S 20 100 m 220 100 l S \
         20 100 m 20 160 l S 120 100 m 120 160 l S 220 100 m 220 160 l S \
         [2 2] 0 d 20 130 m 220 130 l S [] 0 d \
         BT /F1 10 Tf 1 0 0 1 25 146 Tm (Head) Tj ET BT /F1 10 Tf 1 0 0 1 125 146 Tm (Value) Tj ET \
         BT /F1 10 Tf 1 0 0 1 25 116 Tm (alpha) Tj ET BT /F1 10 Tf 1 0 0 1 125 116 Tm (beta) Tj ET";
    let xml = convert("table_fidelity_dashed_rules", content);
    assert!(
        xml.contains(r#"<w:bottom w:val="dashed""#),
        "the dashed middle rule must stay dashed: {xml}"
    );
    assert!(
        xml.contains(r#"<w:top w:val="single""#),
        "solid rules must stay solid: {xml}"
    );
}
