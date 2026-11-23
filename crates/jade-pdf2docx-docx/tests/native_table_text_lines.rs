//! 表セルの原本の表示行と行間・run styleを、単一の再flow段落へ潰さない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// 正式xref付きの一行二列の表へ、異なる高さの二つの本文行を描く。
fn source(gap: u32, second_size: u32) -> Vec<u8> {
    source_with_padding(gap, second_size, "", "")
}

/// 原本の行頭・行末に明示的な空白を含め、字面と描画原点を区別する。
fn source_with_padding(gap: u32, second_size: u32, prefix: &str, suffix: &str) -> Vec<u8> {
    let content = format!(
        "1 w 72 600 m 272 600 l S 72 700 m 272 700 l S 72 600 m 72 700 l S 172 600 m 172 700 l S 272 600 m 272 700 l S BT /F1 10 Tf 82 680 Td ({prefix}Alpha{suffix}) Tj ET BT /F1 {second_size} Tf 92 {} Td (Beta) Tj ET BT /F1 10 Tf 182 680 Td (Control) Tj ET",
        680 - gap,
    );
    owned_pdf_content(&content)
}

/// 描画命令列から、正式xrefと一つのHelvetica resourceを持つ所有PDFを作る。
fn owned_pdf_content(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

/// 製品byte APIが出力した、最初の表セルの段落XMLを取り出す。
fn first_cell(gap: u32, second_size: u32) -> String {
    first_cell_from_source(&source(gap, second_size))
}

/// 指定した所有原本の最初の表セルXMLを、製品byte API経由で読む。
fn first_cell_from_source(source: &[u8]) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-cell-lines.pdf", source).unwrap();
    let xml = document_xml(bytes);
    let start = xml.find("<w:tc>").unwrap();
    let end = start + xml[start..].find("</w:tc>").unwrap();
    xml[start..end].to_string()
}

/// 製品DOCXの本文XMLを、閉じたarchiveから読む。
fn document_xml(bytes: Vec<u8>) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 段落ごとの本文・行box・前余白・font sizeを、属性順に依存せず読む。
fn paragraphs(xml: &str) -> Vec<(String, u32, u32, Vec<u32>)> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut current = (String::new(), 0, 0, Vec::new());
    let mut in_text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(tag) if tag.name().as_ref() == b"w:p" => {
                current = (String::new(), 0, 0, Vec::new())
            }
            Event::Start(tag) if tag.name().as_ref() == b"w:t" => in_text = true,
            Event::End(tag) if tag.name().as_ref() == b"w:t" => in_text = false,
            Event::End(tag) if tag.name().as_ref() == b"w:p" => out.push(current.clone()),
            Event::Text(text) if in_text => current.0.push_str(&text.decode().unwrap()),
            Event::Empty(tag) if tag.name().as_ref() == b"w:spacing" => {
                for a in tag.attributes() {
                    let a = a.unwrap();
                    if a.key.as_ref() == b"w:line" {
                        current.1 = std::str::from_utf8(&a.value).unwrap().parse().unwrap();
                    }
                    if a.key.as_ref() == b"w:before" {
                        current.2 = std::str::from_utf8(&a.value).unwrap().parse().unwrap();
                    }
                }
            }
            Event::Empty(tag) if tag.name().as_ref() == b"w:sz" => {
                for a in tag.attributes() {
                    let a = a.unwrap();
                    if a.key.as_ref() == b"w:val" {
                        current
                            .3
                            .push(std::str::from_utf8(&a.value).unwrap().parse().unwrap());
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

/// 原本二行を二段落に保ち、セル全体の共通fontへ均さない。
#[test]
fn authored_cell_lines_and_font_sizes_remain_distinct() {
    let p = paragraphs(&first_cell(24, 14));
    assert_eq!(
        p.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(),
        ["Alpha", "Beta"]
    );
    assert_eq!(p[0].3, [20]);
    assert_eq!(p[1].3, [28]);
}

/// 行頭空白をtrimしても描画原点だけが残り、文字を左へ移動させてはならない。
#[test]
fn authored_cell_edge_spaces_are_not_trimmed() {
    let p = paragraphs(&first_cell_from_source(&source_with_padding(
        24, 10, "    ", "  ",
    )));
    assert_eq!(p[0].0, "    Alpha  ");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/native_table_text_lines.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 独立に指定したbaseline間隔は、同一fontの段落行boxと前余白の和に残る。
    #[test]
    fn source_cell_line_pitch_survives(gap in 16u32..40) {
        let p = paragraphs(&first_cell(gap, 10));
        prop_assert_eq!(p.len(), 2);
        prop_assert_eq!(p[0].1 + p[1].2, gap * 20);
    }

    /// 原本の先頭・末尾の空白数を、run境界の正規化で減らさない。
    #[test]
    fn source_cell_edge_space_counts_survive(leading in 0usize..9, trailing in 0usize..5) {
        let prefix = " ".repeat(leading);
        let suffix = " ".repeat(trailing);
        let p = paragraphs(&first_cell_from_source(&source_with_padding(24, 10, &prefix, &suffix)));
        prop_assert_eq!(&p[0].0, &format!("{prefix}Alpha{suffix}"));
    }
}

/// 原本の二つの行高を平均せず、上から下の対応を維持する所有PDFを作る。
fn unequal_row_source(top: u32, bottom: u32) -> Vec<u8> {
    let high = 600 + bottom + top;
    let middle = 600 + bottom;
    let mut content = format!(
        "1 w 72 600 m 272 600 l S 72 {middle} m 272 {middle} l S 72 {high} m 272 {high} l S 72 600 m 72 {high} l S 172 600 m 172 {high} l S 272 600 m 272 {high} l S"
    );
    for (label, y) in [("Top", high - 12), ("Bottom", middle - 12)] {
        content.push_str(&format!(" BT /F1 10 Tf 82 {y} Td ({label}) Tj ET BT /F1 10 Tf 82 {} Td (Second) Tj ET BT /F1 10 Tf 182 {y} Td (Control) Tj ET", y - 16));
    }
    owned_pdf_content(&content)
}

/// 製品byte APIの実際の行高属性を、XML属性順に依存せず読む。
fn row_heights(source: &[u8]) -> Vec<u32> {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("unequal-cell-rows.pdf", source).unwrap();
    let xml = document_xml(bytes);
    let mut reader = Reader::from_str(&xml);
    let mut heights = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Empty(tag) if tag.name().as_ref() == b"w:trHeight" => {
                for attr in tag.attributes() {
                    let attr = attr.unwrap();
                    if attr.key.as_ref() == b"w:val" {
                        heights.push(std::str::from_utf8(&attr.value).unwrap().parse().unwrap());
                    }
                }
            }
            Event::Eof => return heights,
            _ => {}
        }
    }
}

/// 行高が40ptと100ptの原本を、各70ptへ潰すと複数行の後半が切れる。
#[test]
fn unequal_source_row_heights_are_not_averaged() {
    assert_eq!(row_heights(&unequal_row_source(40, 100)), [800, 2000]);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/native_table_geometry.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 独立に変えた各行高は、総高や本文の値にかかわらず対応する行へ残る。
    #[test]
    fn each_source_row_height_survives(top in 32u32..96, bottom in 100u32..180) {
        prop_assert_eq!(row_heights(&unequal_row_source(top, bottom)), [top * 20, bottom * 20]);
    }
}

/// 原本の重なる字面を、セル高を超える段落boxの積み重ねへ変換しない所有PDFを作る。
fn overfull_cell_source(gap: u32, second_size: u32) -> Vec<u8> {
    owned_pdf_content(&format!(
        "1 w 72 670 m 272 670 l S 72 700 m 272 700 l S 72 670 m 72 700 l S 172 670 m 172 700 l S 272 670 m 272 700 l S BT /F1 10 Tf 82 680 Td (Alpha) Tj ET BT /F1 {second_size} Tf 92 {} Td (Beta) Tj ET BT /F1 10 Tf 182 680 Td (Control) Tj ET",
        680 - gap,
    ))
}

/// 二つの行boxと原本上余白が30ptを超えるなら、編集可能な旧セル本文を保持する。
#[test]
fn cell_line_boxes_exceeding_source_height_keep_legacy_layout() {
    let p = paragraphs(&first_cell_from_source(&overfull_cell_source(6, 14)));
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].0, "Alpha Beta");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/native_table_fit.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 重なる原本字面の間隔やem高を変えても、原本セルより高い段落列を公開しない。
    #[test]
    fn overfull_cell_line_boxes_keep_all_text(gap in 5u32..9, second_size in 14u32..25) {
        let p = paragraphs(&first_cell_from_source(&overfull_cell_source(gap, second_size)));
        prop_assert_eq!(p.len(), 1);
        prop_assert_eq!(&p[0].0, "Alpha Beta");
    }
}
