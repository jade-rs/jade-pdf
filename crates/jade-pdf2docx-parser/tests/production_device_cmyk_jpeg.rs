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

/// 四成分のDecodeとMatteをRGB化より前に扱い、透明度の指定も保持する。
#[test]
fn device_cmyk_jpeg_matte_is_applied_in_source_components() {
    let pdf = fixture::source_image_pdf(
        "/ColorSpace /DeviceCMYK /Filter /DCTDecode /SMask 5 0 R",
        owned::CMYK,
        "/Matte [0 0 0 0]",
    );
    let parsed =
        parse_pdf_bytes_with_options("owned-cmyk-matte.pdf", &pdf, &ParseOptions::default())
            .unwrap();
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }],
        "DeviceCMYK with verified source Matte must decode"
    );
    let pixels = fixture::scanline(&parsed.image_elements[0].png_bytes);
    assert_eq!(pixels.len(), 9);
    assert!(pixels[1] > 240 && pixels[2] < 20 && pixels[3] < 20);
    assert_eq!(pixels[4], 85);
    assert!(pixels[5] < 60 && (60..=160).contains(&pixels[6]) && pixels[7] > 130);
    assert_eq!(pixels[8], 255);
}
