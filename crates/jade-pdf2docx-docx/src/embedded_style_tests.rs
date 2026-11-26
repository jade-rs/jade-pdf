//! 埋め込みfont内部のstyleと権利をDOCXまで伝播する回帰。
use super::*;
use std::io::{Cursor, Read};

/// styleだけを検証する所有SFNTを作る。描画用fontの代替には使わない。
pub(super) fn metadata_font(selection: u16, permission: u16) -> Vec<u8> {
    let mut bytes = vec![0u8; 176];
    bytes[..4].copy_from_slice(b"OTTO");
    bytes[4..6].copy_from_slice(&2u16.to_be_bytes());
    for (index, (tag, offset, length)) in [(b"head", 44u32, 54u32), (b"OS/2", 98, 78)]
        .into_iter()
        .enumerate()
    {
        let entry = 12 + index * 16;
        bytes[entry..entry + 4].copy_from_slice(tag);
        bytes[entry + 8..entry + 12].copy_from_slice(&offset.to_be_bytes());
        bytes[entry + 12..entry + 16].copy_from_slice(&length.to_be_bytes());
    }
    bytes[44..48].copy_from_slice(&0x00010000u32.to_be_bytes());
    bytes[56..60].copy_from_slice(&0x5f0f3cf5u32.to_be_bytes());
    bytes[106..108].copy_from_slice(&permission.to_be_bytes());
    bytes[160..162].copy_from_slice(&selection.to_be_bytes());
    bytes
}

/// 単一の埋め込みfaceを本文に選択したDOCXを作る。
fn package(selection: u16, permission: u16, name: &str) -> Vec<u8> {
    let mut parsed = jade_pdf2docx_parser::parse_pdf_bytes(
        "owned-style.pdf",
        b"%PDF-1.4\n1 0 obj << /Type /Page /MediaBox [0 0 200 200] >> endobj\n",
    )
    .unwrap();
    parsed.text_elements = vec![ParsedTextElement {
        fill_stroke_width: None,
        font_weight: None,
        baseline_y: Some(100.),
        page: 0,
        bounds: ParsedBounds {
            x0: 20.,
            y0: 100.,
            x1: 60.,
            y1: 110.,
        },
        font_size: 10.,
        font_name: Some(name.into()),
        fill_color: None,
        text: "test".into(),
    }];
    parsed.embedded_fonts = vec![ParsedEmbeddedFont {
        font_name: name.into(),
        bytes: metadata_font(selection, permission),
    }];
    let docx = build_structured_docx_from_parsed(&parsed, &None, &[]).unwrap();
    pack_docx_to_bytes(docx, &parsed.embedded_fonts).unwrap()
}

/// DOCX内の指定XMLを読む。
fn xml(bytes: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 名前にstyleを含まないfaceでも、runと埋め込みslotを一致させる。
#[test]
fn embedded_style_uses_intrinsic_attributes_in_runs_and_font_table() {
    for (selection, bold, italic, slot) in [
        (32, true, false, "embedBold"),
        (1, false, true, "embedItalic"),
        (33, true, true, "embedBoldItalic"),
        (64, false, false, "embedRegular"),
    ] {
        let bytes = package(selection, 0, "JadeT1-hash");
        let document = xml(&bytes, "word/document.xml");
        assert_eq!(document.contains("<w:b />"), bold, "{document}");
        assert_eq!(document.contains("<w:i />"), italic, "{document}");
        assert!(xml(&bytes, "word/fontTable.xml").contains(&format!("<w:{slot} ")));
    }
}

/// ランダムな識別名の部分文字列をstyleの根拠にしない。
#[test]
fn embedded_style_ignores_bold_italic_substrings_in_derived_name() {
    let bytes = package(64, 0, "JadeT1-BOLDITALIC-hash");
    let document = xml(&bytes, "word/document.xml");
    assert!(!document.contains("<w:b />"), "{document}");
    assert!(!document.contains("<w:i />"), "{document}");
}

/// 全glyphを保つno-subsetting faceにsubsetted=trueを宣言しない。
#[test]
fn embedded_style_preserves_no_subsetting_permission() {
    let bytes = package(64, 0x100, "JadeT1-full");
    assert!(xml(&bytes, "word/fontTable.xml").contains("w:subsetted=\"false\""));
}

/// OS/2がないfaceではheadのstyleを使い、壊れたdirectoryを拒否する。
#[test]
fn embedded_style_reads_head_fallback_and_rejects_invalid_offsets() {
    let mut bytes = metadata_font(0, 0);
    bytes[28..32].copy_from_slice(b"name");
    bytes[88..90].copy_from_slice(&3u16.to_be_bytes());
    let face = embedded_face::Face::read(&bytes).unwrap();
    assert!(face.bold && face.italic);
    for offset in [0u32, 12, 43, 175, u32::MAX] {
        let mut corrupt = bytes.clone();
        corrupt[20..24].copy_from_slice(&offset.to_be_bytes());
        assert!(embedded_face::Face::read(&corrupt).is_none());
    }
}

proptest::proptest! {
    /// 任意のstyle/rightsビットでも、既知の各属性だけを独立に伝える。
    #[test]
    fn embedded_style_flags_are_independent(selection in proptest::prelude::any::<u16>(), rights in proptest::prelude::any::<u16>()) {
        let mut bytes = metadata_font(selection, rights);
        bytes[98..100].copy_from_slice(&4u16.to_be_bytes());
        let face = embedded_face::Face::read(&bytes).unwrap();
        proptest::prop_assert_eq!(face.bold, selection & 32 != 0);
        proptest::prop_assert_eq!(face.italic, selection & 513 != 0);
        proptest::prop_assert_eq!(face.no_subsetting, rights & 256 != 0);
    }

    /// 切断・任意データのmetadata検査はpanicせず、借用範囲の外を読まない。
    #[test]
    fn embedded_style_arbitrary_metadata_is_bounded(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..1024)) {
        let _ = embedded_face::Face::read(&bytes);
        let original = metadata_font(33, 256);
        for end in 0..original.len() {
            proptest::prop_assert!(embedded_face::Face::read(&original[..end]).is_none());
        }
    }
}
