//! 正規PDF座標で描いた表の上下順を、描画順や数字の大小で反転しない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// 三行二列の表を、指定された任意の描画順で正式xref付きPDFへ描く。
fn source(labels: [&str; 3], order: [usize; 3]) -> Vec<u8> {
    let mut content = String::from(
        "72 580 m 272 580 l S 72 620 m 272 620 l S 72 660 m 272 660 l S 72 700 m 272 700 l S 72 580 m 72 700 l S 172 580 m 172 700 l S 272 580 m 272 700 l S ",
    );
    for row in order {
        let y = 680 - row * 40;
        let label = labels[row];
        let control = ["Top", "Middle", "Bottom"][row];
        content.push_str(&format!(
            "BT /F1 10 Tf 82 {y} Td ({label}) Tj ET BT /F1 10 Tf 182 {y} Td ({control}) Tj ET "
        ));
    }
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

/// 製品byte APIの表をXMLで読み、実際の行・セル順に本文を返す。
fn rows(labels: [&str; 3], order: [usize; 3]) -> Vec<Vec<String>> {
    let output =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-table-order.pdf", &source(labels, order))
            .unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(output)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let mut reader = Reader::from_str(&xml);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut in_text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(tag) if tag.name().as_ref() == b"w:tr" => row.clear(),
            Event::Start(tag) if tag.name().as_ref() == b"w:tc" => cell.clear(),
            Event::Start(tag) if tag.name().as_ref() == b"w:t" => in_text = true,
            Event::End(tag) if tag.name().as_ref() == b"w:t" => in_text = false,
            Event::Text(text) if in_text => cell.push_str(&text.decode().unwrap()),
            Event::End(tag) if tag.name().as_ref() == b"w:tc" => row.push(cell.clone()),
            Event::End(tag) if tag.name().as_ref() == b"w:tr" => rows.push(row.clone()),
            Event::Eof => break,
            _ => {}
        }
    }
    rows
}

/// 値の降順で並ぶ表も、原本の上段から下段の座標順を保つ。
#[test]
fn descending_values_do_not_reverse_source_rows() {
    assert_eq!(
        rows(["9", "5", "1"], [0, 1, 2]),
        [["9", "Top"], ["5", "Middle"], ["1", "Bottom"]]
    );
}

/// 下段から先に描かれたPDFでも、paint順を表の上下方向と取り違えない。
#[test]
fn reverse_paint_order_does_not_reverse_source_rows() {
    assert_eq!(
        rows(["Alpha", "Beta", "Gamma"], [2, 1, 0]),
        [["Alpha", "Top"], ["Beta", "Middle"], ["Gamma", "Bottom"]]
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 32,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/native_table_coordinate_order.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 三行の全描画順と任意の数値に対し、独立した原本座標の行・列対応が不変である。
    #[test]
    fn coordinate_order_is_independent_of_paint_order_and_values(
        permutation in 0usize..6,
        values in prop::array::uniform3(0u32..1000),
    ) {
        let orders = [[0,1,2], [0,2,1], [1,0,2], [1,2,0], [2,0,1], [2,1,0]];
        let strings = values.map(|n| n.to_string());
        let expected = vec![
            vec![strings[0].clone(), "Top".to_string()],
            vec![strings[1].clone(), "Middle".to_string()],
            vec![strings[2].clone(), "Bottom".to_string()],
        ];
        prop_assert_eq!(rows(strings.each_ref().map(String::as_str), orders[permutation]), expected);
    }
}
