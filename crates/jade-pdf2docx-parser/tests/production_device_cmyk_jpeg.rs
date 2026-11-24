//! 所有JPEGのPDF色極性を、公開bytes parserのPNGと独立Poppler色票で照合する。
#[path = "support/source_image_pdf.rs"]
mod fixture;
#[path = "support/owned_device_cmyk_jpeg.rs"]
mod owned;
use jade_pdf2docx_parser::{ParseOptions, QualityIssue, parse_pdf_bytes_with_options};

/// DeviceCMYKの既定Decodeを保ち、JPEGライブラリのstandalone RGB規約へ置き換えない。
fn source_polarity(jpeg: &[u8]) {
    let pdf = fixture::source_image_pdf("/ColorSpace /DeviceCMYK /Filter /DCTDecode", jpeg, "");
    let parsed =
        parse_pdf_bytes_with_options("owned-device-cmyk.pdf", &pdf, &ParseOptions::default())
            .unwrap();
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
    assert!(parsed.text_preview.contains("retained text"));
    assert_eq!(parsed.image_elements.len(), 1);
    let pixels = fixture::scanline(&parsed.image_elements[0].png_bytes);
    assert_eq!(pixels.len(), 7);
    let orange = &pixels[1..4];
    let blue = &pixels[4..7];
    assert!(
        orange[0] > 200
            && (80..=180).contains(&orange[1])
            && orange[2] < 60
            && blue[0] < 60
            && (60..=160).contains(&blue[1])
            && blue[2] > 130,
        "PDF DeviceCMYK JPEG must preserve source polarity: orange={orange:?}, blue={blue:?}"
    );
}

/// Adobe transform0のCMYK JPEGでも、PDFで確認した橙と青を反転させない。
#[test]
fn device_cmyk_jpeg_preserves_pdf_source_polarity() {
    source_polarity(owned::CMYK);
}

/// Adobe transform2のYCCK JPEGでも、PDFで確認した橙と青を反転させない。
#[test]
fn device_ycck_jpeg_preserves_pdf_source_polarity() {
    source_polarity(owned::YCCK);
}
