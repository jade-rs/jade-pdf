//! 既知のRoman互換faceだけを代替familyへ解決し、埋め込みfaceを上書きしない回帰。
use super::*;
use proptest::prelude::*;
use std::io::{Cursor, Read};

/// 外部fontを使わず、所有文字と任意の所有SFNTメタデータを実際のDOCXへ梱包する。
fn packaged(name: &str, selection: Option<u16>) -> String {
    let mut parsed = jade_pdf2docx_parser::parse_pdf_bytes(
        "owned-roman-fallback.pdf",
        b"%PDF-1.4\n1 0 obj << /Type /Page /MediaBox [0 0 200 200] >> endobj\n",
    )
    .unwrap();
    parsed.text_elements = vec![ParsedTextElement {
        baseline_y: Some(100.),
        page: 0,
        bounds: ParsedBounds {
            x0: 20.,
            y0: 100.,
            x1: 160.,
            y1: 110.,
        },
        font_size: 10.,
        font_name: Some(name.into()),
        fill_color: None,
        text: "Owned fallback text".into(),
    }];
    if let Some(selection) = selection {
        parsed.embedded_fonts.push(ParsedEmbeddedFont {
            font_name: name.into(),
            bytes: embedded_style_tests::metadata_font(selection, 0),
        });
    }
    let docx = build_structured_docx_from_parsed(&parsed, &None, &[]).unwrap();
    let bytes = pack_docx_to_bytes(docx, &parsed.embedded_fonts).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 既知の四faceでは、family解決とMedi/Italの属性を一緒に保持する。
#[test]
fn known_unembedded_roman_faces_keep_family_and_style() {
    for (name, bold, italic) in [
        ("NimbusRomNo9L-Regu", false, false),
        ("NimbusRomNo9L-Medi", true, false),
        ("NimbusRomNo9L-ReguItal", false, true),
        ("ABCDEF+NimbusRomNo9L-MediItal", true, true),
    ] {
        let xml = packaged(name, None);
        assert!(
            xml.contains("w:ascii=\"Times New Roman\""),
            "known unembedded Roman face must resolve to compatible family: {xml}"
        );
        assert_eq!(xml.contains("<w:b />"), bold, "{name}: {xml}");
        assert_eq!(xml.contains("<w:i />"), italic, "{name}: {xml}");
        assert!(xml.contains("Owned fallback text"));
    }
}

/// 実際に埋め込むfaceの名前と内部属性は、互換familyの推測で置き換えない。
#[test]
fn embedded_roman_face_keeps_original_name_and_intrinsic_style() {
    let xml = packaged("NimbusRomNo9L-MediItal", Some(0));
    assert!(xml.contains("w:ascii=\"NimbusRomNo9L-MediItal\""));
    assert!(!xml.contains("w:ascii=\"Times New Roman\""));
    assert!(!xml.contains("<w:b />") && !xml.contains("<w:i />"));
}

/// 似た名前や派生変形名に、既知四faceのfamilyや属性を拡大適用しない。
#[test]
fn unverified_roman_lookalikes_keep_existing_name_fallback() {
    for name in [
        "OwnedNimbusRomNo9L-Regu",
        "NimbusRomNo9L-Regu-Slant_167",
        "NimbusRomNo9L-MediCustom",
    ] {
        let xml = packaged(name, None);
        assert!(xml.contains(&format!("w:ascii=\"{name}\"")), "{xml}");
        assert!(!xml.contains("w:ascii=\"Times New Roman\""));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// subset名・四face・内部styleを変えても、埋め込みfaceの属性を代替規則で変えない。
    #[test]
    fn embedded_face_identity_and_style_override_aliases(
        prefix in "[A-Z]{6}",
        face in 0usize..4,
        style in 0u8..4,
    ) {
        let base = ["NimbusRomNo9L-Regu", "NimbusRomNo9L-Medi", "NimbusRomNo9L-ReguItal", "NimbusRomNo9L-MediItal"][face];
        let name = format!("{prefix}+{base}");
        let selection = if style & 1 != 0 {32} else {0} | if style & 2 != 0 {1} else {0};
        let xml = packaged(&name, Some(selection));
        prop_assert!(xml.contains(&format!("w:ascii=\"{name}\"")), "embedded family changed");
        prop_assert!(!xml.contains("w:ascii=\"Times New Roman\""));
        prop_assert_eq!(xml.contains("<w:b />"), style & 1 != 0);
        prop_assert_eq!(xml.contains("<w:i />"), style & 2 != 0);
    }
}
