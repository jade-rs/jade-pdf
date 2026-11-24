//! Form 内の画像を局所辞書・合成行列・切り抜き付きで保つ回帰。
use super::*;
use std::io::Write;

/// 所有する本文から長さが一致する stream を作る。
fn stream(dictionary: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dictionary} /Length {} >>\nstream\n", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\nendstream");
    out
}

/// 赤い一画素と局所名 Im を持つ Form を、正規のページ木から二回描く。
fn fixture(page: &str, form: &str, attributes: &str) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&[255, 0, 0]).unwrap();
    let pixels = encoder.finish().unwrap();
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /XObject << /Fm 5 0 R /Unused 6 0 R >> /ExtGState << /Half << /ca .5 >> >> >> /Contents 4 0 R >>".to_vec(),
        stream("", page.as_bytes()),
        stream(&format!("/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Matrix [2 0 0 3 4 5] /Resources << /XObject << /Im 6 0 R >> >> {attributes}"), form.as_bytes()),
        stream("/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode", &pixels),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in &offsets[1..] {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Root 1 0 R /Size {} >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    out
}

/// 未描画のページ別名だけで、Form 内の実描画を除外しない。
#[test]
fn nested_image_uses_local_resources_and_each_composed_matrix() {
    let pdf = fixture(
        "q 1 0 0 1 10 20 cm /Fm Do Q q 1 0 0 1 100 120 cm /Fm Do Q",
        "8 0 0 6 1 2 cm /Im Do",
        "",
    );
    let parsed = parse_pdf_bytes("owned-form-image.pdf", &pdf).unwrap();
    assert_eq!(
        parsed.image_elements.len(),
        2,
        "called Form must retain its nested Image XObject"
    );
    for (image, bounds) in parsed
        .image_elements
        .iter()
        .zip([[16., 31., 32., 49.], [106., 131., 122., 149.]])
    {
        assert_eq!(
            image.bounds,
            ParsedBounds {
                x0: bounds[0],
                y0: bounds[1],
                x1: bounds[2],
                y1: bounds[3]
            }
        );
        let pixels =
            decode_filter0_png_to_rgba(&DecodeBudget::default(), &image.png_bytes).unwrap();
        assert_eq!(pixels.rgba, [255, 0, 0, 255]);
    }
    assert!(parsed.quality.issues.iter().any(|issue| matches!(
        issue,
        QualityIssue::UnsupportedXObject {
            reason: crate::UnsupportedXObjectReason::Form,
            ..
        }
    )));
    assert!(
        parsed
            .quality
            .issues
            .iter()
            .any(|issue| matches!(issue, QualityIssue::OcrRequired { page: 0 }))
    );
}

/// Form BBox と本文の凸 clip の交差だけを画像として残す。
#[test]
fn nested_image_is_cropped_to_form_bbox_and_local_clip() {
    let pdf = fixture("/Fm Do", "2 0 6 10 re W n 20 0 0 20 -5 -5 cm /Im Do", "");
    let parsed = parse_pdf_bytes("owned-form-crop.pdf", &pdf).unwrap();
    assert_eq!(
        parsed.image_elements.len(),
        1,
        "called Form must retain its nested Image XObject"
    );
    assert_eq!(
        parsed.image_elements[0].bounds,
        ParsedBounds {
            x0: 8.,
            y0: 5.,
            x1: 20.,
            y1: 35.
        }
    );
}

/// 呼出側の alpha を継承し、Form 内 q/Q の変更を親へ漏らさない。
#[test]
fn nested_image_inherits_parent_opacity() {
    let pdf = fixture("/Half gs /Fm Do", "8 0 0 6 1 2 cm /Im Do", "");
    let parsed = parse_pdf_bytes("owned-form-alpha.pdf", &pdf).unwrap();
    assert_eq!(
        parsed.image_elements.len(),
        1,
        "called Form must retain its nested Image XObject"
    );
    let pixels = decode_filter0_png_to_rgba(
        &DecodeBudget::default(),
        &parsed.image_elements[0].png_bytes,
    )
    .unwrap();
    assert_eq!(pixels.rgba, [255, 0, 0, 128]);
}
