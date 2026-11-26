//! pageと画像CTMの回転が相殺される場合、表示矩形と画像の向きを二重に回転しない。
#[allow(dead_code)]
#[path = "support/source_image_pdf.rs"]
mod fixture;
use jade_pdf2docx_parser::{ParsedBounds, parse_pdf_bytes};

/// 元の二画素と正式xrefを持ち、非正方形ページと画像CTMの回転を独立に指定する。
fn source(rotation: i32, matrix: &str) -> Vec<u8> {
    let samples = fixture::compressed(&[255, 0, 0, 0, 255, 0]);
    let content = format!("q {matrix} cm /Im Do Q BT /F1 12 Tf 10 10 Td (retained text) Tj ET");
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Rotate {rotation} /Resources << /XObject << /Im 4 0 R >> /Font << /F1 6 0 R >> >> /Contents 5 0 R >>").into_bytes(),
        stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode", &samples),
        stream("", content.as_bytes()),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

/// 画像sampleを変更せず、正確なLengthのstreamを作る。
fn stream(attributes: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {attributes} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// 独立に求めた表示四隅と、相殺後の水平な赤・緑の二画素を保持する。
#[test]
fn page_and_exact_image_rotations_cancel_without_swapping_the_source_pixels() {
    for (rotation, matrix, expected) in [
        (90, "0 60 -30 0 80 20", [20., 120., 80., 150.]),
        (180, "-60 0 0 -30 100 80", [100., 220., 160., 250.]),
        (270, "0 -60 30 0 20 100", [200., 20., 260., 50.]),
    ] {
        let parsed =
            parse_pdf_bytes("owned-cancelled-image.pdf", &source(rotation, matrix)).unwrap();
        assert_eq!(parsed.image_elements.len(), 1);
        let image = &parsed.image_elements[0];
        assert_eq!(
            image.bounds,
            ParsedBounds {
                x0: expected[0],
                y0: expected[1],
                x1: expected[2],
                y1: expected[3]
            }
        );
        assert_eq!(image.rotation_degrees, 0, "page rotation {rotation}");
        assert_eq!(
            (image.width_px, image.height_px),
            (80, 40),
            "page rotation {rotation}"
        );
        assert_eq!(
            fixture::scanline(&image.png_bytes),
            [0, 255, 0, 0, 0, 255, 0]
        );
    }
}
