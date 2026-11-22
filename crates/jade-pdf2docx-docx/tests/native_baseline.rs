use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};

#[path = "support/docx_assert.rs"]
mod docx_assert;

/// 複数字体の字面高が違っても、同じSDK baselineを置換文字と一緒に保持する。
#[test]
fn authored_replacement_keeps_shared_native_baseline() {
    for replacement in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("owned.pdf");
        let output = directory.path().join("owned.docx");
        let source = owned_pdf(replacement, 1.0, 0.0, 0.0);
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
fn owned_pdf(replacement: bool, scale: f32, dx: f32, dy: f32) -> Vec<u8> {
    let before = if replacement {
        "/Span << /ActualText <FEFF0061006C0070006800610061> >> BDC "
    } else {
        ""
    };
    let after = if replacement { " EMC" } else { "" };
    let font_size = 12.0 * scale;
    let baseline = 100.0 * scale + dy;
    let left = 20.0 * scale + dx;
    let middle = 49.352 * scale + dx;
    let right = 61.352 * scale + dx;
    let content = format!(
        r"{before}BT /F1 {font_size} Tf {left} {baseline} Td (alpha) Tj ET{after} BT /F2 {font_size} Tf {middle} {baseline} Td (\227) Tj ET BT /F1 {font_size} Tf {right} {baseline} Td (beta) Tj ET"
    );
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>",
            300.0 * scale + dx,
            200.0 * scale + dy
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding >>"
            .into(),
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

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(8))]
    /// 平行移動・拡大縮小でもnative fallbackの字面高へ依存せず同一行の文字順を保つ。
    #[test]
    fn native_baseline_survives_translation_and_uniform_scale(scale in 0.5f32..3.0, dx in 0f32..20.0, dy in 0f32..20.0) {
        for replacement in [false,true] {
            let directory=tempfile::tempdir().unwrap();
            let input=directory.path().join("owned.pdf");
            let output=directory.path().join("owned.docx");
            let source=owned_pdf(replacement,scale,dx,dy);
            std::fs::write(&input,&source).unwrap();
            convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest { input_pdf_path:input.clone(),output_docx_path:output.clone(),ocr_text_lines:None,ocr_table:None,clean_output:true }).unwrap();
            let expected=if replacement {"alphaa—beta"} else {"alpha—beta"};
            let paragraphs=docx_assert::paragraph_texts_in_docx(&output);
            proptest::prop_assert!(paragraphs.iter().any(|p| p==expected),"scale={}, dx={}, dy={}, replacement={}, {:?}",scale,dx,dy,replacement,paragraphs);
            proptest::prop_assert_eq!(std::fs::read(&input).unwrap(),source);
        }
    }
}
