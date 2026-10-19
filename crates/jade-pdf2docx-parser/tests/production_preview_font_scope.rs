//! xrefなしCMapの最新本文とページ・Fontごとのpreview復号を公開APIで検査する。
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;

/// 文字列streamを正確なLengthで保存する。
fn stream(number: u32, generation: u16, body: &str) -> String {
    format!(
        "{number} {generation} obj << /Length {} >> stream\n{body}\nendstream endobj\n",
        body.len()
    )
}

/// 1byteコード41の復号先を変更し、42はBへ固定する。
fn cmap(character: char) -> String {
    format!(
        "1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<41> <{:04X}>\n<42> <0042>\nendbfchar",
        character as u32
    )
}

/// Font参照世代と最新CMap本文を変更できるRootなし1ページPDFを作る。
fn pdf(reference_generation: u16, streams: &str, resources: &str, content: &str) -> Vec<u8> {
    format!("%PDF-1.7\n1 0 obj << /Type /Page /MediaBox [0 0 200 200] /Resources {resources} /Contents 3 0 R >> endobj\n4 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Courier /ToUnicode 11 {reference_generation} R >> endobj\n{}{}%%EOF\n", stream(3, 0, content), streams).into_bytes()
}

/// 追加objectをEOFの前へ配置する。
fn append_objects(bytes: &mut Vec<u8>, objects: &[u8]) {
    bytes.truncate(bytes.len() - b"%%EOF\n".len());
    bytes.extend_from_slice(objects);
    bytes.extend_from_slice(b"%%EOF\n");
}

/// 非ゼロ世代のCMapと同番号更新の最新本文を本文抽出が利用する。
#[test]
fn body_uses_latest_matching_cmap_generation() {
    for (reference, streams, expected) in [
        (7, stream(11, 7, &cmap('文')), "文B"),
        (
            7,
            stream(11, 7, &cmap('旧')) + &stream(11, 7, &cmap('文')),
            "文B",
        ),
        (
            7,
            stream(11, 0, &cmap('旧')) + &stream(11, 7, &cmap('文')),
            "文B",
        ),
        (
            0,
            stream(11, 0, &cmap('旧')) + &stream(11, 7, &cmap('文')),
            "AB",
        ),
        (
            7,
            stream(11, 7, &cmap('旧')) + "11 7 obj null endobj\n",
            "AB",
        ),
    ] {
        let bytes = pdf(
            reference,
            &streams,
            "<< /Font << /F 4 0 R >> >>",
            "BT /F 10 Tf 10 10 Td <4142> Tj ET",
        );
        let parsed = parse_pdf_bytes("latest-cmap.pdf", &bytes).unwrap();
        assert_eq!(
            parsed.text_elements[0].text, expected,
            "reference={reference}, streams={streams}"
        );
    }
}

/// previewもFont参照の世代不一致を拒否する。
#[test]
fn preview_rejects_cmap_generation_mismatch() {
    let bytes = pdf(
        0,
        &stream(11, 7, &cmap('文')),
        "<< /Font << /F 4 0 R >> >>",
        "BT /F 10 Tf 10 10 Td <4142> Tj ET",
    );
    let parsed = parse_pdf_bytes("mismatched-cmap.pdf", &bytes).unwrap();
    assert_eq!(parsed.text_elements[0].text, "AB");
    assert_eq!(parsed.text_preview, "AB");
}

/// Fontのないページへ他objectのCMapを混入させない。
#[test]
fn preview_respects_empty_page_resources() {
    let bytes = pdf(
        7,
        &stream(11, 7, &cmap('文')),
        "<< >>",
        "BT /F 10 Tf 10 10 Td <4142> Tj ET",
    );
    let parsed = parse_pdf_bytes("empty-page-font.pdf", &bytes).unwrap();
    assert_eq!(parsed.text_elements[0].text, "AB");
    assert_eq!(parsed.text_preview, "AB");
}

/// 同じページのTf切替が異なるToUnicode対応を選ぶ。
#[test]
fn preview_follows_font_switches() {
    let mut bytes = pdf(
        7,
        &(stream(11, 7, &cmap('文')) + &stream(12, 3, &cmap('字'))),
        "<< /Font << /F 4 0 R /G 5 0 R >> >>",
        "BT /F 10 Tf 10 10 Td <4142> Tj /G 10 Tf 0 -10 Td <4142> Tj ET",
    );
    append_objects(
        &mut bytes,
        b"5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Courier /ToUnicode 12 3 R >> endobj\n",
    );
    let parsed = parse_pdf_bytes("font-switch.pdf", &bytes).unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|value| value.text.as_str())
            .collect::<Vec<_>>(),
        ["文B", "字B"]
    );
    assert_eq!(parsed.text_preview, "文B\n字B");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// ページごとに同じFont名を再利用しても独立した対応を保持する。
    #[test]
    fn page_local_cmaps_match_independent_model(left in 0x4e00u32..0x4e80, right in 0x4e80u32..0x4f00) {
        let left = char::from_u32(left).unwrap();
        let right = char::from_u32(right).unwrap();
        let mut bytes = pdf(7, &(stream(11, 7, &cmap(left)) + &stream(12, 3, &cmap(right)) + &stream(6, 0, "BT /F 10 Tf 10 10 Td <4142> Tj ET")), "<< /Font << /F 4 0 R >> >>", "BT /F 10 Tf 10 10 Td <4142> Tj ET");
        append_objects(&mut bytes, b"2 0 obj << /Type /Page /MediaBox [0 0 200 200] /Resources << /Font << /F 5 0 R >> >> /Contents 6 0 R >> endobj\n5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Courier /ToUnicode 12 3 R >> endobj\n");
        let parsed = parse_pdf_bytes("local-cmaps.pdf", &bytes).unwrap();
        prop_assert_eq!(parsed.text_elements.len(), 2);
        prop_assert_eq!(&parsed.text_elements[0].text, &format!("{left}B"));
        prop_assert_eq!(&parsed.text_elements[1].text, &format!("{right}B"));
        prop_assert_eq!(parsed.text_preview, format!("{left}B\n{right}B"));
    }
}
