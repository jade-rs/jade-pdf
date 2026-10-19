use std::io::Write;

use flate2::{Compression, write::ZlibEncoder};
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use jade_pdf2docx_parser::parse_pdf_bytes;

/// 50ページの本文を保持し、通常変換でDOCXを生成する。
#[test]
fn large_synthetic_pdf_parses_and_converts_without_empty_docx() {
    let pdf_bytes = synthetic_multi_page_pdf(50);

    let parsed = parse_pdf_bytes("large-synthetic.pdf", &pdf_bytes)
        .expect("large synthetic PDF should parse");
    assert_eq!(parsed.page_count, 50);
    assert_eq!(parsed.text_elements.len(), 50);

    let docx_bytes = convert_minimum_pdf_bytes_to_docx_bytes("large-synthetic.pdf", &pdf_bytes)
        .expect("large synthetic PDF should convert to DOCX bytes");
    assert!(
        docx_bytes.len() > 1024,
        "large synthetic conversion should produce a non-trivial DOCX"
    );
}

/// 指定ページ数の本文を持つ圧縮PDFを作る。
fn synthetic_multi_page_pdf(page_count: usize) -> Vec<u8> {
    let mut contents = Vec::with_capacity(page_count);
    for page_index in 0..page_count {
        contents.push(format!(
            "BT /F1 10 Tf 1 0 0 1 20 150 Tm (Large page {}) Tj ET",
            page_index + 1
        ));
    }
    flate_pdf_with_page_contents(
        &contents
            .iter()
            .map(String::as_bytes)
            .collect::<Vec<&[u8]>>(),
    )
}

/// Page・Contents・Fontが重複しないobject番号を割り当てる。
fn flate_pdf_with_page_contents(contents: &[&[u8]]) -> Vec<u8> {
    let mut pdf = Vec::new();
    let font_object = 3 + 2 * contents.len();
    pdf.extend_from_slice(b"%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
    let kids = (0..contents.len())
        .map(|index| format!("{} 0 R", 3 + index))
        .collect::<Vec<_>>()
        .join(" ");
    pdf.extend_from_slice(
        format!(
            "2 0 obj << /Type /Pages /Kids [{kids}] /Count {} >> endobj\n",
            contents.len()
        )
        .as_bytes(),
    );
    for index in 0..contents.len() {
        let page_object = 3 + index;
        let content_object = 3 + contents.len() + index;
        pdf.extend_from_slice(
            format!(
                "{page_object} 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents {content_object} 0 R /Resources << /Font << /F1 {font_object} 0 R >> >> >> endobj\n"
            )
            .as_bytes(),
        );
    }
    for (index, content) in contents.iter().enumerate() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(content).expect("zlib write must succeed");
        let compressed = encoder.finish().expect("zlib finish must succeed");
        let object_number = 3 + contents.len() + index;
        pdf.extend_from_slice(
            format!(
                "{object_number} 0 obj << /Length {} /Filter /FlateDecode >> stream\n",
                compressed.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&compressed);
        pdf.extend_from_slice(b"\nendstream endobj\n");
    }
    pdf.extend_from_slice(format!("{font_object} 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n%%EOF\n").as_bytes());
    pdf
}
