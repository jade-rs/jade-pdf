//! 所属ページの Font/XObject 個別参照の型と抽出結果を検証する。
use jade_pdf2docx_parser::{ParseError, parse_pdf_bytes};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// Font の resource map は辞書であり、不正な間接参照を空の辞書にしない。
#[test]
fn invalid_font_dictionary_is_rejected() {
    for value in ["99 0 R", "9 1 R", "[0]", "123", "9 0 R"] {
        let mut objects = pages(&format!(
            "/MediaBox [0 0 100 100] /Resources << /Font {value} >>"
        ));
        objects[8] = "9 0 R".into();
        assert!(
            matches!(
                parse_pdf_bytes("font-map.pdf", &pdf(&objects)),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "{value}"
        );
    }
}

/// 個別 resource の参照切れ・別世代・循環を Font と XObject の両方で検査する。
#[test]
fn invalid_individual_resource_reference_is_rejected() {
    for category in ["Font", "XObject"] {
        for value in ["99 0 R", "10 1 R", "10 0 R"] {
            let mut objects = pages(&format!(
                "/MediaBox [0 0 100 100] /Resources << /{category} << /Item {value} >> >>"
            ));
            objects.push("10 0 R".into());
            assert!(
                matches!(
                    parse_pdf_bytes("resource-reference.pdf", &pdf(&objects)),
                    Err(ParseError::InvalidPdfStructure { .. })
                ),
                "{category}: {value}"
            );
        }
    }
}

/// Font は Type Font と名前の Subtype を持つ通常辞書である。
#[test]
fn malformed_individual_font_is_rejected() {
    for value in [
        "123",
        "[0]",
        "<< >>",
        "<< /Type /Page /Subtype /Type1 >>",
        "<< /Type /Font >>",
        "<< /Type /Font /Subtype 1 >>",
        "<< /Type /Font /Subtype /Type1 /Length 0 >>\nstream\n\nendstream",
    ] {
        let mut objects = pages("/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> >>");
        objects.push(value.into());
        assert!(
            matches!(
                parse_pdf_bytes("font-type.pdf", &pdf(&objects)),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "{value}"
        );
    }
}

/// XObject は必須 Subtype を持つ stream であり、Type の存在時は XObject に一致する。
#[test]
fn malformed_individual_xobject_is_rejected() {
    for value in [
        "123",
        "[0]",
        "<< /Type /XObject /Subtype /Image >>",
        "<< /Type /Page /Subtype /Form /Length 0 >>\nstream\n\nendstream",
        "<< /Type /XObject /Length 0 >>\nstream\n\nendstream",
        "<< /Subtype 1 /Length 0 >>\nstream\n\nendstream",
    ] {
        let mut objects =
            pages("/MediaBox [0 0 100 100] /Resources << /XObject << /Im 10 0 R >> >>");
        objects.push(value.into());
        assert!(
            matches!(
                parse_pdf_bytes("xobject-type.pdf", &pdf(&objects)),
                Err(ParseError::InvalidPdfStructure { .. })
            ),
            "{value}"
        );
    }
}

/// XObject の個別参照 chain でも描画先は最終 stream になる。
#[test]
fn chained_individual_xobject_preserves_images() {
    let mut objects = pages("/MediaBox [0 0 100 100] /Resources << /XObject << /Im 10 0 R >> >>");
    objects.push("11 0 R".into());
    objects.push("7 0 R".into());
    let parsed = parse_pdf_bytes("image-alias.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.image_elements.len(), 2);
}

/// Font map と個別 Font の参照 chain から最終フォント名を保持する。
#[test]
fn chained_font_resources_preserve_text_font() {
    for font_map in ["<< /F 10 0 R >>", "12 0 R"] {
        let mut objects = pages(&format!(
            "/MediaBox [0 0 100 100] /Resources << /Font {font_map} >>"
        ));
        objects[4] = stream("BT /F 12 Tf 10 20 Td (Hello) Tj ET");
        objects[5] = stream("BT /F 12 Tf 10 20 Td (World) Tj ET");
        objects.push("11 0 R".into());
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
        objects.push("13 0 R".into());
        objects.push("<< /F 10 0 R >>".into());
        let parsed = parse_pdf_bytes("font-alias.pdf", &pdf(&objects)).unwrap();
        assert_eq!(parsed.text_elements.len(), 2);
        assert!(
            parsed
                .text_elements
                .iter()
                .all(|text| text.font_name.as_deref() == Some("Helvetica")),
            "{font_map}"
        );
    }
}

/// null の resource エントリは不在として扱い、画像 stream の Type 省略も許す。
#[test]
fn optional_xobject_type_and_null_resources_are_accepted() {
    for type_entry in ["", "/Type null", "/Type /XObject"] {
        let mut objects = pages(
            "/MediaBox [0 0 100 100] /Resources << /Font null /XObject << /Unused null /Im 7 0 R >> >>",
        );
        objects[6] = objects[6].replace("/Type /XObject", type_entry);
        let parsed = parse_pdf_bytes("optional-image-type.pdf", &pdf(&objects)).unwrap();
        assert_eq!(parsed.image_elements.len(), 2);
    }
}

/// Font/XObject 個別参照にも inclusive な深度上限を適用する。
#[test]
fn individual_resource_reference_depth_is_bounded() {
    use jade_pdf2docx_parser::{ParseLimits, parse_pdf_bytes_with_limits};
    for category in ["Font", "XObject"] {
        let mut objects = pages(&format!(
            "/MediaBox [0 0 100 100] /Resources << /{category} << /Item 10 0 R >> >>"
        ));
        objects.extend([
            "11 0 R".into(),
            "12 0 R".into(),
            "13 0 R".into(),
            "14 0 R".into(),
            if category == "Font" {
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into()
            } else {
                objects[6].clone()
            },
        ]);
        let bytes = pdf(&objects);
        let mut limits = ParseLimits {
            max_container_nesting: 4,
            ..Default::default()
        };
        assert!(matches!(
            parse_pdf_bytes_with_limits("resource-depth.pdf", &bytes, &limits),
            Err(ParseError::ResourceLimit {
                resource: "page attribute depth",
                actual: 5,
                limit: 4
            })
        ));
        limits.max_container_nesting = 5;
        assert_eq!(
            parse_pdf_bytes_with_limits("resource-depth.pdf", &bytes, &limits)
                .unwrap()
                .pages
                .len(),
            2
        );
    }
}

/// 直接 Font と間接 Type/Subtype の合法な組み合わせを保持する。
#[test]
fn inline_font_and_indirect_resource_type_names_are_accepted() {
    let mut objects = pages(
        "/MediaBox [0 0 100 100] /Resources << /Font << /F << /Type 10 0 R /Subtype 11 0 R /BaseFont /Helvetica >> >> /XObject << /Im 7 0 R >> >>",
    );
    objects[4] = stream("BT /F 12 Tf 10 20 Td (Hello) Tj ET");
    objects.push("/Font".into());
    objects.push("/Type1".into());
    let parsed = parse_pdf_bytes("inline-resource.pdf", &pdf(&objects)).unwrap();
    assert_eq!(
        parsed.text_elements[0].font_name.as_deref(),
        Some("Helvetica")
    );
    assert_eq!(parsed.image_elements.len(), 1);
}

proptest! {
    /// 個別 XObject の任意の alias 長でも正常世代の最終画像を失わない。
    #[test]
    fn xobject_alias_chains_preserve_placements(depth in 1u32..12) {
        let mut objects = pages("/MediaBox [0 0 100 100] /Resources << /XObject << /Im 10 0 R >> >>");
        for offset in 0..depth {
            objects.push(if offset + 1 == depth { "7 0 R".into() } else {
                format!("{} 0 R", 11 + offset)
            });
        }
        let parsed = parse_pdf_bytes("alias-property.pdf", &pdf(&objects)).unwrap();
        prop_assert_eq!(parsed.image_elements.len(), 2);
    }

    /// chain の任意の位置で generation が異なれば Font/XObject の両方で拒否する。
    #[test]
    fn resource_alias_generation_mismatch_is_rejected(depth in 1u32..12, fault in 0u32..12, font in any::<bool>()) {
        let category = if font { "Font" } else { "XObject" };
        let mut objects = pages(&format!("/MediaBox [0 0 100 100] /Resources << /{category} << /Item 10 0 R >> >>"));
        for offset in 0..depth {
            let number = if offset + 1 == depth { if font { 10 + depth } else { 7 } } else { 11 + offset };
            let generation = u16::from(offset == fault % depth);
            objects.push(format!("{number} {generation} R"));
        }
        if font {
            objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
        }
        let result = parse_pdf_bytes("alias-generation-property.pdf", &pdf(&objects));
        prop_assert!(matches!(result, Err(ParseError::InvalidPdfStructure { .. })), "wrong generation must be rejected");
    }
}

/// 辞書と Do の異なる name escape 表記を同一 resource として照合する。
#[test]
fn escaped_xobject_resource_names_preserve_images() {
    for (key, operand) in [
        ("#49m", "Im"),
        ("Im", "I#6D"),
        ("I#FF", "I#ff"),
        ("I#23m", "I#23#6D"),
        ("I#20m", "I#20#6D"),
    ] {
        let mut objects = pages(&format!(
            "/MediaBox [0 0 100 100] /Resources << /XObject << /{key} 7 0 R >> >>"
        ));
        objects[4] = stream(&format!("q 10 0 0 10 0 0 cm /{operand} Do Q"));
        objects[5] = objects[4].clone();
        let parsed = parse_pdf_bytes("escaped-image-resource.pdf", &pdf(&objects)).unwrap();
        assert_eq!(parsed.image_elements.len(), 2, "{key} / {operand}");
        assert_eq!(parsed.image_elements[0].bounds.x1, 10.0);
    }
}

/// Font 辞書と Tf の異なる escape 表記でもフォント名を保持する。
#[test]
fn escaped_font_resource_names_preserve_font_name() {
    for (key, operand) in [
        ("#46", "F"),
        ("F", "#46"),
        ("F#FF", "F#ff"),
        ("F#20x", "F#20#78"),
    ] {
        let mut objects = pages(&format!(
            "/MediaBox [0 0 100 100] /Resources << /Font << /{key} 10 0 R >> >>"
        ));
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
        objects[4] = stream(&format!("BT /{operand} 12 Tf 10 20 Td (Hello) Tj ET"));
        objects[5] = objects[4].clone();
        let parsed = parse_pdf_bytes("escaped-font-resource.pdf", &pdf(&objects)).unwrap();
        assert_eq!(parsed.text_elements.len(), 2);
        assert!(
            parsed
                .text_elements
                .iter()
                .all(|text| text.font_name.as_deref() == Some("Helvetica")),
            "{key} / {operand}"
        );
    }
}

/// 間接 Type の Font でも BaseFont と ToUnicode による本文を保持する。
#[test]
fn indirect_font_type_preserves_name_and_unicode_text() {
    let mut objects = pages("/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> >>");
    objects
        .push("<< /Type 11 0 R /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 12 0 R >>".into());
    objects.push("13 0 R".into());
    objects.push(stream(
        "1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<41> <3042>\nendbfchar\n",
    ));
    objects.push("/Font".into());
    objects[4] = stream("BT /F 12 Tf 10 20 Td <41> Tj ET");
    objects[5] = objects[4].clone();
    let parsed = parse_pdf_bytes("indirect-font-type.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.text_elements.len(), 2);
    assert_eq!(
        parsed.text_elements[0].font_name.as_deref(),
        Some("Helvetica")
    );
    assert!(parsed.text_elements.iter().all(|text| text.text == "あ"));
}

/// escape を含む Tf は ToUnicode を選び、literal と hex の両方を復元する。
#[test]
fn escaped_font_selection_preserves_tounicode_text() {
    let mut objects =
        pages("/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R /G 12 0 R >> >>");
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 11 0 R >>".into());
    objects.push(stream(
        "1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<41> <3042>\nendbfchar\n",
    ));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier /ToUnicode 13 0 R >>".into());
    objects.push(stream("1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<41> <3044>\nendbfchar\n"));
    objects[4] = stream("BT /#46 12 Tf 10 20 Td (A) Tj ET");
    objects[5] = stream("BT /#46 12 Tf 10 20 Td <41> Tj ET");
    let parsed = parse_pdf_bytes("escaped-font-cmap.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.text_elements.len(), 2);
    assert!(parsed.text_elements.iter().all(|text| text.text == "あ"));
}

/// 同じ表示文字に見える byte 列でも別 resource の Font を混同しない。
#[test]
fn distinct_resource_name_bytes_do_not_collide() {
    let mut objects =
        pages("/MediaBox [0 0 100 100] /Resources << /Font << /F#E9 10 0 R /F#C3#A9 11 0 R >> >>");
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into());
    objects[4] = stream("BT /F#e9 12 Tf 10 20 Td (Hello) Tj ET");
    objects[5] = stream("BT /F#c3#a9 12 Tf 10 20 Td (Hello) Tj ET");
    let parsed = parse_pdf_bytes("distinct-name-bytes.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.text_elements.len(), 2);
    assert_eq!(
        parsed.text_elements[0].font_name.as_deref(),
        Some("Helvetica")
    );
    assert_eq!(
        parsed.text_elements[1].font_name.as_deref(),
        Some("Courier")
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// 空白・区切り・非 UTF-8 を含む任意の非 null byte 列を、大小の escape で照合する。
    #[test]
    fn escaped_resource_bytes_preserve_images_and_fonts(bytes in prop::collection::vec(1u8..=255, 1..12)) {
        let key = bytes.iter().map(|byte| format!("#{byte:02X}")).collect::<String>();
        let operand = bytes.iter().map(|byte| format!("#{byte:02x}")).collect::<String>();
        let mut objects = pages(&format!("/MediaBox [0 0 100 100] /Resources << /Font << /{key} 10 0 R >> /XObject << /{key} 7 0 R >> >>"));
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
        objects[4] = stream(&format!("q 10 0 0 10 0 0 cm /{operand} Do Q BT /{operand} 12 Tf 10 20 Td (Hello) Tj ET"));
        objects[5] = objects[4].clone();
        let parsed = parse_pdf_bytes("name-bytes-property.pdf", &pdf(&objects)).unwrap();
        prop_assert_eq!(parsed.image_elements.len(), 2);
        prop_assert_eq!(parsed.text_elements.len(), 2);
        prop_assert!(parsed.text_elements.iter().all(|text| text.font_name.as_deref() == Some("Helvetica")));
    }
}
