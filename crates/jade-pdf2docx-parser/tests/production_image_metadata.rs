//! 画像辞書の字句境界と補助属性を、画素・資源上限で検証する。
use jade_pdf2docx_parser::{ParseLimits, parse_pdf_bytes, parse_pdf_bytes_with_limits};
use proptest::prelude::*;
use std::io::{Read, Write};

/// 正式な xref と 1 ページを含む PDF を作る。
fn pdf(extra: &[Vec<u8>]) -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_vec(),
    ];
    objects.extend_from_slice(extra);
    let content = b"BT (x) Tj ET q 1 0 0 1 0 0 cm /Im Do Q";
    objects[2] = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Im 4 0 R >> >> /Contents {} 0 R >>", objects.len() + 1).into_bytes();
    objects.push(stream("", content));
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

/// 任意属性の stream を Length とともに生成する。
fn stream(attributes: &str, data: &[u8]) -> Vec<u8> {
    let mut bytes = format!("<< {attributes} /Length {} >>\nstream\n", data.len()).into_bytes();
    bytes.extend_from_slice(data);
    bytes.extend_from_slice(b"\nendstream");
    bytes
}

/// FlateDecode の stream を生成する。
fn flate(attributes: &str, data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::none());
    encoder.write_all(data).unwrap();
    stream(
        &format!("{attributes} /Filter /FlateDecode"),
        &encoder.finish().unwrap(),
    )
}

/// PNG のフィルタなし scanline を展開する。
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

/// 入れ子・文字列・コメント・別キーの Image は Subtype にせず、描画された Form を診断する。
#[test]
fn fake_image_subtype_does_not_create_image() {
    for attributes in [
        "/Note << /Subtype /Image >> /Subtype /Form",
        "/Note (/Subtype /Image) /Subtype /Form",
        "/Note /Image /Subtype /Form",
        "/Subtype /Form % /Subtype /Image\n",
    ] {
        let parsed = parse_pdf_bytes(
            "fake-image.pdf",
            &pdf(&[flate(
                &format!("/BBox [0 0 100 100] {attributes}"),
                &[10, 20, 30],
            )]),
        )
        .unwrap();
        assert!(parsed.image_elements.is_empty(), "{attributes}");
        assert_eq!(
            parsed.quality.issues,
            vec![jade_pdf2docx_parser::QualityIssue::UnsupportedXObject {
                page: 0,
                object_number: 4,
                reason: jade_pdf2docx_parser::UnsupportedXObjectReason::Form,
            }],
            "{attributes}"
        );
    }
}

/// 正規の escaped 属性を持つ RGB 画像を保持する。
#[test]
fn escaped_image_attributes_keep_pixels() {
    let parsed = parse_pdf_bytes("escaped-image.pdf", &pdf(&[flate(
        "/Sub#74ype /Im#61ge /Wid#74h 1 /He#69ght 1 /BitsPerCompo#6Eent 8 /Color#53pace /DeviceR#47B", &[10,20,30]
    )])).unwrap();
    assert_eq!(parsed.image_elements.len(), 1);
    assert_eq!(pixels(&parsed.image_elements[0].png_bytes), [0, 10, 20, 30]);
}

proptest! {
    /// 偽の巨大寸法が root 寸法と上限検査に影響しない。
    #[test]
    fn nested_dimensions_do_not_change_pixel_limit(width in 2u32..100_000, height in 2u32..100_000) {
        let attributes = format!("/Subtype /Image /Note << /Width {width} /Height {height} >> /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB");
        let parsed = parse_pdf_bytes_with_limits("dimensions.pdf", &pdf(&[flate(&attributes, &[10,20,30])]), &ParseLimits { max_image_pixels: 1, ..Default::default() }).unwrap();
        prop_assert_eq!(pixels(&parsed.image_elements[0].png_bytes), vec![0,10,20,30]);
    }
}

/// BPC と色空間は入れ子・文字列より辞書直下の値を優先する。
#[test]
fn nested_component_depth_and_color_space_keep_pixels() {
    for (attributes, data, expected) in [
        (
            "/Note << /BitsPerComponent 1 >> /BitsPerComponent 8 /ColorSpace /DeviceRGB",
            vec![10, 20, 30],
            vec![0, 10, 20, 30],
        ),
        (
            "/Note (/DeviceRGB) /BitsPerComponent 8 /ColorSpace /DeviceGray",
            vec![77],
            vec![0, 77, 77, 77],
        ),
        (
            "/Note << /ColorSpace [/Indexed /DeviceRGB 0 <FFFFFF>] >> /BitsPerComponent 8 /ColorSpace /DeviceRGB",
            vec![0, 20, 30],
            vec![0, 0, 20, 30],
        ),
    ] {
        let parsed = parse_pdf_bytes(
            "components.pdf",
            &pdf(&[flate(
                &format!("/Subtype /Image /Width 1 /Height 1 {attributes}"),
                &data,
            )]),
        )
        .unwrap();
        assert_eq!(
            pixels(&parsed.image_elements[0].png_bytes),
            expected,
            "{attributes}"
        );
    }
}

/// alpha の参照先と mask 寸法も辞書直下だけを読む。
#[test]
fn nested_smask_reference_and_dimensions_keep_alpha() {
    let parsed = parse_pdf_bytes_with_limits("mask.pdf", &pdf(&[
        flate("/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Note << /SMask 5 0 R >> /SMask 6 0 R", &[10,20,30]),
        flate("/Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray", &[11]),
        flate("/Note << /Width 99 /Height 99 >> /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray", &[77]),
    ]), &ParseLimits { max_image_pixels: 1, ..Default::default() }).unwrap();
    assert_eq!(
        pixels(&parsed.image_elements[0].png_bytes),
        [0, 10, 20, 30, 77]
    );
}

/// palette の hex object は metadata 内の偽 header によらず解決する。
#[test]
fn palette_lookup_ignores_fake_objects() {
    for metadata in [
        stream("", b"6 0 obj <FFFFFF> endobj"),
        b"<< /Title (6 0 obj <FFFFFF> endobj) >>".to_vec(),
        b"<< /Title (title) >>\n% 6 0 obj <FFFFFF> endobj\n".to_vec(),
    ] {
        let parsed = parse_pdf_bytes("palette.pdf", &pdf(&[
            flate("/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace [/Indexed /DeviceRGB 0 6 0 R]", &[0]),
            metadata, b"<0A141E>".to_vec(),
        ])).unwrap();
        assert_eq!(pixels(&parsed.image_elements[0].png_bytes), [0, 10, 20, 30]);
    }
}

/// 間接 ColorSpace 配列内の inline palette も読み取る。
#[test]
fn indirect_color_space_keeps_palette_pixels() {
    let parsed = parse_pdf_bytes(
        "indirect-color.pdf",
        &pdf(&[
            flate(
                "/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace 5 0 R",
                &[0],
            ),
            b"[/Indexed /DeviceRGB 0 <0A141E>]".to_vec(),
        ]),
    )
    .unwrap();
    assert_eq!(pixels(&parsed.image_elements[0].png_bytes), [0, 10, 20, 30]);
}

/// lookup string の escape は文字列値として読み取る。
#[test]
fn literal_palette_keeps_escaped_bytes() {
    let parsed = parse_pdf_bytes("literal-palette.pdf", &pdf(&[
        flate("/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace [/Indexed /DeviceRGB 0 (\\012\\024\\036)]", &[0]),
    ])).unwrap();
    assert_eq!(pixels(&parsed.image_elements[0].png_bytes), [0, 10, 20, 30]);
}

/// CCITT は DecodeParms 直下の値だけを使い、直接・間接・単一配列で同じ画素を得る。
#[test]
fn ccitt_uses_scoped_decode_parameters() {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    encoder
        .encode_line([fax::Color::Black; 3].into_iter(), 3)
        .unwrap();
    let mut writer = encoder.finish().unwrap();
    writer.pad();
    let data = writer.finish();
    for black_is_one in [false, true] {
        let params = format!(
            "<< /Note << /Columns 999 /Rows 999 /K 0 /BlackIs1 {} >> /Columns 3 /Rows 1 /K -1 /BlackIs1 {black_is_one} >>",
            !black_is_one
        );
        for value in [
            params.clone(),
            format!("[{params}]"),
            "5 0 R".to_string(),
            "[5 0 R]".to_string(),
        ] {
            let parsed = parse_pdf_bytes_with_limits("ccitt.pdf", &pdf(&[
                stream(&format!("/Subtype /Image /Width 3 /Height 1 /Filter /CCITTFaxDecode /Columns 888 /Rows 888 /BlackIs1 {} /DecodeParms {value}", !black_is_one), &data),
                params.as_bytes().to_vec(),
            ]), &ParseLimits { max_image_pixels: 3, ..Default::default() }).unwrap();
            let gray = if black_is_one { 255 } else { 0 };
            assert_eq!(
                pixels(&parsed.image_elements[0].png_bytes),
                [0, gray, gray, gray, gray, gray, gray, gray, gray, gray],
                "{value}"
            );
        }
    }
}

proptest! {
    /// literal palette は全 byte 値を UTF-8 へ変換せず保持する。
    #[test]
    fn literal_palette_keeps_every_byte(rgb in any::<[u8; 3]>()) {
        let lookup = rgb.iter().map(|byte| format!("\\{byte:03o}")).collect::<String>();
        let parsed = parse_pdf_bytes("literal-bytes.pdf", &pdf(&[flate(
            &format!("/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace [/Indexed /DeviceRGB 0 ({lookup})]"), &[0]
        )])).unwrap();
        prop_assert_eq!(pixels(&parsed.image_elements[0].png_bytes), vec![0,rgb[0],rgb[1],rgb[2]]);
    }
}

/// SMask の filter も escaped name を受け付ける。
#[test]
fn escaped_mask_filter_keeps_alpha() {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::none());
    encoder.write_all(&[77]).unwrap();
    let mask = stream(
        "/Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /Flate#44ecode",
        &encoder.finish().unwrap(),
    );
    let parsed = parse_pdf_bytes("mask-filter.pdf", &pdf(&[
        flate("/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /SMask 5 0 R", &[10,20,30]), mask,
    ])).unwrap();
    assert_eq!(
        pixels(&parsed.image_elements[0].png_bytes),
        [0, 10, 20, 30, 77]
    );
}

/// Subtype を省略した補助 mask でも allocation 前に画素上限を検査する。
#[test]
fn auxiliary_mask_pixels_are_bounded() {
    let error = parse_pdf_bytes_with_limits("mask-limit.pdf", &pdf(&[
        flate("/Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /SMask 5 0 R", &[10,20,30]),
        flate("/Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray", &[77,88]),
    ]), &ParseLimits { max_image_pixels: 1, ..Default::default() }).unwrap_err();
    assert!(matches!(
        error,
        jade_pdf2docx_parser::ParseError::ResourceLimit {
            resource: "image pixels",
            actual: 2,
            limit: 1
        }
    ));
}
