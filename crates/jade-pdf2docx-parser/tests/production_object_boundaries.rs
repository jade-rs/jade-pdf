//! binary payload 内の object らしい byte がページ・font・CMap を変更しない回帰。
use jade_pdf2docx_parser::{ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};
use proptest::prelude::*;

/// 正式な xref table を持つ object 列を生成する。
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
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

/// Length で本文を区切る stream を作る。
fn stream(payload: &[u8]) -> Vec<u8> {
    let mut bytes = format!("<< /Length {} >>\nstream\n", payload.len()).into_bytes();
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(b"\nendstream");
    bytes
}

/// 1ページと Helvetica、無関係な binary stream を持つ PDF を作る。
fn text_pdf(payload: &[u8]) -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream(b"BT /F1 12 Tf 10 20 Td <4142> Tj ET"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        stream(payload),
    ])
}

/// 参照される metadata に文字列またはコメントとして偽 object を入れる。
fn metadata_pdf(metadata: &[u8]) -> Vec<u8> {
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R /Info 6 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream(b"BT /F1 12 Tf 10 20 Td <4142> Tj ET"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        metadata.to_vec(),
    ])
}

/// literal string 内の偽 Page/Font/CMap と endobj は構造として扱わない。
#[test]
fn literal_metadata_does_not_change_pages_fonts_or_text() {
    let metadata = b"<< /Title (outer \\( escaped \\) (nested) /Type /Page /MediaBox [0 0 999 999] endobj 5 0 obj << /Type /Font /BaseFont /Courier /Encoding /Identity-H /ToUnicode 6 0 R >> endobj 2 beginbfchar <41> <0058> <42> <0059> endbfchar) >>";
    let parsed = parse_pdf_bytes_with_limits(
        "literal.pdf",
        &metadata_pdf(metadata),
        &ParseLimits {
            max_pages: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(parsed.page_count, 1);
    assert_eq!(parsed.pages.len(), 1);
    assert_eq!(parsed.pages[0].width, 200.0);
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(
        parsed.text_elements[0].font_name.as_deref(),
        Some("Helvetica")
    );
    assert_eq!(parsed.text_elements[0].text, "AB");
    assert_eq!(parsed.text_preview, "AB");
}

/// コメント内の偽 object/CMap は文字対応と font を変更しない。
#[test]
fn comments_do_not_override_font_or_tounicode() {
    let metadata = b"<< /Title (title) >>\n% endobj 5 0 obj << /Type /Font /BaseFont /Courier /Encoding /Identity-H /ToUnicode 6 0 R >> endobj\n% 2 beginbfchar <41> <0058> <42> <0059> endbfchar\n";
    let parsed = parse_pdf_bytes("comment.pdf", &metadata_pdf(metadata)).unwrap();
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "AB");
    assert_eq!(
        parsed.text_elements[0].font_name.as_deref(),
        Some("Helvetica")
    );
    assert_eq!(parsed.text_preview, "AB");
}

/// 型名と実際の辞書値の間にコメントがあっても実ページを認識する。
#[test]
fn real_page_with_comments_and_nested_fake_type_has_one_page() {
    let parsed = parse_pdf_bytes_with_limits("type.pdf", &pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Note << /Type /Page /MediaBox [0 0 999 999] >> /Type % /Page /MediaBox [0 0 888 888]\n/Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>".to_vec(),
        stream(b"BT /F1 12 Tf 10 20 Td (AB) Tj ET"),
    ]), &ParseLimits { max_pages: 1, ..Default::default() }).unwrap();
    assert_eq!(parsed.page_count, 1);
    assert_eq!(parsed.pages.len(), 1);
    assert_eq!(parsed.pages[0].width, 200.0);
    assert_eq!(parsed.text_elements[0].page, 0);
    assert_eq!(parsed.text_elements[0].text, "AB");
}

/// metadata の literal string に content operator があっても本文に混ぜない。
#[test]
fn literal_metadata_text_does_not_enter_preview() {
    let parsed = parse_pdf_bytes(
        "preview.pdf",
        &metadata_pdf(b"<< /Title (BT (EVIL) Tj ET) >>"),
    )
    .unwrap();
    assert_eq!(parsed.text_preview, "AB");
    assert_eq!(parsed.text_elements[0].text, "AB");
}

/// 入れ子の Parent が実際のページ寸法・回転の継承先を変更しない。
#[test]
fn nested_parent_does_not_override_page_inheritance() {
    let parsed = parse_pdf_bytes(
        "parent.pdf",
        &pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 200 200] /Rotate 90 >>"
                .to_vec(),
            b"<< /Note << /Parent 5 0 R >> /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
            stream(b"BT /F1 12 Tf 10 20 Td (AB) Tj ET"),
            b"<< /MediaBox [0 0 999 999] /Rotate 180 >>".to_vec(),
        ]),
    )
    .unwrap();
    assert_eq!(parsed.pages[0].width, 200.0);
    assert_eq!(parsed.pages[0].rotation, 90);
    assert_eq!(parsed.text_elements[0].text, "AB");
}

/// 入れ子の Resources が最外辞書のフォント定義を上書きしない。
#[test]
fn nested_resources_do_not_override_page_fonts() {
    let parsed = parse_pdf_bytes("resources.pdf", &pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Note << /Resources << /Font << /F1 6 0 R >> >> >> /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream(b"BT /F1 12 Tf 10 20 Td <4142> Tj ET"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_vec(),
    ])).unwrap();
    assert_eq!(
        parsed.text_elements[0].font_name.as_deref(),
        Some("Helvetica")
    );
    assert_eq!(parsed.text_elements[0].text, "AB");
}

/// 入れ子の font 属性が実フォント名・encoding・ToUnicode を変更しない。
#[test]
fn nested_font_attributes_do_not_override_real_font() {
    let parsed = parse_pdf_bytes("font-keys.pdf", &pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream(b"BT /F1 12 Tf 10 20 Td <4142> Tj ET"),
        b"<< /Note << /Type /Font /BaseFont /Courier /Encoding /Identity-H /ToUnicode 7 0 R >> /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /ToUnicode 6 0 R >>".to_vec(),
        stream(b"2 beginbfchar <41> <0041> <42> <0042> endbfchar"),
        stream(b"2 beginbfchar <41> <0058> <42> <0059> endbfchar"),
    ])).unwrap();
    assert_eq!(
        parsed.text_elements[0].font_name.as_deref(),
        Some("Helvetica")
    );
    assert_eq!(parsed.text_elements[0].text, "AB");
}

/// font の型とキーの name escape・コメントを通常の字句として扱う。
#[test]
fn escaped_font_type_and_keys_keep_font_and_text() {
    let parsed = parse_pdf_bytes("font-escaped.pdf", &pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream(b"BT /F1 12 Tf 10 20 Td <4142> Tj ET"),
        b"<< /Ty#70e % real font
/Fo#6Et /Subtype /Type1 /Base#46ont /Helv#65tica >>".to_vec(),
    ])).unwrap();
    assert_eq!(
        parsed.text_elements[0].font_name.as_deref(),
        Some("Helvetica")
    );
    assert_eq!(parsed.text_elements[0].text, "AB");
}

/// literal とコメント内の偽 ColorSpace object を間接参照として使わない。
#[test]
fn fake_color_space_in_metadata_does_not_discard_alpha() {
    for metadata in [
        b"<< /Title (7 0 obj /DeviceRGB endobj) >>".as_slice(),
        b"<< /Title (title) >>
% 7 0 obj /DeviceRGB endobj
",
    ] {
        let parsed = parse_pdf_bytes("mask-metadata.pdf", &pdf(&[
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 5 0 R >> >> /Contents 8 0 R >>".to_vec(),
            metadata.to_vec(),
            flate_stream("/Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /SMask 6 0 R", &[10,20,30]),
            flate_stream("/Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace 7 0 R", &[77]),
            b"/DeviceGray".to_vec(),
            stream(b"q 1 0 0 1 10 10 cm /Im1 Do Q"),
        ])).unwrap();
        assert_eq!(
            png_scanline(&parsed.image_elements[0].png_bytes),
            [0, 10, 20, 30, 77]
        );
    }
}

proptest! {
    /// 任意枚数の偽ページを含むコメント・literal string で実ページを保持する。
    #[test]
    fn metadata_page_markers_do_not_change_pages(count in 1usize..12, comment in any::<bool>()) {
        let fake = "/Type /Page /MediaBox [0 0 999 999] ".repeat(count);
        let metadata = if comment { format!("<< /Title (title) >>\n% {fake}\n") } else { format!("<< /Title ({fake}) >>") };
        let parsed = parse_pdf_bytes_with_limits("metadata.pdf", &metadata_pdf(metadata.as_bytes()), &ParseLimits { max_pages: 1, ..Default::default() }).unwrap();
        prop_assert_eq!(parsed.page_count, 1);
        prop_assert_eq!(parsed.pages.len(), 1);
        prop_assert_eq!(parsed.pages[0].width, 200.0);
        prop_assert_eq!(&parsed.text_elements[0].text, "AB");
    }
}

/// 偽 Page はページ数上限も実ページ情報も変えない。
#[test]
fn fake_page_in_payload_does_not_change_pages_or_trigger_limit() {
    let payload = b"endstream\nendobj\n99 0 obj << /Type /Page /MediaBox [0 0 999 999] >> endobj";
    let limits = ParseLimits {
        max_pages: 1,
        ..Default::default()
    };
    let parsed = parse_pdf_bytes_with_limits("page.pdf", &text_pdf(payload), &limits).unwrap();
    assert_eq!(parsed.page_count, 1);
    assert_eq!(parsed.pages.len(), 1);
    assert_eq!(parsed.pages[0].width, 200.0);
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].page, 0);
}

/// binary の偽 Font object は正規 font の名前と文字対応を上書きしない。
#[test]
fn fake_font_in_payload_does_not_override_real_font() {
    let payload = b"endstream\nendobj\n5 0 obj << /Type /Font /Subtype /Type0 /BaseFont /Courier /Encoding /Identity-H >> endobj\n<< /Font << /F1 5 0 R >> >>";
    let parsed = parse_pdf_bytes("font.pdf", &text_pdf(payload)).unwrap();
    assert_eq!(parsed.text_elements.len(), 1);
    assert_eq!(parsed.text_elements[0].text, "AB");
    assert_eq!(
        parsed.text_elements[0].font_name.as_deref(),
        Some("Helvetica")
    );
}

/// 無関係な payload の CMap と ToUnicode 参照は本文の文字を変えない。
#[test]
fn fake_tounicode_in_payload_does_not_map_text() {
    let payload = b"endstream\nendobj\n99 0 obj << /Type /Font /ToUnicode 6 0 R >> endobj\n2 beginbfchar\n<41> <0058>\n<42> <0059>\nendbfchar\n";
    let parsed = parse_pdf_bytes("cmap.pdf", &text_pdf(payload)).unwrap();
    assert_eq!(parsed.text_elements[0].text, "AB");
    assert_eq!(parsed.text_preview, "AB");
}

/// FontFile2 が存在しても binary 内の偽 Descriptor から埋め込まない。
#[test]
fn fake_descriptor_in_payload_does_not_embed_font() {
    let mut font = vec![0u8; 38];
    font[..4].copy_from_slice(b"\0\x01\0\0");
    font[4..6].copy_from_slice(&1u16.to_be_bytes());
    font[12..16].copy_from_slice(b"OS/2");
    font[20..24].copy_from_slice(&28u32.to_be_bytes());
    font[24..28].copy_from_slice(&10u32.to_be_bytes());
    let parsed = parse_pdf_bytes("descriptor.pdf", &pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_vec(),
        stream(b"endstream\nendobj\n99 0 obj << /Type /FontDescriptor /FontName /Fake /FontFile2 5 0 R >> endobj"),
        stream(&font),
    ])).unwrap();
    assert!(parsed.embedded_fonts.is_empty());
}

/// Descriptor の Type は入れ子にあると埋め込み候補にしない。
#[test]
fn nested_descriptor_type_does_not_embed_font() {
    let parsed = parse_pdf_bytes(
        "nested-descriptor.pdf",
        &descriptor_pdf(
            b"<< /Note << /Type /FontDescriptor >> /FontName /Fake /FontFile2 5 0 R >>",
        ),
    )
    .unwrap();
    assert!(parsed.embedded_fonts.is_empty());
}

/// escape とコメントがある実 Descriptor も埋め込み対象になる。
#[test]
fn escaped_descriptor_type_keeps_embedded_font() {
    let parsed = parse_pdf_bytes(
        "escaped-descriptor.pdf",
        &descriptor_pdf(
            b"<< /Ty#70e % descriptor\n/FontDescr#69ptor /FontName /Real /FontFile2 5 0 R >>",
        ),
    )
    .unwrap();
    assert_eq!(parsed.embedded_fonts.len(), 1);
    assert_eq!(parsed.embedded_fonts[0].font_name, "Real");
}

/// Unicode の対応を復元できない PDF subset は DOCX に再埋め込みしない。
#[test]
fn unmapped_subset_font_is_not_embedded_even_when_permission_allows_it() {
    let parsed = parse_pdf_bytes(
        "subset-descriptor.pdf",
        &descriptor_pdf(b"<< /Type /FontDescriptor /FontName /ABCDEF+ArialMT /FontFile2 5 0 R >>"),
    )
    .unwrap();
    assert!(parsed.embedded_fonts.is_empty());
}

/// 埋め込み許可付きの最小 font と Descriptor 候補を持つ PDF を作る。
fn descriptor_pdf(dictionary: &[u8]) -> Vec<u8> {
    let mut font = vec![0u8; 38];
    font[..4].copy_from_slice(b"\0\x01\0\0");
    font[4..6].copy_from_slice(&1u16.to_be_bytes());
    font[12..16].copy_from_slice(b"OS/2");
    font[20..24].copy_from_slice(&28u32.to_be_bytes());
    font[24..28].copy_from_slice(&10u32.to_be_bytes());
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_vec(),
        dictionary.to_vec(),
        stream(&font),
    ])
}

proptest! {
    /// 偽ページの枚数・寸法が変わっても実際の1ページを保持する。
    #[test]
    fn payload_page_markers_do_not_change_page_metadata(count in 1usize..16, size in 1u32..10000) {
        let mut payload = b"endstream\nendobj\n".to_vec();
        for number in 100..100+count {
            payload.extend_from_slice(format!("{number} 0 obj << /Type /Page /MediaBox [0 0 {size} {size}] >> endobj\n").as_bytes());
        }
        let parsed = parse_pdf_bytes("pages.pdf", &text_pdf(&payload)).unwrap();
        prop_assert_eq!(parsed.page_count, 1);
        prop_assert_eq!(parsed.pages.len(), 1);
        prop_assert_eq!(parsed.pages[0].width, 200.0);
        prop_assert_eq!(&parsed.text_elements[0].text, "AB");
    }
}

/// 辞書に圧縮本文を付け、Length は圧縮後の byte 数に合わせる。
fn flate_stream(dictionary: &str, payload: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::none());
    encoder.write_all(payload).unwrap();
    let data = encoder.finish().unwrap();
    let mut body = format!(
        "<< {dictionary} /Filter /FlateDecode /Length {} >>\nstream\n",
        data.len()
    )
    .into_bytes();
    body.extend_from_slice(&data);
    body.extend_from_slice(b"\nendstream");
    body
}

/// parser が生成した PNG の無加工 scanline を読む。
fn png_scanline(png: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut cursor = 8;
    let mut idat = Vec::new();
    while cursor + 12 <= png.len() {
        let length = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
        if &png[cursor + 4..cursor + 8] == b"IDAT" {
            idat.extend_from_slice(&png[cursor + 8..cursor + 8 + length]);
        }
        cursor += length + 12;
    }
    let mut decoded = Vec::new();
    flate2::read::ZlibDecoder::new(idat.as_slice())
        .read_to_end(&mut decoded)
        .unwrap();
    decoded
}

/// 先行する binary 内の偽 ColorSpace ではなく正規の間接 object を使う。
#[test]
fn fake_color_space_in_payload_does_not_discard_alpha() {
    let parsed = parse_pdf_bytes("mask.pdf", &pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 5 0 R >> >> /Contents 8 0 R >>".to_vec(),
        stream(b"endstream\nendobj\n7 0 obj /DeviceRGB endobj"),
        flate_stream("/Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /SMask 6 0 R", &[10,20,30]),
        flate_stream("/Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace 7 0 R", &[77]),
        b"/DeviceGray".to_vec(),
        stream(b"q 1 0 0 1 10 10 cm /Im1 Do Q"),
    ])).unwrap();
    assert_eq!(parsed.image_elements.len(), 1);
    assert_eq!(
        png_scanline(&parsed.image_elements[0].png_bytes),
        [0, 10, 20, 30, 77]
    );
}
