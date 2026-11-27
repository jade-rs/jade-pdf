//! 原本位置付きセルにも、既存のMS-PGothic代替fontの字幅補正を渡す。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};
#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 正式な原本gridへ一行・複数行・収まらない二行を配置し、表外の対照も残す。
fn source(mode: usize, font: &str) -> Vec<u8> {
    transformed_source(mode, font, 0., 0., 1.)
}

/// 正式なpageと同じ文字所有を保ったまま、原本の平行移動と一様拡縮を変える。
fn transformed_source(mode: usize, font: &str, dx: f32, dy: f32, scale: f32) -> Vec<u8> {
    let (first, second) = match mode {
        0 => (625, None),
        1 => (628, Some(608)),
        _ => (624, Some(620)),
    };
    let mut content = format!(
        "1 w 72 600 m 272 600 l S 72 650 m 272 650 l S 72 600 m 72 650 l S 172 600 m 172 650 l S 272 600 m 272 650 l S BT /F 10 Tf 82 {first} Td (Alpha) Tj ET BT /F 10 Tf 182 625 Td (Control) Tj ET BT /F 10 Tf 72 710 Td (Outside) Tj ET "
    );
    if let Some(y) = second {
        content.push_str(&format!("BT /F 10 Tf 82 {y} Td (Omega) Tj ET "));
    }
    let content = format!("q {scale} 0 0 {scale} {dx} {dy} cm {content} Q");
    let widths = std::iter::repeat_n("600", 95).collect::<Vec<_>>().join(" ");
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        fixture::stream(&content),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /{font} /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths [{widths}] /FontDescriptor 6 0 R >>"),
        format!("<< /Type /FontDescriptor /FontName /{font} /FontBBox [0 -200 1000 800] /Flags 32 /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 /FontWeight 400 >>"),
    ])
}

/// 一行・収容できる複数行・代替の一段落のいずれでも同じセル補正を使う。
#[test]
fn every_source_cell_route_keeps_fallback_width_without_changing_body_runs() {
    for mode in 0..3 {
        for font in ["MS-PGothic", "AAAAAA+MS-PGothic", "MSPGothic"] {
            let output = xml(convert_minimum_pdf_bytes_to_docx_bytes(
                "owned-cell-width-routes.pdf",
                &source(mode, font),
            )
            .unwrap());
            for label in ["Alpha", "Control"]
                .into_iter()
                .chain((mode > 0).then_some("Omega"))
            {
                assert!(
                    run(&output, label).contains("<w:w w:val=\"92\""),
                    "{mode}/{font}/{label}"
                );
            }
            assert!(!run(&output, "Outside").contains("<w:w "));
        }
    }
}

/// 他のfamilyは字幅を変えず、既存の通常体を維持する。
#[test]
fn unrelated_fallback_families_keep_their_existing_widths_and_weight() {
    for font in ["Helvetica", "MS-Gothic", "MS-Mincho"] {
        let output = xml(convert_minimum_pdf_bytes_to_docx_bytes(
            "owned-other-font-width.pdf",
            &source(1, font),
        )
        .unwrap());
        for label in ["Alpha", "Omega", "Control", "Outside"] {
            assert!(!run(&output, label).contains("<w:w "));
            assert!(run(&output, label).contains("<w:b w:val=\"false\""));
        }
    }
}

/// native file変換でも各セル経路へ補正を渡し、入力のbyte列を変えない。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_file_cells_keep_the_existing_fallback_width() {
    use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};
    for mode in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let input = source(mode, "MS-PGothic");
        let path = dir.path().join("owned.pdf");
        let dest = dir.path().join("owned.docx");
        std::fs::write(&path, &input).unwrap();
        convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
            input_pdf_path: path.clone(),
            output_docx_path: dest.clone(),
            ocr_text_lines: None,
            ocr_table: None,
            clean_output: true,
        })
        .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), input);
        let output = xml(std::fs::read(dest).unwrap());
        for label in ["Alpha", "Control"]
            .into_iter()
            .chain((mode > 0).then_some("Omega"))
        {
            assert!(
                run(&output, label).contains("<w:w w:val=\"92\""),
                "native mode{mode}/{label}"
            );
        }
        assert!(!run(&output, "Outside").contains("<w:w "));
    }
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// セルの位置・sizeと配置経路を変えても、代替字幅はセルだけへ適用する。
    #[test]
    fn fallback_width_is_covariant_with_source_cell_transforms(mode in 0_usize..3,alias in any::<bool>(),dx in 0_f32..20.,dy in 0_f32..15.,scale in 0.8_f32..1.05) {
        let font=if alias {"AAAAAA+MS-PGothic"} else {"MSPGothic"};
        let output=xml(convert_minimum_pdf_bytes_to_docx_bytes("owned-cell-width-transform.pdf",&transformed_source(mode,font,dx,dy,scale)).unwrap());
        for label in ["Alpha","Control"].into_iter().chain((mode>0).then_some("Omega")) {
            prop_assert!(run(&output,label).contains("<w:w w:val=\"92\""),"source-cell correction missing");
        }
        prop_assert!(!run(&output,"Outside").contains("<w:w "));
    }
}

/// 製品の本文XMLを読み、文字を持つrunの属性だけを検査する。
fn xml(output: Vec<u8>) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(output)).unwrap();
    let mut text = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

/// 一意なラベルのrunを切り出し、別セルや表全体のfont指定と混同しない。
fn run<'a>(xml: &'a str, label: &str) -> &'a str {
    let position = xml.find(&format!("{label}</w:t>")).unwrap();
    let start = xml[..position].rfind("<w:r>").unwrap();
    let end = position + xml[position..].find("</w:r>").unwrap() + "</w:r>".len();
    &xml[start..end]
}

/// 複数行の原本位置付きセルは、既存の代替familyと92%の字幅を同時に保持する。
#[test]
fn bounded_multiline_cells_keep_the_existing_ms_pgothic_fallback_width() {
    let output = xml(convert_minimum_pdf_bytes_to_docx_bytes(
        "owned-cell-fallback-width.pdf",
        &source(1, "AAAAAA+MS-PGothic"),
    )
    .unwrap());
    assert_eq!(output.matches("<w:tbl>").count(), 1);
    for label in ["Alpha", "Omega", "Control"] {
        let r = run(&output, label);
        assert!(r.contains("w:ascii=\"Arial Unicode MS\""));
        assert!(
            r.contains("<w:w w:val=\"92\""),
            "missing existing fallback width for {label}: {r}"
        );
    }
    assert!(
        !run(&output, "Outside").contains("<w:w "),
        "body route keeps its current width"
    );
}
