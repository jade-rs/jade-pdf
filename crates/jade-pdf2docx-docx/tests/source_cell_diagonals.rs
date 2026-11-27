//! 元の空セルの対角線を、向きと線幅を保ったWord罫線へ渡す。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 正式な二行二列の表の右上セルだけへ黒い対角線を引く。
fn source() -> Vec<u8> {
    let content = "0 G .5 w 72 590 m 272 590 l S 72 620 m 272 620 l S 72 650 m 272 650 l S 72 590 m 72 650 l S 172 590 m 172 650 l S 272 590 m 272 650 l S 172 650 m 272 620 l S BT /F 10 Tf 82 633 Td (Alpha) Tj ET BT /F 10 Tf 82 603 Td (Beta) Tj ET BT /F 10 Tf 182 603 Td (Control) Tj ET";
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
}

/// 黒い斜線を、同じ空セルの左上から右下へ引くWord罫線にする。
#[test]
fn a_source_diagonal_is_preserved_in_its_empty_table_cell() {
    let output =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-cell-diagonal.pdf", &source()).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(output)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert_eq!(xml.matches("<w:tbl>").count(), 1);
    assert!(
        xml.contains("Alpha</w:t>") && xml.contains("Beta</w:t>") && xml.contains("Control</w:t>")
    );
    assert_eq!(
        xml.matches("<w:tl2br ").count(),
        1,
        "the actual top-left-to-bottom-right source line must remain in its cell"
    );
}

/// 塗り・空セル・結合・変換を独立に変え、端点からセル所有を確認する。
fn controlled(diagonal: &str, upper: &str, merged: bool, dx: f32, dy: f32, scale: f32) -> Vec<u8> {
    let middle = if merged {
        "72 620 m 172 620 l S"
    } else {
        "72 620 m 272 620 l S"
    };
    let lower = if merged {
        ""
    } else {
        "BT /F 10 Tf 182 603 Td (Control) Tj ET"
    };
    let content = format!(
        "q {scale} 0 0 {scale} {dx} {dy} cm 0 G .5 w 72 590 m 272 590 l S {middle} 72 650 m 272 650 l S 72 590 m 72 650 l S 172 590 m 172 650 l S 272 590 m 272 650 l S {diagonal} BT /F 10 Tf 82 633 Td (Alpha) Tj ET BT /F 10 Tf 82 603 Td (Beta) Tj ET {lower} {upper} Q"
    );
    fixture::pdf(&["<< /Type /Catalog /Pages 2 0 R >>".into(),"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),fixture::stream(&content),"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into()])
}

/// 二方向の対角線をWord本文XMLで区別する。
fn xml(bytes: Vec<u8>) -> String {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

/// 原本の右上から左下を、端点の記述順を逆にしても同じWord罫線へ渡す。
#[test]
fn reverse_endpoints_do_not_reverse_the_visual_diagonal() {
    for line in ["272 650 m 172 620 l S", "172 620 m 272 650 l S"] {
        let s = xml(convert_minimum_pdf_bytes_to_docx_bytes(
            "owned-opposite-diagonal.pdf",
            &controlled(line, "", false, 0., 0., 1.),
        )
        .unwrap());
        assert_eq!(s.matches("<w:tr2bl ").count(), 1);
        assert_eq!(s.matches("<w:tl2br ").count(), 0);
        let at = s.find("<w:tr2bl ").unwrap();
        let left = s[..at].rfind("<w:tc>").unwrap();
        let right = at + s[at..].find("</w:tc>").unwrap();
        let cell = &s[left..right];
        assert!(!cell.contains("Alpha") && !cell.contains("Beta") && !cell.contains("Control"));
        assert!(cell.contains("w:sz=\"4\""));
    }
}

/// 縦に結合された空セルの全高へ一本だけの罫線を付け、継続セルへ複製しない。
#[test]
fn a_merged_empty_cell_owns_one_full_height_diagonal() {
    let s = xml(convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-merged-diagonal.pdf",
        &controlled("272 650 m 172 590 l S", "", true, 0., 0., 1.),
    )
    .unwrap());
    assert_eq!(s.matches("<w:tr2bl ").count(), 1);
    assert!(s.contains("<w:vMerge w:val=\"restart\""));
    assert!(s.contains("<w:vMerge w:val=\"continue\""));
    assert!(s.contains("Alpha</w:t>") && s.contains("Beta</w:t>"));
}

/// 短い線・角の不一致・非空セル・後の覆いを、空セルの対角罫線へ昇格しない。
#[test]
fn partial_displaced_occupied_and_occluded_lines_keep_the_previous_table() {
    for (line, upper) in [
        ("182 645 m 262 625 l S", ""),
        ("172 647 m 272 620 l S", ""),
        (
            "172 650 m 272 620 l S",
            "BT /F 10 Tf 182 633 Td (Occupied) Tj ET",
        ),
        ("172 650 m 272 620 l S 1 g 170 618 105 35 re f", ""),
        ("1 G 172 650 m 272 620 l S", ""),
    ] {
        let s = xml(convert_minimum_pdf_bytes_to_docx_bytes(
            "owned-unconfirmed-diagonal.pdf",
            &controlled(line, upper, false, 0., 0., 1.),
        )
        .unwrap());
        assert!(
            !s.contains("<w:tr2bl ") && !s.contains("<w:tl2br "),
            "{line} {upper}"
        );
        assert_eq!(s.matches("<w:tbl>").count(), 1);
    }
}

/// 対角罫線の追加以外は、同じ本文XMLを維持して既存セルの文字・書式・幾何を変えない。
#[test]
fn source_diagonal_changes_only_its_own_cell_border_xml() {
    let a = xml(convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-table-control.pdf",
        &controlled("", "", false, 0., 0., 1.),
    )
    .unwrap());
    let b = xml(convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-table-with-diagonal.pdf",
        &controlled("172 650 m 272 620 l S", "", false, 0., 0., 1.),
    )
    .unwrap());
    let start = b.find("<w:tl2br ").unwrap();
    let end = start + b[start..].find("/>").unwrap() + 2;
    assert_eq!(a, format!("{}{}", &b[..start], &b[end..]));
}

/// native file経路でも対角線を保持し、入力PDFを変更しない。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_diagonal_conversion_keeps_the_original_pdf_bytes() {
    use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};
    let d = tempfile::tempdir().unwrap();
    let input = d.path().join("source.pdf");
    let output = d.path().join("output.docx");
    let bytes = source();
    std::fs::write(&input, &bytes).unwrap();
    convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
        input_pdf_path: input.clone(),
        output_docx_path: output.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    })
    .unwrap();
    assert_eq!(std::fs::read(input).unwrap(), bytes);
    assert_eq!(
        xml(std::fs::read(output).unwrap())
            .matches("<w:tl2br ")
            .count(),
        1
    );
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 表の移動・一様拡縮・斜線の向きが変わっても、同じ空セルだけが所有する。
    #[test]
    fn cell_diagonals_follow_source_geometry(dx in 0_f32..15.,dy in 0_f32..10.,scale in 0.8_f32..1.05,opposite in any::<bool>()) {
        let line=if opposite{"272 650 m 172 620 l S"}else{"172 650 m 272 620 l S"};
        let s=xml(convert_minimum_pdf_bytes_to_docx_bytes("owned-transformed-cell-diagonal.pdf",&controlled(line,"",false,dx,dy,scale)).unwrap());
        prop_assert_eq!(s.matches("<w:tr2bl ").count(),usize::from(opposite));prop_assert_eq!(s.matches("<w:tl2br ").count(),usize::from(!opposite));
    }
}

/// 原本セルの矩形clipで端を切った斜線も、同じ空セルだけに保持する。
#[test]
fn cell_clipped_source_diagonal_survives_black_border_overpaint() {
    let line = "q 172 620 100 30 re W n 172 650 m 272 620 l S Q 0 g 171.5 619.5 1 31 re f";
    let s = xml(convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-clipped-cell-diagonal.pdf",
        &controlled(line, "", false, 0., 0., 1.),
    )
    .unwrap());
    assert_eq!(s.matches("<w:tl2br ").count(), 1);
    assert_eq!(s.matches("<w:tr2bl ").count(), 0);
}

/// 二本が角の許容範囲に入っても、描画順で最後の原本線幅を方向ごとに選ぶ。
#[test]
fn diagonal_index_keeps_source_paint_order_for_overlapping_candidates() {
    let lines = "0.5 w 172 650 m 272 620 l S 1 w 272 620.4 m 172 649.5 l S                  0.75 w 172 620 m 272 650 l S 1.25 w 272 649.5 m 172 620.4 l S";
    let s = xml(convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-overlapping-cell-diagonals.pdf",
        &controlled(lines, "", false, 0., 0., 1.),
    )
    .unwrap());
    assert_eq!(s.matches("<w:tl2br ").count(), 1);
    assert_eq!(s.matches("<w:tr2bl ").count(), 1);
    assert!(s.contains(r#"<w:tl2br w:val="single" w:sz="8""#));
    assert!(s.contains(r#"<w:tr2bl w:val="single" w:sz="10""#));
}
