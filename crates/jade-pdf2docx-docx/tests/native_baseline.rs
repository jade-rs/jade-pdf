use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};

#[allow(dead_code)]
#[path = "support/docx_assert.rs"]
mod docx_assert;

/// 複数字体の字面高が違っても、同じSDK baselineを置換文字と一緒に保持する。
#[test]
fn authored_replacement_keeps_shared_native_baseline() {
    for replacement in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("owned.pdf");
        let output = directory.path().join("owned.docx");
        let source = owned_pdf(replacement);
        std::fs::write(&input, &source).unwrap();
        convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
            input_pdf_path: input.clone(),
            output_docx_path: output.clone(),
            ocr_text_lines: None,
            ocr_table: None,
            clean_output: true,
        })
        .unwrap();
        let expected = if replacement {
            "alphaa—beta"
        } else {
            "alpha—beta"
        };
        let paragraphs = docx_assert::paragraph_texts_in_docx(&output);
        assert!(
            paragraphs.iter().any(|text| text == expected),
            "replacement={replacement}; shared baseline split or reordered: {paragraphs:?}"
        );
        assert_eq!(std::fs::read(&input).unwrap(), source);
    }
}

/// Base14字体だけを使い、著者指定のActualTextでnative合成を必要とするowned PDFを作る。
fn owned_pdf(replacement: bool) -> Vec<u8> {
    let before = if replacement {
        "/Span << /ActualText <FEFF0061006C0070006800610061> >> BDC "
    } else {
        ""
    };
    let after = if replacement { " EMC" } else { "" };
    let content = format!(
        "{before}BT /F1 12 Tf 20 100 Td (alpha) Tj ET{after} {}",
        r"BT /F2 12 Tf 49.352 100 Td (\227) Tj ET BT /F1 12 Tf 61.352 100 Td (beta) Tj ET"
    );
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>".into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding >>".into(),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let at = bytes.len();
    bytes.extend_from_slice(b"xref\n0 7\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer << /Size 7 /Root 1 0 R >>\nstartxref\n{at}\n%%EOF\n").as_bytes(),
    );
    bytes
}
