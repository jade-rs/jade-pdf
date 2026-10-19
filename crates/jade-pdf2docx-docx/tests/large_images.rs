//! 多数画像のDOCX保存と資源超過時の保存先保護を検証する。
#[path = "../../jade-pdf2docx-parser/tests/support/image_count_pdf.rs"]
mod fixture;
use std::io::{Cursor, Read};

/// 21枚の描画画像と本文をDOCXへ渡し、mediaを白画像へ変えない。
#[test]
fn many_images_keep_media_and_drawings_in_docx() {
    let pdf = fixture::image_count_pdf(21, 0, true);
    let docx =
        jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes("many.pdf", &pdf).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(docx)).unwrap();
    let mut count = 0;
    for index in 0..archive.len() {
        let mut part = archive.by_index(index).unwrap();
        if !part.name().starts_with("word/media/") || part.is_dir() {
            continue;
        }
        let mut png = Vec::new();
        part.read_to_end(&mut png).unwrap();
        assert_eq!(
            &png[16..24],
            [2u32.to_be_bytes(), 1u32.to_be_bytes()].concat()
        );
        assert_eq!(png[25], 6);
        let mut idat = Vec::new();
        let mut cursor = 8;
        while cursor + 12 <= png.len() {
            let length = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
            if &png[cursor + 4..cursor + 8] == b"IDAT" {
                idat.extend_from_slice(&png[cursor + 8..cursor + 8 + length]);
            }
            cursor += length + 12;
        }
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(idat.as_slice())
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(raw, [0, 10, 20, 30, 0, 40, 50, 60, 77]);
        count += 1;
    }
    assert!(count > 0);
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert_eq!(xml.matches("</pic:pic>").count(), 21);
    assert!(xml.contains("retained text"));
}

/// 多数画像の保存予算超過でも入力と既存DOCXを変更しない。
#[test]
fn many_image_resource_failure_preserves_existing_destination() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("many.pdf");
    let output = directory.path().join("existing.docx");
    let pdf = fixture::image_count_pdf(21, 0, true);
    std::fs::write(&input, &pdf).unwrap();
    std::fs::write(&output, b"original output").unwrap();
    for strict in [false, true] {
        let result = jade_pdf2docx_docx::convert_minimum_pdf_to_docx_with_options(
            &jade_pdf2docx_docx::MinimumPdfToDocxRequest {
                input_pdf_path: input.clone(),
                output_docx_path: output.clone(),
                ocr_text_lines: None,
                ocr_table: None,
                clean_output: true,
            },
            &jade_pdf2docx_parser::ParseOptions {
                strict,
                limits: jade_pdf2docx_parser::ParseLimits {
                    max_total_image_storage_bytes: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assert!(matches!(
            result,
            Err(jade_pdf2docx_docx::ConvertError::Parse(
                jade_pdf2docx_parser::ParseError::ResourceLimit {
                    resource: "image storage bytes",
                    ..
                }
            ))
        ));
        assert_eq!(std::fs::read(&input).unwrap(), pdf);
        assert_eq!(std::fs::read(&output).unwrap(), b"original output");
    }
}
