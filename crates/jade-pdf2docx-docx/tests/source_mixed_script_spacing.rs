//! PDF由来の混植表にWordの自動字間を加えない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read, Seek};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 和文・英字・数字を同じ原本基線に置き、セル右端だけを変える。
fn source(edge: u32) -> Vec<u8> {
    let content = format!(
        concat!(
            "0 g 0 G .5 w 40 400 m 202 400 l S 40 423 m 202 423 l S 40 446 m 202 446 l S ",
            "40 400 m 40 446 l S {} 400 m {} 446 l S 202 400 m 202 446 l S ",
            "BT /F 12 Tf 41 429 Td <69D800417B2C003553F7> Tj ET ",
            "BT /F 12 Tf 41 406 Td <69D800427B2C003653F7> Tj ET ",
            "BT /L 12 Tf 96 429 Td (First) Tj ET BT /L 12 Tf 96 406 Td (Second) Tj ET"
        ),
        edge, edge
    );
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 612 792] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F 5 0 R /L 8 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(&content),
        "<< /Type /Font /Subtype /Type0 /BaseFont /NotoSansJP /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>".into(),
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NotoSansJP /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /DW 1000 /W [53 [515 515] 65 [570 570]] >>".into(),
        fixture::stream("1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n7 beginbfchar\n<69D8> <69D8>\n<7B2C> <7B2C>\n<53F7> <53F7>\n<0041> <0041>\n<0042> <0042>\n<0035> <0035>\n<0036> <0036>\nendbfchar"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 元のセル幅と文字列を保ち、既定の段落書式で自動字間を止める。
#[test]
fn converted_mixed_script_table_preserves_source_spacing() {
    let pdf = source(91);
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-mixed-table.pdf", &pdf).unwrap();
    check_package(Cursor::new(bytes), 1020, Some(980));

    #[cfg(not(target_arch = "wasm32"))]
    {
        use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("owned-mixed-table.pdf");
        let output = directory.path().join("converted.docx");
        std::fs::write(&input, pdf).unwrap();
        convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
            input_pdf_path: input,
            output_docx_path: output.clone(),
            ocr_text_lines: None,
            ocr_table: None,
            clean_output: true,
        })
        .unwrap();
        check_package(std::fs::File::open(output).unwrap(), 1020, Some(980));
    }
}

/// 原本の右辺に余裕がある場合は、明示的なrun幅を設定しない。
#[test]
fn cells_with_source_clearance_keep_normal_word_advances() {
    let bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-wide-table.pdf", &source(94)).unwrap();
    check_package(Cursor::new(bytes), 1080, None);
}

/// 変換物のスタイルXMLと表セルを、最終ZIPから読む。
fn check_package(input: impl Read + Seek, cell_width: u32, fitted_width: Option<u32>) {
    let mut archive = zip::ZipArchive::new(input).unwrap();
    let mut read = |name| {
        let mut content = String::new();
        archive
            .by_name(name)
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        content
    };
    let styles = read("word/styles.xml");
    let defaults = styles
        .split_once("<w:pPrDefault>")
        .unwrap()
        .1
        .split_once("</w:pPrDefault>")
        .unwrap()
        .0;
    for flag in ["w:autoSpaceDE", "w:autoSpaceDN"] {
        assert!(
            defaults.contains(&format!(r#"<{flag} w:val="0""#)),
            "{flag}: {defaults}"
        );
    }
    let document = read("word/document.xml");
    assert_eq!(document.matches("<w:tbl>").count(), 1);
    for text in ["様A第5号", "様B第6号"] {
        let cell = document
            .split("<w:tc>")
            .skip(1)
            .find(|cell| cell.split("</w:tc>").next().unwrap().contains(text))
            .unwrap();
        let cell = cell.split_once("</w:tc>").unwrap().0;
        assert!(
            cell.contains(&format!(r#"<w:tcW w:w="{cell_width}" w:type="dxa""#)),
            "{cell}"
        );
        assert_eq!(cell.matches("<w:t ").count(), 1, "{cell}");
        if let Some(width) = fitted_width {
            assert!(
                cell.contains(&format!(r#"<w:fitText w:val="{width}""#)),
                "{cell}"
            );
        }
    }
    assert_eq!(
        document.matches("<w:fitText").count(),
        usize::from(fitted_width.is_some()) * 2
    );
}
