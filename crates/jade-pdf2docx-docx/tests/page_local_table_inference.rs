//! 罫線表のある別ページが、罫線のない反復列のWord表を消さない契約。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// ページごとの表を独立した内容streamへ置き、正確なxrefを持つ所有PDFを作る。
fn source(pages: &[bool], left: u16, width: u16) -> Vec<u8> {
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
        for row in 0..3 {
            for col in 0..2 {
                let x = left + col * width + 10;
                let y = 680 - row * 60;
                let label = if ruled { "RULED" } else { "GRID" };
                content.push_str(&format!(
                    "BT /F1 10 Tf {x} {y} Td ({label}-R{row}C{col}) Tj ET\n"
                ));
            }
        }
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font} 0 R >> >> /Contents {} 0 R >>",4+page*2));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ));
    }
    objects.push(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
    );
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
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-page-local-tables.pdf",
        &source(pages, left, width),
    )
    .unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let mut reader = Reader::from_str(&xml);
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 位置・列幅・ページ順を変えても、ページを結合したWord表の内容は独立変換と一致する。
    #[test]
    fn table_inference_is_local_to_each_page(left in 20u16..60, width in 160u16..240, ruled_first in any::<bool>()) {
        let pages=[ruled_first,!ruled_first];
        let independent=pages.into_iter().flat_map(|ruled|tables(&[ruled],left,width)).collect::<Vec<_>>();
        prop_assert_eq!(tables(&pages,left,width),independent);
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
