use std::{fs, io::Write};

use flate2::{Compression, write::ZlibEncoder};

use jade_pdf2docx_parser::{ParsedBounds, parse_pdf, parse_pdf_bytes};

mod support;
use support::paths::temp_file_path;
use support::pdf_builders::{
    minimal_pdf_with_inherited_text_font_state, minimal_pdf_with_text,
    minimal_pdf_with_two_text_objects, minimal_pdf_with_two_text_ops,
    minimal_two_page_pdf_with_colliding_font_resource_names,
};

/// 親Pagesから継承した同名font resourceが別objectでもCMapとfont名を混同しない。
#[test]
fn parse_pdf_scopes_colliding_font_resource_names_to_each_page() {
    let bytes = minimal_two_page_pdf_with_colliding_font_resource_names();
    let parsed = parse_pdf_bytes("colliding-page-font-resources.pdf", bytes.as_bytes())
        .expect("two-page PDF must parse");

    let page_text_and_fonts = parsed
        .text_elements
        .iter()
        .map(|text| (text.page, text.text.as_str(), text.font_name.as_deref()))
        .collect::<Vec<_>>();
    assert_eq!(
        page_text_and_fonts,
        [(0, "A", Some("PageOneFont")), (1, "B", Some("PageTwoFont")),],
        "same /F1 name must resolve through each page's inherited resource dictionary"
    );
}

/// 空text objectで指定されたfont名とsizeを次のtext objectへ引き継ぐ。
#[test]
fn parse_pdf_preserves_font_state_across_text_objects() {
    let bytes = minimal_pdf_with_inherited_text_font_state("Inherited state");
    let parsed = parse_pdf_bytes("inherited-text-font-state.pdf", bytes.as_bytes())
        .expect("font-state PDF must parse");

    assert_eq!(parsed.text_elements.len(), 1);
    let text = &parsed.text_elements[0];
    assert_eq!(text.text, "Inherited state");
    assert_eq!(text.font_name.as_deref(), Some("Helvetica"));
    assert_eq!(text.font_size, 24.0);
}

#[test]
fn parse_pdf_places_image_xobject_from_cm_matrix() {
    let temp_file = temp_file_path("image_xobject_cm_test");
    let pdf_bytes = minimal_pdf_with_flate_image_content(
        b"q 50 0 0 40 20 30 cm /Im1 Do Q",
        b"\xff\x00\x00\x00\x00\xff",
        2,
        1,
    );
    fs::write(&temp_file, &pdf_bytes).expect("failed to create temp pdf");

    let parsed = parse_pdf_bytes(temp_file.clone(), &pdf_bytes).expect("pdf with image must parse");
    assert_eq!(parsed.image_elements.len(), 1);
    assert_eq!(
        rounded_bounds(parsed.image_elements[0].bounds),
        (20, 30, 70, 70)
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_rejects_non_pdf_header() {
    let temp_file = temp_file_path("not_pdf_header_test");
    fs::write(&temp_file, b"HELLO-WORLD").expect("failed to create temp file");

    let result = parse_pdf(&temp_file);
    assert!(result.is_err(), "non-pdf input must be rejected");

    let _ = fs::remove_file(temp_file);
}

fn rounded_bounds(bounds: ParsedBounds) -> (i32, i32, i32, i32) {
    (
        bounds.x0.round() as i32,
        bounds.y0.round() as i32,
        bounds.x1.round() as i32,
        bounds.y1.round() as i32,
    )
}

fn minimal_pdf_with_flate_content_stream(stream_contents: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(stream_contents)
        .expect("zlib write must succeed");
    let compressed = encoder.finish().expect("zlib finish must succeed");
    let mut pdf = Vec::new();
    pdf.extend_from_slice(
        b"%PDF-1.4\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj\n",
    );
    pdf.extend_from_slice(
        format!(
            "4 0 obj << /Length {} /Filter /FlateDecode >> stream\n",
            compressed.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&compressed);
    pdf.extend_from_slice(
        b"\nendstream endobj\n\
5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n\
%%EOF\n",
    );
    pdf
}

fn minimal_pdf_with_flate_image_content(
    content_stream: &[u8],
    rgb: &[u8],
    width: u32,
    height: u32,
) -> Vec<u8> {
    let mut content_encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    content_encoder
        .write_all(content_stream)
        .expect("content zlib write must succeed");
    let content = content_encoder
        .finish()
        .expect("content zlib finish must succeed");
    let mut image_encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    image_encoder
        .write_all(rgb)
        .expect("image zlib write must succeed");
    let image = image_encoder
        .finish()
        .expect("image zlib finish must succeed");

    let mut pdf = Vec::new();
    pdf.extend_from_slice(
        b"%PDF-1.4\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 5 0 R >> >> /Contents 4 0 R >> endobj\n",
    );
    pdf.extend_from_slice(
        format!(
            "4 0 obj << /Length {} /Filter /FlateDecode >> stream\n",
            content.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&content);
    pdf.extend_from_slice(b"\nendstream endobj\n");
    pdf.extend_from_slice(
        format!(
            "5 0 obj << /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >> stream\n",
            image.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&image);
    pdf.extend_from_slice(b"\nendstream endobj\n%%EOF\n");
    pdf
}

#[test]
fn parse_pdf_rejects_invalid_version_header() {
    let temp_file = temp_file_path("invalid_pdf_version_test");
    fs::write(&temp_file, b"%PDF-A.B\n1 0 obj").expect("failed to create temp file");

    let result = parse_pdf(&temp_file);
    assert!(result.is_err(), "invalid version must be rejected");

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_extracts_minimum_text_preview() {
    let temp_file = temp_file_path("minimum_text_preview_test");
    fs::write(&temp_file, minimal_pdf_with_text("Hello TDD"))
        .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("Hello TDD"),
        "text preview must include literal string"
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_extracts_text_preview_from_flate_content_stream() {
    let temp_file = temp_file_path("compressed_text_preview_test");
    let pdf_bytes =
        minimal_pdf_with_flate_content_stream(b"BT /F1 12 Tf 10 100 Td (Compressed Text) Tj ET");
    fs::write(&temp_file, &pdf_bytes).expect("failed to write compressed pdf");

    let parsed =
        parse_pdf_bytes(temp_file.clone(), &pdf_bytes).expect("compressed pdf must be parsed");
    assert!(
        parsed.text_preview.contains("Compressed Text"),
        "text preview must include text from FlateDecode stream: {:?}",
        parsed.text_preview
    );

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_deduplicates_same_literal_lines() {
    let temp_file = temp_file_path("dedupe_text_preview_test");
    fs::write(
        &temp_file,
        minimal_pdf_with_two_text_ops("SameLine", "SameLine"),
    )
    .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    let line_count = parsed
        .text_preview
        .lines()
        .filter(|line| *line == "SameLine")
        .count();
    assert_eq!(line_count, 1, "same line should be deduplicated");

    let _ = fs::remove_file(temp_file);
}

#[test]
fn parse_pdf_separates_multiple_text_objects_by_newline() {
    let temp_file = temp_file_path("multi_text_object_preview_test");
    fs::write(
        &temp_file,
        minimal_pdf_with_two_text_objects("FirstLine", "SecondLine"),
    )
    .expect("failed to create minimal pdf");

    let parsed = parse_pdf(&temp_file).expect("minimal pdf must be parsed");
    assert!(
        parsed.text_preview.contains("FirstLine\nSecondLine"),
        "multiple text objects should be split by newline in preview"
    );

    let _ = fs::remove_file(temp_file);
}
