//! 原本の通常体・太字・未知の書体名を区別する回帰。
use super::*;
use docx_rs::BuildXML;

/// font適用後のrun XMLを読む。本文は同じ綴りでもfontごとに独立する。
fn run_xml(name: &str, prior_bold: bool, face: Option<embedded_face::Face>) -> String {
    let run = Run::new().add_text("same source text");
    let run = if prior_bold { run.bold() } else { run };
    String::from_utf8(apply_pdf_font_name_to_run(run, name, face.is_some(), face).build()).unwrap()
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
