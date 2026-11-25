//! 罫線が省略された本文行でも、列をまたぐ原本の行の対応をWordで編集できるように保つ。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// 横結合した見出しと本文の列罫線を持ち、各列へ同じbaselineの値を描く所有原本。
fn source(count: u16, pitch: u16, reverse: bool, synchronized: bool) -> Vec<u8> {
    source_with_units(count, pitch, reverse, synchronized, false)
}

/// 数値列だけの小さい単位行を本文帯の先頭に追加し、名称列と本文の行数を変える。
fn source_with_units(
    count: u16,
    pitch: u16,
    reverse: bool,
    synchronized: bool,
    units: bool,
) -> Vec<u8> {
    let bottom = 650 - count * pitch;
    let mut content = format!(
        "1 w 72 {bottom} m 272 {bottom} l S 72 670 m 272 670 l S 72 700 m 272 700 l S 72 {bottom} m 72 700 l S 172 {bottom} m 172 670 l S 272 {bottom} m 272 700 l S BT /F1 10 Tf 82 680 Td (Summary) Tj ET"
    );
    if units {
        content.push_str(" BT /F1 6 Tf 182 663 Td (units) Tj ET");
    }
    let mut order = (0..count).collect::<Vec<_>>();
    if reverse {
        order.reverse();
    }
    for index in order {
        let y = 650 - index * pitch;
        content.push_str(&format!(" BT /F1 10 Tf 82 {y} Td (Label-{index}) Tj ET"));
        if synchronized || index == 0 {
            content.push_str(&format!(" BT /F1 10 Tf 182 {y} Td (Value-{index}) Tj ET"));
        }
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

/// 公開byte APIのDOCXから、Word表の行とセルの本文を読み戻す。
fn table_rows(source: &[u8]) -> Vec<Vec<String>> {
    let xml = document_xml(source);
    let mut reader = Reader::from_str(&xml);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(tag) if tag.name().as_ref() == b"w:tr" => row.clear(),
            Event::Start(tag) if tag.name().as_ref() == b"w:tc" => cell.clear(),
            Event::Start(tag) if tag.name().as_ref() == b"w:t" => text = true,
            Event::End(tag) if tag.name().as_ref() == b"w:t" => text = false,
            Event::Text(value) if text => cell.push_str(&value.decode().unwrap()),
            Event::End(tag) if tag.name().as_ref() == b"w:tc" => row.push(cell.clone()),
            Event::End(tag) if tag.name().as_ref() == b"w:tr" => rows.push(row.clone()),
            Event::Eof => return rows,
            _ => {}
        }
    }
}

/// 公開byte APIの出力本文XMLを、閉じたDOCX archiveから取り出す。
fn document_xml(source: &[u8]) -> String {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-logical-rows.pdf", source).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 属性の順序に依存せず、各Word行の高さとセルの上下辺の有無を読む。
fn geometry(xml: &str) -> Vec<(u32, Vec<(bool, bool)>)> {
    let mut reader = Reader::from_str(xml);
    let mut rows = Vec::new();
    let mut row = (0, Vec::new());
    let mut cell = (false, false);
    loop {
        match reader.read_event().unwrap() {
            Event::Start(tag) if tag.name().as_ref() == b"w:tr" => row = (0, Vec::new()),
            Event::Start(tag) if tag.name().as_ref() == b"w:tc" => cell = (false, false),
            Event::Empty(tag) => {
                let value = tag
                    .attributes()
                    .map(Result::unwrap)
                    .find(|a| a.key.as_ref() == b"w:val");
                if let Some(value) = value {
                    match tag.name().as_ref() {
                        b"w:trHeight" => {
                            row.0 = std::str::from_utf8(&value.value).unwrap().parse().unwrap()
                        }
                        b"w:top" => cell.0 = value.value.as_ref() != b"nil",
                        b"w:bottom" => cell.1 = value.value.as_ref() != b"nil",
                        _ => {}
                    }
                }
            }
            Event::End(tag) if tag.name().as_ref() == b"w:tc" => row.1.push(cell),
            Event::End(tag) if tag.name().as_ref() == b"w:tr" => rows.push(row.clone()),
            Event::Eof => return rows,
            _ => {}
        }
    }
}

/// 編集可能な行を増やしても原本にない横辺を描かず、外枠と表全体の高さを保つ。
#[test]
fn logical_rows_keep_original_edges_and_total_height() {
    for units in [false, true] {
        let rows = geometry(&document_xml(&source_with_units(3, 25, false, true, units)));
        assert_eq!(rows.len(), 4 + usize::from(units));
        assert_eq!(
            rows.iter().map(|row| row.0).sum::<u32>(),
            (50 + 3 * 25) * 20
        );
        assert_eq!(rows[0].1, [(true, true)]);
        assert!(rows[1].1.iter().all(|edges| *edges == (true, false)));
        assert!(
            rows[2..rows.len() - 1]
                .iter()
                .flat_map(|row| &row.1)
                .all(|edges| *edges == (false, false))
        );
        assert!(
            rows.last()
                .unwrap()
                .1
                .iter()
                .all(|edges| *edges == (false, true))
        );
    }
}

/// 文字の集合が一致しても、複数の名称・値を一つのWord行へまとめた出力は合格にしない。
#[test]
fn unruled_body_lines_retain_unambiguous_row_association() {
    assert_eq!(
        table_rows(&source(3, 25, false, true)),
        expected(3),
        "each original label must share a distinct Word row with its own value"
    );
}

/// 同期する反復行を持たない複数行のセルは、他列の一行と推測で分割しない。
#[test]
fn unmatched_multiline_cells_keep_the_original_grid_rows() {
    assert_eq!(table_rows(&source(3, 25, false, false)).len(), 2);
}

/// 数値列だけに単位があっても、三つの名称と値を再び一つのWord行へ潰さない。
#[test]
fn smaller_numeric_unit_prefix_does_not_hide_the_body_rows() {
    let mut rows = expected(3);
    rows.insert(1, vec![String::new(), "units".to_string()]);
    assert_eq!(
        table_rows(&source_with_units(3, 25, false, true, true)),
        rows
    );
}

/// 所有原本を描画する前に定義した、物理的な上から下への名称・値の対応。
fn expected(count: u16) -> Vec<Vec<String>> {
    std::iter::once(vec!["Summary".to_string()])
        .chain((0..count).map(|index| vec![format!("Label-{index}"), format!("Value-{index}")]))
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct("tests/source_table_logical_rows.proptest-regressions"))),
        ..ProptestConfig::default()
    })]
    /// 行数・行間・描画順にかかわらず、各値は対応する名称と同じ一意なWord行に残る。
    #[test]
    fn original_row_association_is_independent_of_paint_order(
        count in 3u16..7,
        pitch in 16u16..30,
        reverse in any::<bool>(),
        units in any::<bool>(),
    ) {
        let mut rows = expected(count);
        if units { rows.insert(1, vec![String::new(), "units".to_string()]); }
        prop_assert_eq!(table_rows(&source_with_units(count, pitch, reverse, true, units)), rows);
    }
}

/// 新規領域だけに所有原本を保存し、全ページの確認前にはfile製品APIへ渡さない。
#[test]
#[ignore = "fresh JADE_LOGICAL_TABLE_EXPORT_DIR required; review all source pages before file conversion"]
fn export_owned_logical_rows_before_file_product() {
    let dir = std::path::PathBuf::from(std::env::var_os("JADE_LOGICAL_TABLE_EXPORT_DIR").unwrap());
    std::fs::create_dir(&dir).unwrap();
    for (name, bytes) in [
        ("merged-header-synchronized.pdf", source(3, 25, false, true)),
        ("merged-header-bottom-up.pdf", source(3, 25, true, true)),
        (
            "merged-header-numeric-units.pdf",
            source_with_units(3, 25, false, true, true),
        ),
        (
            "merged-header-unmatched-multiline.pdf",
            source(3, 25, false, false),
        ),
    ] {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(name))
            .unwrap();
        file.write_all(&bytes).unwrap();
    }
}
