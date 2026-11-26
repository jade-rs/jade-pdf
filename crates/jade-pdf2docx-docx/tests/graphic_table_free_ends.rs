//! 横罫線へ接続しない棒の端点を、編集可能な表の列境界と断定しない。
use jade_pdf2docx_docx::{
    convert_minimum_pdf_bytes_to_docx_bytes, infer_ocr_tables_from_pdf_graphics,
};
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 同じ外枠と横線で、自由端を持つ棒または横線へ接続した部分縦罫線を描く所有原本。
fn source(anchored: bool) -> Vec<u8> {
    source_with_geometry(anchored, 1, 0, 0, false)
}

/// 平行移動・拡大・同一直線の分割描画を変えても、端点接続の意味を保つ。
fn source_with_geometry(anchored: bool, scale: u16, dx: u16, dy: u16, split: bool) -> Vec<u8> {
    let mut content = format!(
        "q {scale} 0 0 {scale} {dx} {dy} cm 0.5 w 80 100 m 320 100 l 320 400 l 80 400 l h S "
    );
    for y in [200, 300] {
        content.push_str(&format!("80 {y} m 320 {y} l S "));
    }
    for (x, free, joined) in [
        (140, 240, 200),
        (180, 280, 300),
        (220, 340, 400),
        (260, 370, 300),
    ] {
        let end = if anchored { joined } else { free };
        if split {
            content.push_str(&format!("{x} 100 m {x} 150 l S {x} 150 m {x} {end} l S "));
        } else {
            content.push_str(&format!("{x} 100 m {x} {end} l S "));
        }
    }
    content.push_str(
        "BT /F1 10 Tf 90 110 Td (OwnedAlpha) Tj ET BT /F1 10 Tf 270 350 Td (OwnedBeta) Tj ET Q",
    );
    page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 1500 1800] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
            .into(),
        page_attribute_pdf::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 公開byte変換のWord本文を読み、推定APIと実際の表出力を両方検査する。
fn document(input: &[u8]) -> String {
    let bytes =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-free-ended-bars.pdf", input).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// 横線を横切る棒の端点がセル隅にない原本から、架空の行列表を作らない。
#[test]
fn free_ended_bars_do_not_become_editable_table_columns() {
    let input = source(false);
    let parsed = parse_pdf_bytes("owned-free-ended-bars.pdf", &input).unwrap();
    assert!(
        infer_ocr_tables_from_pdf_graphics(&parsed).is_empty(),
        "free-ended bars do not establish cell boundaries"
    );
}

/// 表推定を棄却しても元の語を一度ずつ保持し、実際のWord本文に表を発明しない。
#[test]
fn default_word_output_preserves_labels_without_a_fake_table() {
    let xml = document(&source(false));
    assert!(
        !xml.contains("<w:tbl>"),
        "bar chart must not become a ruled Word table"
    );
    for label in ["OwnedAlpha", "OwnedBeta"] {
        assert_eq!(
            xml.matches(label).count(),
            1,
            "source label is preserved once"
        );
    }
}

/// 横線へ接続した部分縦罫線は、結合セルを含む編集可能な表として維持する。
#[test]
fn anchored_partial_verticals_keep_the_editable_table() {
    let input = source(true);
    let parsed = parse_pdf_bytes("owned-anchored-partial-rules.pdf", &input).unwrap();
    assert_eq!(infer_ocr_tables_from_pdf_graphics(&parsed).len(), 1);
    let xml = document(&input);
    assert!(xml.contains("<w:tbl>"));
    for label in ["OwnedAlpha", "OwnedBeta"] {
        assert_eq!(xml.matches(label).count(), 1);
    }
}

/// 同一直線を複数の描画命令へ分けた接合部を、自由端と誤認しない。
#[test]
fn joined_collinear_segments_keep_the_editable_table() {
    let input = source_with_geometry(true, 1, 0, 0, true);
    let parsed = parse_pdf_bytes("owned-segmented-table-rules.pdf", &input).unwrap();
    assert_eq!(infer_ocr_tables_from_pdf_graphics(&parsed).len(), 1);
    assert!(document(&input).contains("<w:tbl>"));
}

/// 横罫線のない見出し帯で止まる縦区切りは、複数の実在行を横切る棒と混同しない。
#[test]
fn open_header_separator_keeps_the_editable_table() {
    let input = source(true);
    let text = String::from_utf8(input).unwrap();
    let start = text.find("\nstream\n").unwrap() + 8;
    let end = start + text[start..].find("\nendstream").unwrap();
    let content = text[start..end]
        .replace("80 200 m 320 200 l S ", "")
        .replace("220 400 l S", "220 370 l S");
    let input = page_attribute_pdf::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox [0 0 1500 1800] >>".into(),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
            .into(),
        page_attribute_pdf::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ]);
    let parsed = parse_pdf_bytes("owned-open-header.pdf", &input).unwrap();
    assert_eq!(infer_ocr_tables_from_pdf_graphics(&parsed).len(), 1);
    assert!(document(&input).contains("<w:tbl>"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 原本の位置・倍率・描画命令の分割によらず、接続した罫線だけを表へ採用する。
    #[test]
    fn source_boundary_connections_survive_transform_and_segmentation(scale in 1u16..4,dx in 0u16..100,dy in 0u16..100,split in any::<bool>()) {
        for anchored in [false,true] {
            let input=source_with_geometry(anchored,scale,dx,dy,split);
            let parsed=parse_pdf_bytes("owned-transformed-rules.pdf",&input).unwrap();
            prop_assert_eq!(infer_ocr_tables_from_pdf_graphics(&parsed).len(),usize::from(anchored));
            let xml=document(&input);
            prop_assert_eq!(xml.contains("<w:tbl>"),anchored);
            for label in ["OwnedAlpha","OwnedBeta"] {prop_assert_eq!(xml.matches(label).count(),1);}
        }
    }
}
