//! 間接Contents配列を、元の順序・繰り返し・世代を保って解決する回帰。
use jade_pdf2docx_parser::{ParseError, ParseOptions, parse_pdf_bytes_with_options};
use proptest::prelude::*;

/// 任意の世代を持つobjectから、正式xref付きの所有PDFをメモリ内で作る。
fn pdf(objects: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut data = b"%PDF-1.2\n".to_vec();
    let mut offsets = Vec::new();
    for (index, (generation, body)) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend_from_slice(format!("{} {generation} obj\n", index + 1).as_bytes());
        data.extend_from_slice(body);
        data.extend_from_slice(b"\nendobj\n");
    }
    let xref = data.len();
    data.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for (offset, (generation, _)) in offsets.iter().zip(objects) {
        data.extend_from_slice(format!("{offset:010} {generation:05} n \n").as_bytes());
    }
    data.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    data
}

/// 無圧縮の命令列を、正確なLengthを持つstreamへ入れる。
fn stream(content: &str) -> Vec<u8> {
    format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
    .into_bytes()
}

/// Contentsの指定と、その間接配列だけを変えられる二つの本文stream。
fn objects(
    contents: &str,
    array: &str,
    generation: u16,
    first: &str,
    second: &str,
) -> Vec<(u16, Vec<u8>)> {
    vec![
        (0, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (0, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (0, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents {contents} >>").into_bytes()),
        (0, stream(first)),
        (0, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec()),
        (generation, array.as_bytes().to_vec()),
        (0, stream(second)),
    ]
}

/// 製品のpure byte入口で、本文の順番を取り出す。
fn texts(objects: &[(u16, Vec<u8>)]) -> Vec<String> {
    parse_pdf_bytes_with_options(
        "owned-indirect-contents.pdf",
        &pdf(objects),
        &ParseOptions::default(),
    )
    .unwrap()
    .text_elements
    .into_iter()
    .map(|text| text.text)
    .collect()
}

/// 間接配列をstreamと誤認せず、stream境界を越えて文字状態を保持する。
#[test]
fn indirect_contents_array_keeps_order_and_text_state() {
    let objects = objects(
        "6 0 R",
        "[4 0 R 7 0 R]",
        0,
        "BT /F1 12 Tf 72 700 Td (FIRST) Tj",
        "(SECOND) Tj ET",
    );
    assert_eq!(texts(&objects), ["FIRST", "SECOND"]);
}

/// 同一streamを二度参照する命令は、二回の描画として実行する。
#[test]
fn repeated_contents_references_are_not_deduplicated() {
    for contents in ["[4 0 R 7 0 R 7 0 R 8 0 R]", "6 0 R"] {
        let mut objects = objects(
            contents,
            "[4 0 R 7 0 R 7 0 R 8 0 R]",
            0,
            "BT /F1 12 Tf 72 700 Td",
            "(A) Tj",
        );
        objects.push((0, stream("ET")));
        let parsed =
            parse_pdf_bytes_with_options("repeat.pdf", &pdf(&objects), &ParseOptions::default())
                .unwrap();
        let text = parsed
            .text_elements
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(text, ["A", "A"]);
        assert!(parsed.text_elements[1].bounds.x0 > parsed.text_elements[0].bounds.x0);
    }
}

/// Contents配列に至るaliasも解決し、配列本文ではなく実streamを実行する。
#[test]
fn indirect_array_alias_is_resolved() {
    let mut objects = objects(
        "8 0 R",
        "[4 0 R 7 0 R]",
        0,
        "BT /F1 12 Tf 72 700 Td (FIRST) Tj",
        "(SECOND) Tj ET",
    );
    objects.push((0, b"6 0 R".to_vec()));
    assert_eq!(texts(&objects), ["FIRST", "SECOND"]);
}

/// 誤世代・循環・配列の入れ子を、空白文書への黙った置換で受け入れない。
#[test]
fn invalid_contents_targets_are_structural_errors() {
    for (contents, array, generation) in [
        ("6 1 R", "[4 0 R 7 0 R]", 0),
        ("6 0 R", "6 0 R", 0),
        ("[6 0 R]", "[4 0 R 7 0 R]", 0),
    ] {
        let objects = objects(
            contents,
            array,
            generation,
            "BT /F1 12 Tf 72 700 Td (FIRST) Tj",
            "(SECOND) Tj ET",
        );
        assert!(
            matches!(
                parse_pdf_bytes_with_options(
                    "invalid-contents.pdf",
                    &pdf(&objects),
                    &ParseOptions::default()
                ),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "{contents} {array}"
        );
    }
}

/// 空配列とnullは、本当に空白のページとして引き続き受け入れる。
#[test]
fn empty_contents_are_not_reported_as_content_loss() {
    for contents in ["[]", "null"] {
        let objects = objects(contents, "[]", 0, "", "");
        let parsed = parse_pdf_bytes_with_options(
            "blank.pdf",
            &pdf(&objects),
            &ParseOptions {
                strict: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(parsed.page_count, 1);
        assert!(parsed.text_elements.is_empty() && parsed.quality.issues.is_empty());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 間接化と有効な世代は、直接Contents配列の文字・位置・順番を変えない。
    #[test]
    fn indirect_contents_preserve_direct_array_ir(
        first in "[A-Za-z]{1,20}", second in "[A-Za-z]{1,20}", generation in 0u16..8,
    ) {
        let start = format!("BT /F1 12 Tf 72 700 Td ({first}) Tj");
        let end = format!("({second}) Tj ET");
        let direct = objects("[4 0 R 7 0 R]", "[4 0 R 7 0 R]", generation, &start, &end);
        let indirect = objects(&format!("6 {generation} R"), "[4 0 R 7 0 R]", generation, &start, &end);
        let expected = parse_pdf_bytes_with_options("direct.pdf", &pdf(&direct), &ParseOptions::default()).unwrap();
        let actual = parse_pdf_bytes_with_options("indirect.pdf", &pdf(&indirect), &ParseOptions::default()).unwrap();
        prop_assert!(!expected.text_elements.is_empty());
        prop_assert_eq!(actual.text_elements, expected.text_elements);
        prop_assert_eq!(actual.pages, expected.pages);
        prop_assert_eq!(actual.quality, expected.quality);
    }
}
