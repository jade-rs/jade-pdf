//! 罫線表のある別ページが、罫線のない反復列のWord表を消さない契約。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

#[path = "support/pdf_builders.rs"]
mod pdf_builders;

#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

#[path = "support/page_local_source_style_cases.rs"]
mod source_style_cases;

#[path = "support/page_local_source_order_cases.rs"]
mod source_order_cases;

/// ページごとの表を独立した内容streamへ置き、正確なxrefを持つ所有PDFを作る。
fn source(pages: &[bool], left: u16, width: u16) -> Vec<u8> {
    source_with_column_page(pages, left, width, None)
}

/// 一つだけ長い左右本文を置く追加対照を作り、既存の所有PDFの生成結果を保つ。
fn source_with_column_page(
    pages: &[bool],
    left: u16,
    width: u16,
    column_page: Option<usize>,
) -> Vec<u8> {
    source_with_note(pages, left, width, column_page, None)
}

/// 新しい混在ページ対照だけに一段の注記を挟み、既存の所有原本は変更しない。
fn source_with_note(
    pages: &[bool],
    left: u16,
    width: u16,
    column_page: Option<usize>,
    note: Option<(usize, &str)>,
) -> Vec<u8> {
    source_with_palette(pages, left, width, column_page, note, false)
}

/// 既存の原本byteを保ち、新しい対照だけで一セル内の書式を切り替える。
fn source_with_palette(
    pages: &[bool],
    left: u16,
    width: u16,
    column_page: Option<usize>,
    note: Option<(usize, &str)>,
    palette: bool,
) -> Vec<u8> {
    let font = 3 + pages.len() * 2;
    let kids = (0..pages.len())
        .map(|i| format!("{} 0 R", 3 + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Count {} /Kids [{kids}] >>", pages.len()),
    ];
    for (page, ruled) in pages.iter().copied().enumerate() {
        let mut content = String::new();
        if ruled {
            for row in 0..=3 {
                let y = 540 + row * 60;
                content.push_str(&format!("1 w {left} {y} m {} {y} l S\n", left + width * 2));
            }
            for col in 0..=2 {
                let x = left + col * width;
                content.push_str(&format!("{x} 540 m {x} 720 l S\n"));
            }
        }
        let columns = column_page == Some(page);
        for row in 0..if columns { 12 } else { 3 } {
            for col in 0..2 {
                let x = left + col * width + 10;
                let y = 680 - row * if columns { 20 } else { 60 };
                let text = if columns {
                    format!(
                        "COLUMN {} ROW {row}",
                        if col == 0 { "LEFT" } else { "RIGHT" }
                    )
                } else {
                    format!("{}-R{row}C{col}", if ruled { "RULED" } else { "GRID" })
                };
                if palette && !ruled {
                    if col == 1 {
                        content.push_str(&format!("0 g BT /F1 10 Tf {x} {y} Td (GRID-) Tj /F2 12 Tf 1 0 0 rg (R{row}C{col}) Tj ET\n"));
                    } else {
                        content.push_str(&format!("0 g BT /F1 10 Tf {x} {y} Td ({text}) Tj ET\n"));
                    }
                } else {
                    content.push_str(&format!("BT /F1 10 Tf {x} {y} Td ({text}) Tj ET\n"));
                }
            }
        }
        if let Some((note_page, text)) = note
            && note_page == page
        {
            content.push_str(&format!(
                "BT /F1 10 Tf {} 650 Td ({text}) Tj ET\n",
                left + 10
            ));
        }
        let fonts = if palette {
            format!("/F1 {font} 0 R /F2 {} 0 R", font + 1)
        } else {
            format!("/F1 {font} 0 R")
        };
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << {fonts} >> >> /Contents {} 0 R >>",4+page*2));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    objects.push(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
    );
    if palette {
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".into());
    }
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

/// 公開byte変換の梱包済みWord表を読み、通常段落に文字があるだけの結果と区別する。
fn tables(pages: &[bool], left: u16, width: u16) -> Vec<Vec<Vec<String>>> {
    let xml = document_xml(&source(pages, left, width));
    tables_in_xml(&xml)
}

/// 表数だけでは捉えられない実段組も、同じ公開変換の完成パッケージから確認する。
fn document_xml(source: &[u8]) -> String {
    let bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-page-local-tables.pdf", source).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 実際のWord表だけを読み、本文段落を追加セルとして数えない。
fn tables_in_xml(xml: &str) -> Vec<Vec<Vec<String>>> {
    let mut reader = Reader::from_str(xml);
    let mut result = Vec::new();
    let mut table = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut in_cell = false;
    let mut in_text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(t) if t.name().as_ref() == b"w:tbl" => table.clear(),
            Event::Start(t) if t.name().as_ref() == b"w:tr" => row.clear(),
            Event::Start(t) if t.name().as_ref() == b"w:tc" => {
                cell.clear();
                in_cell = true;
            }
            Event::Start(t) if t.name().as_ref() == b"w:t" => in_text = true,
            Event::Text(t) if in_cell && in_text => cell.push_str(&t.decode().unwrap()),
            Event::End(t) if t.name().as_ref() == b"w:t" => in_text = false,
            Event::End(t) if t.name().as_ref() == b"w:tc" => {
                row.push(cell.clone());
                in_cell = false;
            }
            Event::End(t) if t.name().as_ref() == b"w:tr" => table.push(row.clone()),
            Event::End(t) if t.name().as_ref() == b"w:tbl" => result.push(table.clone()),
            Event::Eof => break,
            _ => {}
        }
    }
    result
}

/// 所有する各ページのセル行・列順を、変換結果から作らず固定する。
fn expected(ruled: bool) -> Vec<Vec<String>> {
    let label = if ruled { "RULED" } else { "GRID" };
    (0..3)
        .map(|r| (0..2).map(|c| format!("{label}-R{r}C{c}")).collect())
        .collect()
}

/// 単独なら両方の表形式を編集可能なWord表に変換できる既存の対照を保つ。
#[test]
fn each_table_kind_alone_preserves_all_cells() {
    for ruled in [false, true] {
        assert_eq!(tables(&[ruled], 40, 240), vec![expected(ruled)]);
    }
}

/// 前後どちらの罫線表も、別ページの反復列の表を無効化しない。
#[test]
fn a_ruled_table_on_another_page_does_not_remove_the_borderless_table() {
    for pages in [[true, false], [false, true]] {
        assert_eq!(
            tables(&pages, 40, 240),
            pages.into_iter().map(expected).collect::<Vec<_>>()
        );
    }
}

/// 別ページの罫線表がある原本で、長い左右本文の実段組を偽の表へ変えない。
#[test]
fn independent_page_inference_preserves_body_columns_between_ruled_tables() {
    for column_page in 0..3 {
        let mut pages = [true; 3];
        pages[column_page] = false;
        let source = source_with_column_page(&pages, 40, 240, Some(column_page));
        let xml = document_xml(&source);
        assert_eq!(tables_in_xml(&xml), vec![expected(true); 2]);
        assert_eq!(xml.matches("w:num=\"2\"").count(), 1);
        assert_eq!(xml.matches("<w:sectPr").count(), 3);
        for side in ["LEFT", "RIGHT"] {
            for row in 0..12 {
                assert!(xml.contains(&format!("COLUMN {side} ROW {row}")));
            }
        }
    }
}

/// 新しい表候補が範囲内の注記を出力できなければ、本文を残して表への昇格を止める。
#[test]
fn a_new_borderless_table_must_not_drop_an_interleaved_body_note() {
    let xml = document_xml(&source_with_note(
        &[true, false],
        40,
        240,
        None,
        Some((1, "BODY MUST REMAIN")),
    ));
    assert!(xml.contains("BODY MUST REMAIN"));
    assert_eq!(tables_in_xml(&xml), vec![expected(true)]);
    for row in 0..3 {
        for col in 0..2 {
            assert_eq!(xml.matches(&format!("GRID-R{row}C{col}")).count(), 1);
        }
    }
}

/// 罫線のない短い表を置き、位置の異なる注記を追加できる原本を作る。
fn borderless_table_with_note(note: &str) -> String {
    pdf_builders::pdf(
        &format!(
            "BT /F1 10 Tf 18 155 Td (R0C0) Tj ET\n\
             BT /F1 10 Tf 153 155 Td (R0C1) Tj ET\n\
             BT /F1 10 Tf 18 120 Td (R1C0) Tj ET\n\
             BT /F1 10 Tf 153 120 Td (R1C1) Tj ET\n\
             BT /F1 10 Tf 18 85 Td (R2C0) Tj ET\n\
             BT /F1 10 Tf 153 85 Td (R2C1) Tj ET\n{note}"
        ),
        true,
    )
}

/// 列位置が一致しない表の直後の注記を、編集可能な表と一緒に残す。
#[test]
fn borderless_table_keeps_trailing_notes_in_the_body() {
    let source = borderless_table_with_note(
        "BT /F1 10 Tf 18 55 Td (Notes) Tj ET\n\
         BT /F1 10 Tf 84 55 Td (See below) Tj ET",
    );
    let xml = document_xml(source.as_bytes());
    assert_eq!(xml.matches("Notes").count(), 1);
    assert_eq!(xml.matches("See below").count(), 1);
    assert_eq!(
        tables_in_xml(&xml),
        vec![vec![
            vec!["R0C0", "R0C1"],
            vec!["R1C0", "R1C1"],
            vec!["R2C0", "R2C1"]
        ]],
    );
    assert!(xml.find("</w:tbl>").unwrap() < xml.find("Notes").unwrap());
}

/// 表の範囲内に列へ割り当てられない注記があれば、全断片を本文として残す。
#[test]
fn borderless_table_keeps_interleaved_notes_in_the_body() {
    let source = borderless_table_with_note("BT /F1 10 Tf 18 135 Td (Between rows) Tj ET");
    let xml = document_xml(source.as_bytes());
    assert!(tables_in_xml(&xml).is_empty());
    assert_eq!(xml.matches("Between rows").count(), 1);
    for row in 0..3 {
        for col in 0..2 {
            assert_eq!(xml.matches(&format!("R{row}C{col}")).count(), 1);
        }
    }
}

/// 枠内の見出しを置き去りにする反復列候補で、原本の罫線表を上書きしない。
#[test]
fn a_text_grid_cannot_replace_a_frame_by_leaving_its_header_outside() {
    let mut content = String::from(
        "0.5 w 20 180 m 220 180 l S 20 165 m 220 165 l S 20 80 m 220 80 l S \
         20 80 m 20 180 l S 120 80 m 120 180 l S 220 80 m 220 180 l S \
         BT /F1 10 Tf 25 169 Td (Item) Tj ET BT /F1 10 Tf 125 169 Td (Amount) Tj ET",
    );
    for (y, label, amount) in [
        (150, "Cash", "9,000"),
        (132, "Loans", "7,000"),
        (114, "Bonds", "5,000"),
        (96, "Other", "3,000"),
    ] {
        content.push_str(&format!(
            " BT /F1 10 Tf 25 {y} Td ({label}) Tj ET BT /F1 10 Tf 160 {y} Td ({amount}) Tj ET"
        ));
    }
    let pdf = page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".into(),
        page_attribute_pdf::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
    ]);
    let xml = document_xml(&pdf);
    let tables = tables_in_xml(&xml);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0][0], ["Item", "Amount"]);
    assert!(tables[0][1][0].contains("Cash"));
    assert!(tables[0][1][0].contains("Other"));
    assert_eq!(xml.matches(">Item</w:t>").count(), 1);
    assert!(xml.contains(r#"<w:top w:val="single""#));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 位置・列幅・ページ順を変えても、ページを結合したWord表の内容は独立変換と一致する。
    #[test]
    fn table_inference_is_local_to_each_page(left in 20u16..60, width in 160u16..240, ruled_first in any::<bool>()) {
        let pages=[ruled_first,!ruled_first];
        let independent=pages.into_iter().flat_map(|ruled|tables(&[ruled],left,width)).collect::<Vec<_>>();
        prop_assert_eq!(tables(&pages,left,width),independent);
    }

    /// 注記の文字・位置・列幅・ページ順が変わっても、表の追加による本文削除を許さない。
    #[test]
    fn new_mixed_page_tables_never_discard_owned_note(
        letters in "[a-z]{1,20}", left in 20u16..60, width in 160u16..240, note_first in any::<bool>()
    ) {
        let pages=if note_first {[false,true]}else{[true,false]};
        let page=usize::from(!note_first);
        let note=format!("Note{letters}");
        let xml=document_xml(&source_with_note(&pages,left,width,None,Some((page,&note))));
        prop_assert_eq!(xml.matches(&note).count(),1);
        prop_assert_eq!(tables_in_xml(&xml),vec![expected(true)]);
    }
}

/// 別の新規一時名前空間だけへ、原本全ページを確認するための所有対照を書き出す。
#[test]
#[ignore = "new JADE_PAGE_LOCAL_TABLE_EXPORT_DIR required; review all source pages before file conversion"]
fn export_owned_page_local_table_sources_before_file_conversion() {
    let dir =
        std::path::PathBuf::from(std::env::var_os("JADE_PAGE_LOCAL_TABLE_EXPORT_DIR").unwrap());
    std::fs::create_dir(&dir).unwrap();
    for (name, pages) in [
        ("ruled-only", vec![true]),
        ("borderless-only", vec![false]),
        ("ruled-then-borderless", vec![true, false]),
        ("borderless-then-ruled", vec![false, true]),
    ] {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(format!("{name}.pdf")))
            .unwrap();
        std::io::Write::write_all(&mut file, &source(&pages, 40, 240)).unwrap();
    }
}

/// 注記を挟む追加対照は別の新規一時名前空間へ書き、固定済み原本を上書きしない。
#[test]
#[ignore = "new JADE_PAGE_LOCAL_NOTE_EXPORT_DIR required; review all source pages before file conversion"]
fn export_owned_mixed_page_note_sources_before_file_conversion() {
    let dir =
        std::path::PathBuf::from(std::env::var_os("JADE_PAGE_LOCAL_NOTE_EXPORT_DIR").unwrap());
    std::fs::create_dir(&dir).unwrap();
    for (name, pages, page) in [
        ("ruled-then-note", [true, false], 1),
        ("note-then-ruled", [false, true], 0),
    ] {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(format!("{name}.pdf")))
            .unwrap();
        std::io::Write::write_all(
            &mut file,
            &source_with_note(&pages, 40, 240, None, Some((page, "BODY MUST REMAIN"))),
        )
        .unwrap();
    }
}
