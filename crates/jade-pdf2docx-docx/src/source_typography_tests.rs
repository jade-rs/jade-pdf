//! 原本の通常体・太字・未知の書体名を区別する回帰。
use super::*;
use docx_rs::BuildXML;

/// font適用後のrun XMLを読む。本文は同じ綴りでもfontごとに独立する。
fn run_xml(name: &str, prior_bold: bool, face: Option<embedded_face::Face>) -> String {
    let run = Run::new().add_text("same source text");
    let run = if prior_bold { run.bold() } else { run };
    String::from_utf8(apply_pdf_font_name_to_run(run, name, face.is_some(), face, None).build())
        .unwrap()
}

/// 明示的な通常体は、既に付いた見出しの太字を解除する。
#[test]
fn source_regular_clears_prior_heading_bold() {
    for name in ["Source-Regular", "Source-Light", "Source-Medium"] {
        let xml = run_xml(name, true, None);
        assert!(
            xml.contains("<w:b w:val=\"false\""),
            "explicit normal source weight must override heading bold: {name}: {xml}"
        );
        assert!(!xml.contains("<w:b />"));
    }
}

/// family名の一部はweightの証拠ではなく、未知のW番号も太字を発明しない。
#[test]
fn family_substring_does_not_assert_bold() {
    for name in [
        "Blackburn-Regular",
        "BolderFamily-W3",
        "Gothic-W6",
        "Unknown-W9",
    ] {
        let xml = run_xml(name, false, None);
        assert!(
            !xml.contains("<w:b />"),
            "family spelling must not invent source bold: {name}: {xml}"
        );
    }
}

/// 埋め込みfont内部の通常体も、名前や先に付いた太字より優先する。
#[test]
fn intrinsic_regular_clears_prior_heading_bold() {
    let bytes = embedded_style_tests::metadata_font(64, 0);
    let face = embedded_face::Face::read(&bytes).unwrap();
    let xml = run_xml("Source-Bold", true, Some(face));
    assert!(
        xml.contains("<w:b w:val=\"false\""),
        "intrinsic source regular must override heading bold: {xml}"
    );
    assert!(!xml.contains("<w:b />"));
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(32))]
    /// familyを変えても、明示的な接尾辞の通常体を太字へ変えない。
    #[test]
    fn arbitrary_family_does_not_override_explicit_normal(family in "[A-Za-z]{1,24}", index in 0_usize..4) {
        let style=["Regular","Light","Medium","Book"][index];
        let xml=run_xml(&format!("{family}-{style}"),true,None);
        proptest::prop_assert!(!xml.contains("<w:b />"));
        proptest::prop_assert!(xml.contains("<w:b w:val=\"false\""));
    }
    /// 埋め込みfaceの通常体と太字を名前の反対の宣言で上書きしない。
    #[test]
    fn intrinsic_face_weight_wins_over_conflicting_suffix(bold in proptest::prelude::any::<bool>(), prior in proptest::prelude::any::<bool>()) {
        let bytes=embedded_style_tests::metadata_font(if bold {32}else{64},0);
        let face=embedded_face::Face::read(&bytes).unwrap();
        let xml=run_xml(if bold {"Family-Regular"}else{"Family-Bold"},prior,Some(face));
        proptest::prop_assert_eq!(xml.contains("<w:b />"),bold);
        proptest::prop_assert_eq!(xml.contains("<w:b w:val=\"false\""),!bold);
    }
}

/// 既知の太字・複合styleは保持し、未知名では呼び出し側の指定を消さない。
#[test]
fn explicit_bold_survives_and_unknown_weight_preserves_prior_style() {
    for name in [
        "Helvetica-Bold",
        "Times-BoldItalic",
        "Arial-BoldMT",
        "Family-Black",
        "ABCDEF+Family-BoldOblique",
    ] {
        assert!(run_xml(name, false, None).contains("<w:b />"), "{name}");
    }
    for name in ["Family-W6", "Unknown", "Family-Unrecognized"] {
        assert!(run_xml(name, true, None).contains("<w:b />"), "{name}");
        let xml = run_xml(name, false, None);
        assert!(!xml.contains("<w:b />"));
        assert!(!xml.contains("<w:b w:val=\"false\""));
    }
}

/// 最終ZIPでは通常体をLatinと複雑書字の両方へ明示し、隣の太字は保持する。
#[test]
fn packaged_source_regular_disables_complex_script_bold_as_well() {
    use std::io::{Cursor, Read};
    let regular = apply_pdf_font_name_to_run(
        Run::new().add_text("normal"),
        "Source-Regular",
        false,
        None,
        None,
    );
    let bold = apply_pdf_font_name_to_run(
        Run::new().add_text("bold"),
        "Source-Bold",
        false,
        None,
        None,
    );
    let p = Paragraph::new().add_run(regular).add_run(bold);
    let docx = Docx::new()
        .header(Header::new().add_paragraph(p.clone()))
        .footer(Footer::new().add_paragraph(p.clone()))
        .add_paragraph(p);
    let bytes = pack_docx_to_bytes(docx, &[]).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    for name in ["word/document.xml", "word/header1.xml", "word/footer1.xml"] {
        let mut xml = String::new();
        zip.by_name(name).unwrap().read_to_string(&mut xml).unwrap();
        assert!(xml.contains("<w:b w:val=\"false\""), "{name}: {xml}");
        assert!(
            xml.contains("<w:bCs w:val=\"false\""),
            "packaged source regular must disable complex-script bold: {name}: {xml}"
        );
        assert_eq!(
            xml.matches("<w:bCs />").count(),
            1,
            "the neighbouring source bold must survive: {name}: {xml}"
        );
    }
}

/// family名がなくても、独立に確認されたdescriptorの太字を保持する。
#[test]
fn declared_weight_survives_missing_font_name() {
    for name in [None, Some(String::new()), Some(" ".into())] {
        let run = StructuredTextRun {
            text: "Weight".into(),
            style: StructuredTextStyle {
                font_name: name,
                font_weight: Some(700),
                ..Default::default()
            },
        };
        let xml = String::from_utf8(docx_run_from_structured_run(&run).build()).unwrap();
        assert!(xml.contains("<w:b />"), "{xml}");
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(24))]
    /// 実際の埋め込みfaceは、反対のdescriptor weightでも上書きしない。
    #[test]
    fn intrinsic_face_precedes_declared_weight(bold in proptest::prelude::any::<bool>(), weight in 1u16..=1000) {
        let face=embedded_face::Face {bold, ..Default::default()};
        let xml=String::from_utf8(apply_pdf_font_name_to_run(Run::new(),"Owned",true,Some(face),Some(weight)).build()).unwrap();
        proptest::prop_assert_eq!(xml.contains("<w:b />"),bold);
        proptest::prop_assert_eq!(xml.contains("<w:b w:val=\"false\""),!bold);
    }
}
