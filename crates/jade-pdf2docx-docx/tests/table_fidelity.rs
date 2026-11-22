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
