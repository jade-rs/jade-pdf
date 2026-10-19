//! 最新 xref revision に従う通常 object と stream の採用を検証する。
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};
use proptest::prelude::*;
use std::io::{Read, Write};

/// 単一ページと指定 object を持つ base revision と xref 位置を返す。
fn base(extra: &[Vec<u8>]) -> (Vec<u8>, usize) {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_vec(),
    ];
    objects.extend_from_slice(extra);
    if extra
        .first()
        .is_some_and(|body| body.windows(6).any(|word| word == b"/Image"))
    {
        let content_number = objects.len() + 1;
        objects[2] = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Im 4 0 R >> >> /Contents {content_number} 0 R >>").into_bytes();
        objects.push(image_content());
    }
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
    (bytes, xref)
}

/// 通常 object の更新または free entry を Prev で base に連結する。
fn revision(mut bytes: Vec<u8>, prev: usize, number: u32, body: Option<&[u8]>) -> Vec<u8> {
    let offset = bytes.len();
    if let Some(body) = body {
        bytes.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    let entry = if body.is_some() {
        format!("{offset:010} 00000 n \n")
    } else {
        "0000000000 00001 f \n".to_owned()
    };
    bytes.extend_from_slice(format!("xref\n{number} 1\n{entry}trailer << /Size 10 /Root 1 0 R /Prev {prev} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    bytes
}

/// 任意属性を持つ Flate stream を生成する。
fn flate(attributes: &str, data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::none());
    encoder.write_all(data).unwrap();
    let encoded = encoder.finish().unwrap();
    let mut body = format!(
        "<< {attributes} /Filter /FlateDecode /Length {} >>\nstream\n",
        encoded.len()
    )
    .into_bytes();
    body.extend_from_slice(&encoded);
    body.extend_from_slice(b"\nendstream");
    body
}

/// 末尾に書かれた正式な xref の位置を読む。
fn last_xref(bytes: &[u8]) -> usize {
    let start = bytes
        .windows(9)
        .rposition(|word| word == b"startxref")
        .unwrap();
    std::str::from_utf8(&bytes[start + 9..])
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

/// 偽 object を含む古い stream を置換し、その Length を後の revision で更新する。
fn replaced_indirect_stream(updated_length: Option<&[u8]>, initially_correct: bool) -> Vec<u8> {
    let old = b"endstream\nendobj\n99 0 obj << /Type /Page >> endobj\ntrailer << /Encrypt null >>";
    let mut stream = b"<< /Length 5 0 R >>\nstream\n".to_vec();
    stream.extend_from_slice(old);
    stream.extend_from_slice(b"\nendstream");
    let initial_length = if initially_correct { old.len() } else { 1 };
    let (mut bytes, mut xref) = base(&[stream, initial_length.to_string().into_bytes()]);
    if !initially_correct {
        bytes = revision(bytes, xref, 5, Some(old.len().to_string().as_bytes()));
        xref = last_xref(&bytes);
    }
    let bytes = revision(
        bytes,
        xref,
        3,
        Some(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>"),
    );
    let xref = last_xref(&bytes);
    let bytes = revision(
        bytes,
        xref,
        4,
        Some(&flate("", b"BT /F1 12 Tf 10 20 Td (NEW) Tj ET")),
    );
    let xref = last_xref(&bytes);
    revision(bytes, xref, 5, updated_length)
}

/// 古い stream の間接 Length が更新されても、最新本文と正しい page 数を保持する。
#[test]
fn replaced_stream_uses_historical_length_after_length_update() {
    let parsed = parse_pdf_bytes(
        "old-length-updated.pdf",
        &replaced_indirect_stream(Some(b"1"), true),
    )
    .unwrap();
    assert_eq!(parsed.page_count, 1);
    assert!(
        parsed
            .text_elements
            .iter()
            .any(|element| element.text == "NEW")
    );
}

/// 古い stream の間接 Length が削除されても、本文中の偽 object を拾わない。
#[test]
fn replaced_stream_uses_historical_length_after_length_deletion() {
    let parsed = parse_pdf_bytes(
        "old-length-deleted.pdf",
        &replaced_indirect_stream(None, true),
    )
    .unwrap();
    assert_eq!(parsed.page_count, 1);
    assert!(
        parsed
            .text_elements
            .iter()
            .any(|element| element.text == "NEW")
    );
}

/// stream の初出後に修正された Length は、置換される直前の revision から読む。
#[test]
fn replaced_stream_uses_last_live_revision_of_corrected_length() {
    let parsed = parse_pdf_bytes(
        "old-length-corrected.pdf",
        &replaced_indirect_stream(None, false),
    )
    .unwrap();
    assert_eq!(parsed.page_count, 1);
    assert!(
        parsed
            .text_elements
            .iter()
            .any(|element| element.text == "NEW")
    );
}

/// PNG の scanline を展開して画像の色を検証する。
fn pixels(png: &[u8]) -> Vec<u8> {
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

/// 更新前の Page を page 数と MediaBox に採用しない。
#[test]
fn latest_page_revision_replaces_old_page() {
    let (bytes, xref) = base(&[]);
    let bytes = revision(
        bytes,
        xref,
        3,
        Some(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] >>"),
    );
    let parsed = parse_pdf_bytes_with_limits(
        "updated-page.pdf",
        &bytes,
        &ParseLimits {
            max_pages: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(parsed.page_count, 1);
    assert_eq!(parsed.pages.len(), 1);
    assert_eq!(parsed.pages[0].width, 200.0);
    assert_eq!(parsed.pages[0].height, 300.0);
}

/// name と literal が空白なしでも、後続の Page 属性を保持する。
#[test]
fn compact_literal_metadata_preserves_page() {
    let (bytes, xref) = base(&[]);
    let bytes = revision(
        bytes,
        xref,
        3,
        Some(b"<</LastModified(date)/Type/Page/Parent 2 0 R/MediaBox[0 0 200 300]>>"),
    );
    let parsed = parse_pdf_bytes("compact-literal-metadata.pdf", &bytes).unwrap();
    assert_eq!(parsed.page_count, 1);
    assert_eq!(parsed.pages[0].width, 200.0);
}

/// free entry にされた古い Page を数えない。
#[test]
fn deleted_page_is_not_counted() {
    let (bytes, xref) = base(&[]);
    let bytes = revision(bytes, xref, 3, None);
    let prev = last_xref(&bytes);
    let bytes = revision(
        bytes,
        prev,
        2,
        Some(b"<< /Type /Pages /Count 0 /Kids [] >>"),
    );
    let parsed = parse_pdf_bytes("deleted-page.pdf", &bytes).unwrap();
    assert_eq!(parsed.page_count, 0);
    assert!(parsed.pages.is_empty());
}

/// 更新済み Image は最新 stream の画素だけを採用する。
#[test]
fn latest_image_revision_replaces_old_pixels() {
    let attributes =
        "/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB";
    let (bytes, xref) = base(&[flate(attributes, &[1, 2, 3])]);
    let bytes = revision(bytes, xref, 4, Some(&flate(attributes, &[10, 20, 30])));
    let parsed = parse_pdf_bytes("updated-image.pdf", &bytes).unwrap();
    assert_eq!(parsed.image_elements.len(), 1);
    assert_eq!(pixels(&parsed.image_elements[0].png_bytes), [0, 10, 20, 30]);
}

/// 削除済み Image は画素上限と品質診断にも影響させない。
#[test]
fn deleted_image_does_not_consume_pixel_quota() {
    let (bytes, xref) = base(&[flate(
        "/Subtype /Image /Width 100 /Height 100 /BitsPerComponent 8 /ColorSpace /DeviceRGB",
        &[0],
    )]);
    let bytes = revision(bytes, xref, 4, None);
    assert!(matches!(
        parse_pdf_bytes("stale-deleted-image.pdf", &bytes),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
    // 最新 Page から削除済み画像を外す。履歴に残る巨大画像は予算を消費しない。
    let prev = last_xref(&bytes);
    let bytes = revision(
        bytes,
        prev,
        3,
        Some(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> >>"),
    );
    let parsed = parse_pdf_bytes_with_limits(
        "deleted-image.pdf",
        &bytes,
        &ParseLimits {
            max_image_pixels: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(parsed.image_elements.is_empty());
    assert!(parsed.quality.issues.is_empty());
}

proptest! {
    /// 削除 stream の任意 binary 本文は、Length 削除後も PDF 構文として走査しない。
    #[test]
    fn deleted_indirect_stream_binary_data_stays_out_of_structure(payload in proptest::collection::vec(any::<u8>(), 0..512)) {
        let mut stream = b"<< /Length 5 0 R >>\nstream\n".to_vec();
        stream.extend_from_slice(&payload);
        stream.extend_from_slice(b"\nendstream");
        let (bytes, xref) = base(&[stream, payload.len().to_string().into_bytes()]);
        let bytes = revision(bytes, xref, 4, None);
        let xref = last_xref(&bytes);
        let bytes = revision(bytes, xref, 5, None);
        let parsed = parse_pdf_bytes("deleted-indirect-binary.pdf", &bytes).unwrap();
        prop_assert_eq!(parsed.page_count, 1);
        prop_assert!(parsed.text_elements.is_empty());
        prop_assert!(parsed.image_elements.is_empty());
        prop_assert!(parsed.quality.issues.is_empty());
    }

    /// 間接 palette の色は最新 revision で決まり、古い色に依存しない。
    #[test]
    fn latest_palette_revision_replaces_old_color(old in any::<[u8; 3]>(), new in any::<[u8; 3]>()) {
        let palette = |color: [u8; 3]| format!("<{:02x}{:02x}{:02x}>", color[0], color[1], color[2]).into_bytes();
        let (bytes, xref) = base(&[flate("/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace [/Indexed /DeviceRGB 0 5 0 R]", &[0]), palette(old)]);
        let bytes = revision(bytes, xref, 5, Some(&palette(new)));
        let parsed = parse_pdf_bytes("updated-palette.pdf", &bytes).unwrap();
        prop_assert_eq!(pixels(&parsed.image_elements[0].png_bytes), vec![0, new[0], new[1], new[2]]);
    }
}

/// 更新済み Contents は旧 xref の stream offset を使わない。
#[test]
fn latest_content_revision_replaces_old_text() {
    let (bytes, xref) = base(&[flate("", b"BT /F1 12 Tf 10 20 Td (OLD) Tj ET")]);
    let bytes = revision(
        bytes,
        xref,
        3,
        Some(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>"),
    );
    let previous = last_xref(&bytes);
    let bytes = revision(
        bytes,
        previous,
        4,
        Some(&flate("", b"BT /F1 12 Tf 10 20 Td (NEW) Tj ET")),
    );
    let parsed = parse_pdf_bytes("updated-content.pdf", &bytes).unwrap();
    assert!(
        parsed
            .text_elements
            .iter()
            .any(|element| element.text == "NEW")
    );
    assert!(
        !parsed
            .text_elements
            .iter()
            .any(|element| element.text.contains("OLD"))
    );
}

/// 非ゼロ世代の palette を参照できるが、別世代の参照には流用しない。
#[test]
fn palette_reference_matches_generation() {
    for generation in [0, 1] {
        let (bytes, xref) = base(&[
            flate(
                &format!(
                    "/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace [/Indexed /DeviceRGB 0 5 {generation} R]"
                ),
                &[0],
            ),
            b"<010203>".to_vec(),
        ]);
        let mut bytes = revision(bytes, xref, 5, Some(b"<102030>"));
        let header = bytes.windows(7).rposition(|w| w == b"5 0 obj").unwrap();
        bytes[header + 2] = b'1';
        let entry = bytes.windows(8).rposition(|w| w == b"00000 n ").unwrap();
        bytes[entry + 4] = b'1';
        let parsed = parse_pdf_bytes("generation-palette.pdf", &bytes).unwrap();
        let color = pixels(&parsed.image_elements[0].png_bytes);
        if generation == 1 {
            assert_eq!(color, [0, 16, 32, 48]);
        } else {
            assert_ne!(color, [0, 16, 32, 48]);
        }
    }
}

/// Page を ObjStm に置く xref stream の base revision を作る。
fn compressed_base() -> (Vec<u8>, usize) {
    compressed_pages(1)
}

/// 複数の Page を同じ ObjStm に格納した入力を作る。
fn compressed_pages(count: usize) -> (Vec<u8>, usize) {
    let numbers = (0..count)
        .map(|index| if index == 0 { 3 } else { index + 5 })
        .collect::<Vec<_>>();
    let mut header = String::new();
    let mut area = Vec::new();
    for number in &numbers {
        header.push_str(&format!("{number} {} ", area.len()));
        area.extend_from_slice(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >> ");
    }
    let first = header.len();
    let mut payload = header.into_bytes();
    payload.extend_from_slice(&area);
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (number, body) in [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            format!(
                "<< /Type /Pages /Count {count} /Kids [{}] >>",
                numbers
                    .iter()
                    .map(|number| format!("{number} 0 R"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .into_bytes(),
        ),
        (
            4,
            flate(&format!("/Type/ObjStm/N {count}/First {first}"), &payload),
        ),
    ] {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    let mut entries = Vec::new();
    for (kind, value, generation) in [
        (0u8, 0usize, 65535u16),
        (1, offsets[0], 0),
        (1, offsets[1], 0),
        (2, 4, 0),
        (1, offsets[2], 0),
        (1, xref, 0),
    ] {
        entries.push(kind);
        entries.extend_from_slice(&(value as u32).to_be_bytes());
        entries.extend_from_slice(&generation.to_be_bytes());
    }
    for member in 1..count {
        entries.push(2);
        entries.extend_from_slice(&4u32.to_be_bytes());
        entries.extend_from_slice(&(member as u16).to_be_bytes());
    }
    bytes.extend_from_slice(b"5 0 obj\n");
    bytes.extend_from_slice(&flate(
        &format!("/Type /XRef /Size {} /Root 1 0 R /W [1 4 2]", count + 5),
        &entries,
    ));
    bytes.extend_from_slice(format!("\nendobj\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    (bytes, xref)
}

/// 古い圧縮 object は最新 revision の通常 object または free entry に置き換わる。
#[test]
fn compressed_page_obeys_newer_table_revision() {
    for body in [
        Some(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] >>".as_slice()),
        None,
    ] {
        let (bytes, xref) = compressed_base();
        let mut bytes = revision(bytes, xref, 3, body);
        if body.is_none() {
            let prev = last_xref(&bytes);
            bytes = revision(
                bytes,
                prev,
                2,
                Some(b"<< /Type /Pages /Count 0 /Kids [] >>"),
            );
        }
        let parsed = parse_pdf_bytes("updated-compressed-page.pdf", &bytes).unwrap();
        assert_eq!(parsed.page_count, usize::from(body.is_some()));
        if body.is_some() {
            assert_eq!(parsed.pages[0].width, 200.0);
        }
    }
}

/// 空白なしの delimiter を持つ ObjStm 属性でも有効な Page を復元する。
#[test]
fn compact_object_stream_preserves_active_page() {
    let (bytes, _) = compressed_base();
    let parsed = parse_pdf_bytes("compact-object-stream.pdf", &bytes).unwrap();
    assert_eq!(parsed.page_count, 1);
    assert_eq!(parsed.pages[0].width, 100.0);
}

/// xref の世代と header が違う Image stream を有効 object としない。
#[test]
fn image_header_generation_must_match_xref() {
    let (mut bytes, _) = base(&[flate(
        "/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB",
        &[1, 2, 3],
    )]);
    let header = bytes.windows(7).position(|w| w == b"4 0 obj").unwrap();
    bytes[header + 2] = b'1';
    assert!(matches!(
        parse_pdf_bytes("mismatched-image-header.pdf", &bytes),
        Err(ParseError::InvalidPdfStructure { .. })
    ));
}

/// Contents の参照と最新 stream の世代が違う場合に、別世代の本文を採用しない。
#[test]
fn content_reference_does_not_alias_another_generation() {
    for generation in [0, 1] {
        let (bytes, xref) = base(&[flate("", b"BT /F1 12 Tf 10 20 Td (OLD) Tj ET")]);
        let bytes = revision(bytes, xref, 3, Some(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 {generation} R >>").as_bytes()));
        let previous = bytes.windows(9).rposition(|w| w == b"startxref").unwrap();
        let previous: usize = std::str::from_utf8(&bytes[previous + 9..])
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let mut bytes = revision(
            bytes,
            previous,
            4,
            Some(&flate("", b"BT /F1 12 Tf 10 20 Td (NEW) Tj ET")),
        );
        let header = bytes.windows(7).rposition(|w| w == b"4 0 obj").unwrap();
        bytes[header + 2] = b'1';
        let entry = bytes.windows(8).rposition(|w| w == b"00000 n ").unwrap();
        bytes[entry + 4] = b'1';
        let parsed = parse_pdf_bytes("generation-content.pdf", &bytes).unwrap();
        let new = parsed
            .text_elements
            .iter()
            .any(|element| element.text == "NEW");
        assert_eq!(new, generation == 1);
        assert!(
            !parsed
                .text_elements
                .iter()
                .any(|element| element.text.contains("OLD"))
        );
    }
}

/// 同じ ObjStm の全メンバーは、一回の展開量として解析予算に計上する。
#[test]
fn shared_object_stream_is_not_decoded_per_member() {
    let (bytes, _) = compressed_pages(10);
    let parsed = parse_pdf_bytes_with_limits(
        "shared-object-stream.pdf",
        &bytes,
        &ParseLimits {
            max_decoded_stream_bytes: 8192,
            max_total_decoded_bytes: 32768,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(parsed.page_count, 10);
}

/// ColorSpace と palette を同じ ObjStm に格納した画像 PDF を作る。
fn compressed_image(
    color_space: &[u8],
    palette: &[u8],
    generation: u16,
    data: &[u8],
) -> (Vec<u8>, usize) {
    compressed_image_object(
        color_space,
        palette,
        flate(
            &format!(
                "/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace 5 {generation} R"
            ),
            data,
        ),
    )
}

/// 指定 Image と2つの圧縮補助 object を正式な xref stream で登録する。
fn compressed_image_object(color_space: &[u8], palette: &[u8], image: Vec<u8>) -> (Vec<u8>, usize) {
    let header = format!("5 0 6 {} ", color_space.len() + 1);
    let first = header.len();
    let mut payload = header.into_bytes();
    payload.extend_from_slice(color_space);
    payload.push(b' ');
    payload.extend_from_slice(palette);
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = [0; 10];
    for (number, body) in [
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Im 4 0 R >> >> /Contents 9 0 R >>".to_vec(),
        ),
        (4, image),
        (9, image_content()),
        (
            7,
            flate(&format!("/Type /ObjStm /N 2 /First {first}"), &payload),
        ),
    ] {
        offsets[number] = bytes.len();
        bytes.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    offsets[8] = xref;
    let mut entries = Vec::new();
    for (number, offset) in offsets.into_iter().enumerate() {
        let (kind, value, member) = match number {
            0 => (0, 0, 65535u16),
            5 => (2, 7, 0),
            6 => (2, 7, 1),
            _ => (1, offset, 0),
        };
        entries.push(kind);
        entries.extend_from_slice(&(value as u32).to_be_bytes());
        entries.extend_from_slice(&member.to_be_bytes());
    }
    bytes.extend_from_slice(b"8 0 obj\n");
    bytes.extend_from_slice(&flate(
        "/Type /XRef /Size 10 /Root 1 0 R /W [1 4 2]",
        &entries,
    ));
    bytes.extend_from_slice(format!("\nendobj\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    (bytes, xref)
}

/// 圧縮された DeviceRGB の色空間を参照し、白画像への置換を避ける。
#[test]
fn compressed_color_space_preserves_rgb_pixels() {
    let (bytes, _) = compressed_image(b"/DeviceRGB", b"null", 0, &[17, 33, 65]);
    let parsed = parse_pdf_bytes("compressed-color-space.pdf", &bytes).unwrap();
    assert_eq!(pixels(&parsed.image_elements[0].png_bytes), [0, 17, 33, 65]);
    assert_eq!(
        parsed.quality.issues,
        [jade_pdf2docx_parser::QualityIssue::OcrRequired { page: 0 }]
    );
}

/// 圧縮 palette の参照は最新 revision の更新・削除に従う。
#[test]
fn compressed_palette_obeys_latest_revision() {
    for updated in [Some(b"<102030>".as_slice()), None] {
        let (bytes, xref) =
            compressed_image(b"[/Indexed /DeviceRGB 0 6 0 R]", b"<010203>", 0, &[0]);
        let bytes = revision(bytes, xref, 6, updated);
        let parsed = parse_pdf_bytes("compressed-palette-revision.pdf", &bytes).unwrap();
        if updated.is_some() {
            assert_eq!(pixels(&parsed.image_elements[0].png_bytes), [0, 16, 32, 48]);
            assert_eq!(
                parsed.quality.issues,
                [jade_pdf2docx_parser::QualityIssue::OcrRequired { page: 0 }]
            );
        } else {
            assert!(parsed.quality.issues.iter().any(|issue| matches!(
                issue,
                jade_pdf2docx_parser::QualityIssue::ImageReplaced { .. }
            )));
            assert_ne!(pixels(&parsed.image_elements[0].png_bytes), [0, 1, 2, 3]);
        }
    }
}

/// 圧縮 ColorSpace は世代0だけを参照でき、free 更新でも復活しない。
#[test]
fn compressed_color_space_rejects_wrong_generation_and_deletion() {
    for generation in [0, 1] {
        let (bytes, xref) = compressed_image(b"/DeviceRGB", b"null", generation, &[17, 33, 65]);
        let bytes = if generation == 0 {
            revision(bytes, xref, 5, None)
        } else {
            bytes
        };
        let parsed = parse_pdf_bytes("compressed-color-space-invalid.pdf", &bytes).unwrap();
        assert!(parsed.quality.issues.iter().any(|issue| matches!(
            issue,
            jade_pdf2docx_parser::QualityIssue::ImageReplaced { .. }
        )));
        assert_ne!(pixels(&parsed.image_elements[0].png_bytes), [0, 17, 33, 65]);
    }
}

proptest! {
    /// 同じ ObjStm の ColorSpace と任意 palette 色を、member index に従って読む。
    #[test]
    fn compressed_indexed_palette_preserves_color(color in any::<[u8; 3]>()) {
        let palette = format!("<{:02x}{:02x}{:02x}>", color[0], color[1], color[2]);
        let (bytes, _) = compressed_image(b"[/Indexed /DeviceRGB 0 6 0 R]", palette.as_bytes(), 0, &[0]);
        let parsed = parse_pdf_bytes("compressed-indexed-palette.pdf", &bytes).unwrap();
        prop_assert_eq!(pixels(&parsed.image_elements[0].png_bytes), vec![0, color[0], color[1], color[2]]);
        prop_assert_eq!(parsed.quality.issues, vec![jade_pdf2docx_parser::QualityIssue::OcrRequired { page: 0 }]);
    }
}

/// 色空間が通常 object に更新された場合も、古い圧縮 member を再利用しない。
#[test]
fn compressed_color_space_obeys_replacement_and_owner_deletion() {
    for (number, body) in [(5, Some(b"/DeviceGray".as_slice())), (7, None)] {
        let (bytes, xref) = compressed_image(b"/DeviceRGB", b"null", 0, &[17, 33, 65]);
        let parsed = parse_pdf_bytes(
            "compressed-color-space-replaced.pdf",
            &revision(bytes, xref, number, body),
        )
        .unwrap();
        if number == 5 {
            assert_eq!(pixels(&parsed.image_elements[0].png_bytes), [0, 17, 17, 17]);
            assert_eq!(
                parsed.quality.issues,
                [jade_pdf2docx_parser::QualityIssue::OcrRequired { page: 0 }]
            );
        } else {
            assert!(parsed.quality.issues.iter().any(|issue| matches!(
                issue,
                jade_pdf2docx_parser::QualityIssue::ImageReplaced { .. }
            )));
            assert_ne!(pixels(&parsed.image_elements[0].png_bytes), [0, 17, 33, 65]);
        }
    }
}

/// palette の圧縮 member は世代0であり、世代1の参照には使わない。
#[test]
fn compressed_palette_rejects_nonzero_generation() {
    let (bytes, _) = compressed_image(b"[/Indexed /DeviceRGB 0 6 1 R]", b"<010203>", 0, &[0]);
    let parsed = parse_pdf_bytes("compressed-palette-generation.pdf", &bytes).unwrap();
    assert!(parsed.quality.issues.iter().any(|issue| matches!(
        issue,
        jade_pdf2docx_parser::QualityIssue::ImageReplaced { .. }
    )));
    assert_ne!(pixels(&parsed.image_elements[0].png_bytes), [0, 1, 2, 3]);
}

/// 大きい共有 ObjStm をページ・画像の抽出ごとに再展開せず、実展開の上限は保持する。
#[test]
fn compressed_image_metadata_shares_decode_quota() {
    let mut palette = b"<010203>".to_vec();
    palette.extend_from_slice(&[b' '; 1024]);
    let (bytes, _) = compressed_image(b"[/Indexed /DeviceRGB 0 6 0 R]", &palette, 0, &[0]);
    let parsed = parse_pdf_bytes_with_limits(
        "shared-image-metadata.pdf",
        &bytes,
        &ParseLimits {
            max_decoded_stream_bytes: 2048,
            // 未圧縮Contentsの再保存も含むが、1 KiB超のObjStmの二重展開は許可しない。
            max_total_decoded_bytes: 1536,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(pixels(&parsed.image_elements[0].png_bytes), [0, 1, 2, 3]);
    for limits in [
        ParseLimits {
            max_decoded_stream_bytes: 1024,
            ..Default::default()
        },
        ParseLimits {
            max_total_decoded_bytes: 1024,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            parse_pdf_bytes_with_limits("metadata-over-limit.pdf", &bytes, &limits),
            Err(jade_pdf2docx_parser::ParseError::ResourceLimit { .. })
        ));
    }
    let (other, _) = compressed_image(b"[/Indexed /DeviceRGB 0 6 0 R]", b"<102030>", 0, &[0]);
    let parsed = parse_pdf_bytes("metadata-next-parse.pdf", &other).unwrap();
    assert_eq!(pixels(&parsed.image_elements[0].png_bytes), [0, 16, 32, 48]);
}

/// 圧縮 DecodeParms の辞書・直接配列・間接配列は同じ CCITT 画素を返す。
#[test]
fn compressed_ccitt_parameters_keep_array_ownership() {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    encoder
        .encode_line([fax::Color::Black; 3].into_iter(), 3)
        .unwrap();
    let mut writer = encoder.finish().unwrap();
    writer.pad();
    let data = writer.finish();
    for black_is_one in [false, true] {
        let params = format!("<< /Columns 3 /Rows 1 /K -1 /BlackIs1 {black_is_one} >>");
        for member in [params.clone(), format!("[{params}]"), "[6 0 R]".to_owned()] {
            let mut image = format!("<< /Subtype /Image /Width 3 /Height 1 /Filter /CCITTFaxDecode /DecodeParms 5 0 R /Length {} >>\nstream\n", data.len()).into_bytes();
            image.extend_from_slice(&data);
            image.extend_from_slice(b"\nendstream");
            let (bytes, _) = compressed_image_object(member.as_bytes(), params.as_bytes(), image);
            let parsed = parse_pdf_bytes("compressed-ccitt-parameters.pdf", &bytes).unwrap();
            let gray = if black_is_one { 255 } else { 0 };
            assert_eq!(
                pixels(&parsed.image_elements[0].png_bytes),
                [0, gray, gray, gray, gray, gray, gray, gray, gray, gray],
                "{member}"
            );
            assert_eq!(
                parsed.quality.issues,
                [jade_pdf2docx_parser::QualityIssue::OcrRequired { page: 0 }]
            );
        }
    }
}

/// Indexed の base 色空間も圧縮 object の参照として解決する。
#[test]
fn indexed_base_color_space_resolves_compressed_reference() {
    let (bytes, _) = compressed_image(b"[/Indexed 6 0 R 0 <010203>]", b"/DeviceRGB", 0, &[0]);
    let parsed = parse_pdf_bytes("compressed-indexed-base.pdf", &bytes).unwrap();
    assert_eq!(pixels(&parsed.image_elements[0].png_bytes), [0, 1, 2, 3]);
    assert_eq!(
        parsed.quality.issues,
        [jade_pdf2docx_parser::QualityIssue::OcrRequired { page: 0 }]
    );
}

/// 画像 decoder の fixture で明示的に Image を描画し OCR 診断と分離する。
fn image_content() -> Vec<u8> {
    // 文字レイヤを画像へ重ね、圧縮objectの検査をOCR対象判定から分離する。
    let content = "BT /F1 12 Tf 0 .5 Td (x) Tj ET q 1 0 0 1 0 0 cm /Im Do Q";
    format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
    .into_bytes()
}
