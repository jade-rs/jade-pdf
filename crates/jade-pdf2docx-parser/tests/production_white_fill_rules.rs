//! 白い目隠しの塗りを黒罫線へ変え、離れた表を接続しない。
use jade_pdf2docx_parser::parse_pdf_bytes;
#[allow(dead_code)]
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 所有する一ページの本文とResourceだけから正式なPDFを組み立てる。
fn source(content: &str, resources: &str) -> Vec<u8> {
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 500 500] /Resources {resources} >>"),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into(),
        page_attribute_pdf::stream(content),
    ])
}

/// 白い中央帯を挟む黒い二本の原本罫線を、元の座標のまま返す。
fn edges(prefix: &str, resources: &str) -> Vec<[i32; 4]> {
    let content = format!("{prefix} 0 g 20 100 100 1 re f 1 g 120 100 20 1 re f 0 g 140 100 100 1 re f");
    let parsed = parse_pdf_bytes("owned-white-clearance.pdf", &source(&content, resources)).unwrap();
    parsed.graphics.iter().map(|g| [g.bounds.x0 as i32,g.bounds.y0 as i32,g.bounds.x1 as i32,g.bounds.y1 as i32]).collect()
}

/// DeviceGrayの白塗りは黒い中心線ではなく、離れた黒罫線の空白帯として残す。
#[test]
fn opaque_white_fill_does_not_bridge_black_rules() {
    assert_eq!(edges("", "<< >>"), [[20,101,120,101],[140,101,240,101]]);
}

/// 印刷用の既定設定が白を反転しないことを確認しても、白い目隠しから線を作らない。
#[test]
fn normal_print_defaults_keep_white_clearance_between_rules() {
    let resources = "<< /ExtGState << /Print << /Type /ExtGState /BG2 /Default /UCR2 /Default /OP false /op false /OPM 1 /SA false /SM 0.02 >> >> >>";
    assert_eq!(edges("/Print gs", resources), [[20,101,120,101],[140,101,240,101]]);
}
