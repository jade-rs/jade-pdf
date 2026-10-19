//! source Decodeの実media保存と、不正Decodeのstrict保存先保護。
#[path = "../../jade-pdf2docx-parser/tests/support/ccitt_mask.rs"]
mod codec;
#[path = "../../jade-pdf2docx-parser/tests/support/source_image_pdf.rs"]
mod fixture;
#[path = "../../jade-pdf2docx-parser/tests/support/jbig2_mask.rs"]
mod jbig2;
use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx_with_options};
use jade_pdf2docx_parser::ParseOptions;

/// 描画時のcaを適用したstencil/通常画像のRGBAをDOCX内へ保持する。
#[test]
fn nonstroking_opacity_is_preserved_in_docx_media() {
    use std::io::Read;
    for (attributes, samples, bits, expected) in [
        (
            "/ImageMask true /Filter /FlateDecode",
            vec![0x40],
            1,
            [0, 64, 128, 191, 128, 64, 128, 191, 0],
        ),
        (
            "/ColorSpace /DeviceGray /Filter /FlateDecode /SMask 5 0 R",
            vec![64, 192],
            8,
            [0, 64, 64, 64, 43, 192, 192, 192, 128],
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("alpha.pdf");
        let output = directory.path().join("alpha.docx");
        let pdf = fixture::image_with_resources(
            attributes,
            &fixture::compressed(&samples),
            bits,
            b".25 .5 .75 rg /S gs /Im Do",
            "<< /XObject << /Im 4 0 R >> /ExtGState << /S << /ca .5 >> >> >>",
            false,
            &[],
        );
        std::fs::write(&input, &pdf).unwrap();
        let report = convert_minimum_pdf_to_docx_with_options(
            &MinimumPdfToDocxRequest {
                input_pdf_path: input.clone(),
                output_docx_path: output.clone(),
                ocr_text_lines: None,
                ocr_table: None,
                clean_output: true,
            },
            &ParseOptions::default(),
        )
        .unwrap();
        assert_eq!(
            report.quality.issues,
            [jade_pdf2docx_parser::QualityIssue::OcrRequired { page: 0 }]
        );
        let mut archive = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
        let mut images = 0;
        for index in 0..archive.len() {
            let mut part = archive.by_index(index).unwrap();
            if part.is_dir() || !part.name().starts_with("word/media/") {
                continue;
            }
            let mut png = Vec::new();
            part.read_to_end(&mut png).unwrap();
            assert_eq!(fixture::scanline(&png), expected);
            images += 1;
        }
        assert_eq!(images, 1);
        assert_eq!(std::fs::read(input).unwrap(), pdf);
    }
}

/// 非identity DecodeとMatteを復元した実PNGをDOCX mediaまで保持する。
#[test]
fn source_decode_preserves_media_pixels() {
    use std::io::Read;
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("decode.pdf");
    let output = directory.path().join("decode.docx");
    let pdf = fixture::source_image_pdf(
        "/ColorSpace /DeviceGray /Filter /FlateDecode /Decode [.1 .5] /SMask 5 0 R",
        &fixture::compressed(&[128, 64]),
        "/Matte [.2]",
    );
    std::fs::write(&input, &pdf).unwrap();
    let request = MinimumPdfToDocxRequest {
        input_pdf_path: input,
        output_docx_path: output.clone(),
        ocr_text_lines: None,
        ocr_table: None,
        clean_output: true,
    };
    let report = convert_minimum_pdf_to_docx_with_options(
        &request,
        &ParseOptions {
            strict: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        report.quality.issues,
        [jade_pdf2docx_parser::QualityIssue::OcrRequired { page: 0 }]
    );
    let mut archive = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    let mut images = 0;
    for index in 0..archive.len() {
        let mut part = archive.by_index(index).unwrap();
        if part.is_dir() || !part.name().starts_with("word/media/") {
            continue;
        }
        let mut png = Vec::new();
        part.read_to_end(&mut png).unwrap();
        assert_eq!(
            fixture::scanline(&png),
            [0, 128, 128, 128, 85, 51, 51, 51, 255]
        );
        images += 1;
    }
    assert_eq!(images, 1);
}

/// CCITTの反転済み色と独立した透明度を、実DOCX mediaまで保持する。
#[test]
fn ccitt_inverse_source_decode_preserves_media_and_alpha() {
    use std::io::Read;
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("ccitt.pdf");
    let output = directory.path().join("ccitt.docx");
    let pdf = fixture::source_image_pdf_with_bits(
        "/ColorSpace /DeviceGray /Filter /CCITTFaxDecode /DecodeParms << /K -1 /Columns 2 >> /Decode 8 0 R /SMask 5 0 R",
        &codec::encoded_mask(&[vec![true, false]]),
        "",
        1,
    );
    std::fs::write(&input, &pdf).unwrap();
    let report = convert_minimum_pdf_to_docx_with_options(
        &MinimumPdfToDocxRequest {
            input_pdf_path: input.clone(),
            output_docx_path: output.clone(),
            ocr_text_lines: None,
            ocr_table: None,
            clean_output: true,
        },
        &ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(
        report.quality.issues,
        [jade_pdf2docx_parser::QualityIssue::OcrRequired { page: 0 }]
    );
    let mut archive = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    let mut images = 0;
    for index in 0..archive.len() {
        let mut part = archive.by_index(index).unwrap();
        if !part.is_dir() && part.name().starts_with("word/media/") {
            let mut png = Vec::new();
            part.read_to_end(&mut png).unwrap();
            assert_eq!(
                fixture::scanline(&png),
                [0, 255, 255, 255, 85, 0, 0, 0, 255]
            );
            images += 1;
        }
    }
    assert_eq!(images, 1);
    assert_eq!(std::fs::read(input).unwrap(), pdf);
}

/// stencilのDoごとの色と透明度を実DOCXのmediaで保持する。
#[test]
fn stencil_fill_and_alpha_survive_docx_media() {
    use std::io::Read;
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("stencil.pdf");
    let output = directory.path().join("stencil.docx");
    std::fs::write(
        &input,
        fixture::source_image_pdf_with_content(
            "/ImageMask true /Filter /FlateDecode /Decode [1 0]",
            &fixture::compressed(&[0x40]),
            "",
            1,
            b"q 20 0 0 10 0 0 cm 1 0 0 rg /Im Do 0 1 0 rg /Im Do Q",
        ),
    )
    .unwrap();
    convert_minimum_pdf_to_docx_with_options(
        &MinimumPdfToDocxRequest {
            input_pdf_path: input,
            output_docx_path: output.clone(),
            ocr_text_lines: None,
            ocr_table: None,
            clean_output: true,
        },
        &ParseOptions::default(),
    )
    .unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    let mut pixels = Vec::new();
    for index in 0..archive.len() {
        let mut part = archive.by_index(index).unwrap();
        if !part.is_dir() && part.name().starts_with("word/media/") {
            let mut png = Vec::new();
            part.read_to_end(&mut png).unwrap();
            pixels.push(fixture::scanline(&png));
        }
    }
    pixels.sort();
    let mut expected = vec![
        vec![0, 255, 0, 0, 0, 255, 0, 0, 255],
        vec![0, 0, 255, 0, 0, 0, 255, 0, 255],
    ];
    expected.sort();
    assert_eq!(pixels, expected);
}

/// JBIG2の反転stencilも実DOCXのmediaへ色とalphaを保持する。
#[test]
fn jbig2_stencil_fill_and_alpha_survive_docx_media() {
    use std::io::Read;
    let pdf = fixture::source_image_pdf_with_content(
        "/ImageMask true /Filter /JBIG2Decode /Decode [1 0]",
        &jbig2::encoded_mask(&[vec![true, false]]),
        "",
        1,
        b"q 20 0 0 10 0 0 cm 1 0 0 rg /Im Do Q",
    );
    let bytes =
        jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes("stencil.pdf", &pdf).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut pixels = Vec::new();
    for index in 0..archive.len() {
        let mut part = archive.by_index(index).unwrap();
        if !part.is_dir() && part.name().starts_with("word/media/") {
            let mut png = Vec::new();
            part.read_to_end(&mut png).unwrap();
            pixels.push(fixture::scanline(&png));
        }
    }
    assert_eq!(pixels, [vec![0, 255, 0, 0, 0, 255, 0, 0, 255]]);
}
