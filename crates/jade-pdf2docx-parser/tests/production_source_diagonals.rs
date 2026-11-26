//! 対角線の方向・描画状態・clip・描画順を、正式な公開PDF入力から検証する。
use jade_pdf2docx_parser::{ParsedDocument, parse_pdf_bytes};
#[allow(dead_code)]
#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// 任意のContents列・継承Resource・ページ属性を持つ原本を作る。
fn document(contents: &[&str], resources: &str, attributes: &str) -> ParsedDocument {
    let refs = (0..contents.len())
        .map(|i| format!("{} 0 R", i + 4))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!(
            "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 500 500] /Resources {resources} >>"
        ),
        format!("<< /Type /Page /Parent 2 0 R /Contents [{refs}] {attributes} >>"),
    ];
    objects.extend(contents.iter().map(|s| fixture::stream(s)));
    parse_pdf_bytes("owned-source-diagonals.pdf", &fixture::pdf(&objects)).unwrap()
}

/// 不正な線幅命令を、既定の線幅の根拠へ黙って戻さない。
#[test]
fn an_invalid_stroke_width_does_not_recover_the_previous_known_width() {
    let parsed = document(&["NaN w 100 200 m 200 100 l S"], "<< >>", "");
    assert!(
        parsed.source_diagonals.is_empty(),
        "an unknown stroke width cannot prove a source diagonal"
    );
}

/// 不明なline cap・変換・座標や、miterが伸びる連結pathを単純な可視線と見なさない。
#[test]
fn unverified_caps_matrices_and_connected_paths_do_not_gain_a_diagonal() {
    for content in [
        "3 J 100 200 m 200 100 l S",
        "NaN 0 0 1 0 0 cm 100 200 m 200 100 l S",
        "100 200 m NaN 100 l 200 100 l S",
        "100 200 m 200 100 l 210 150 l S",
    ] {
        assert!(
            document(&[content], "<< >>", "")
                .source_diagonals
                .is_empty(),
            "{content}"
        );
    }
}
