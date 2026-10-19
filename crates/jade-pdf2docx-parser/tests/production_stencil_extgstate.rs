//! stencilへ影響しないExtGStateを実PDFと正式xrefで検証する。
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{ParseOptions, QualityIssue, parse_pdf_bytes_with_options};

/// 直接/間接/継承ResourceのOPMだけでは塗り色とalphaを変更しない。
#[test]
fn overprint_mode_only_preserves_stencil_paint() {
    for inherited in [false, true] {
        for indirect in [false, true] {
            for indirect_states in [false, true] {
                for state in [
                    "<< >>",
                    "<< /Type /ExtGState /OPM 1 >>",
                    "<< /OPM 0 >>",
                    "<< /OPM +1 >>",
                    "<< /OPM -0 >>",
                    "<< /Ty#70e /ExtG#53tate /O#50M 9 0 R >>",
                    "<< /AIS false /BM /Normal /CA 1 /OP false /OPM 1 /SA true /SMask /None /Type /ExtGState /ca 1 /op false >>",
                    "<< /c#61 +01.000 /CA 9 0 R /BM /Nor#6dal >>",
                ] {
                    let states = "<< /S#61fe 12 0 R >>";
                    let resources = format!(
                        "<< /XObject << /Im 4 0 R >> /ExtGState {} >>",
                        if indirect_states { "14 0 R" } else { states }
                    );
                    let pdf = fixture::stencil_with_resources(
                        &fixture::compressed(&[0x40]),
                        b".25 .5 .75 rg /Safe gs /Im Do",
                        if indirect { "13 0 R" } else { &resources },
                        inherited,
                        &[state.as_bytes(), resources.as_bytes(), states.as_bytes()],
                    );
                    let document =
                        parse_pdf_bytes_with_options("gs.pdf", &pdf, &ParseOptions::default())
                            .unwrap();
                    assert_eq!(
                        document.quality.issues,
                        [QualityIssue::OcrRequired { page: 0 }],
                        "{state}, inherited={inherited}, indirect={indirect}, states={indirect_states}"
                    );
                    assert_eq!(
                        fixture::scanline(&document.image_elements[0].png_bytes),
                        [0, 64, 128, 191, 255, 64, 128, 191, 0]
                    );
                }
            }
        }
    }
}

/// 未対応・不正・未解決状態の後にno-op gsを適用しても、不透明と推測しない。
#[test]
fn safe_state_does_not_reset_unsupported_transparency() {
    for state in [
        "<< /SMask /None /BM /Multiply >>",
        "<< /TR /Identity >>",
        "<< /OPM 2 >>",
        "<< /OPM 1.0 >>",
        "<< /Type /Wrong /OPM 1 >>",
        "<< /OPM 99 0 R >>",
        "<< /OPM 9 1 R >>",
        "<< /OPM 12 0 R >>",
        "<< /OPM 1 /OPM 0 >>",
        "<< /OPM 1 /Dangling >>",
        "<< /ca 1 /ca .5 >>",
        "<< /ca 1 /c#61 .5 >>",
        "<< /ca 1.0000000000000001 >>",
        "<< /ca 1e0 >>",
        "<< /ca nan >>",
        "<< /AIS true >>",
        "<< /OP true >>",
        "<< /op true >>",
        "<< /SA false >>",
        "<< /TK 1 >>",
        "<< /TK true /T#4b false >>",
        "<< /BM [/Normal] >>",
        "<< /SMask << /S /Alpha >> >>",
        "null",
        "[]",
    ] {
        let pdf = fixture::stencil_with_resources(
            &fixture::compressed(&[0x40]),
            b"1 0 0 rg q /Bad gs /Im Do /Safe gs /Im Do Q /Im Do",
            "<< /XObject << /Im 4 0 R >> /ExtGState << /Bad 12 0 R /Safe << /OPM 1 >> >> >>",
            false,
            &[state.as_bytes()],
        );
        let document =
            parse_pdf_bytes_with_options("gs.pdf", &pdf, &ParseOptions::default()).unwrap();
        assert!(
            document
                .quality
                .issues
                .iter()
                .any(|issue| matches!(issue, QualityIssue::ImageReplaced { .. })),
            "{state}"
        );
        assert_ne!(
            fixture::scanline(&document.image_elements[0].png_bytes),
            [0, 255, 0, 0, 255, 255, 0, 0, 0],
            "{state}"
        );
        assert_ne!(
            fixture::scanline(&document.image_elements[1].png_bytes),
            [0, 255, 0, 0, 255, 255, 0, 0, 0],
            "{state}"
        );
        assert_eq!(
            fixture::scanline(&document.image_elements[2].png_bytes),
            [0, 255, 0, 0, 255, 255, 0, 0, 0],
            "{state}"
        );
    }
}

/// gs名前空間の重複や末尾破損で、安全な先頭項目だけを採用しない。
#[test]
fn malformed_state_namespace_is_not_partially_accepted() {
    for states in [
        "<< /S << /OPM 1 >> /S << /ca .5 >> >>",
        "<< /S << /OPM 1 >> /#53 << /ca .5 >> >>",
        "<< /S << /OPM 1 >> /S#00 << >> /Dangling >>",
    ] {
        let resources = format!("<< /XObject << /Im 4 0 R >> /ExtGState {states} >>");
        let pdf = fixture::stencil_with_resources(
            &fixture::compressed(&[0x40]),
            b"/S gs /Im Do",
            &resources,
            false,
            &[],
        );
        let document =
            parse_pdf_bytes_with_options("namespace.pdf", &pdf, &ParseOptions::default()).unwrap();
        assert!(
            document
                .quality
                .issues
                .iter()
                .any(|issue| matches!(issue, QualityIssue::ImageReplaced { .. })),
            "{states}"
        );
    }
}
