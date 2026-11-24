//! Form 内の画像を局所辞書・合成行列・切り抜き付きで保つ回帰。
use super::*;
use std::io::Write;

#[path = "form_image_invariants.rs"]
mod invariants;

#[path = "form_content_ascii85_tests.rs"]
mod ascii85_content;

/// 所有する本文から長さが一致する stream を作る。
fn stream(dictionary: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dictionary} /Length {} >>\nstream\n", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\nendstream");
    out
}

/// 赤い一画素と局所名Imを持つ、編集可能な所有object列を返す。
fn objects(page: &str, form: &str, attributes: &str) -> Vec<Vec<u8>> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&[255, 0, 0]).unwrap();
    let pixels = encoder.finish().unwrap();
    vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /XObject << /Fm 5 0 R /Unused 6 0 R >> /ExtGState << /Half << /ca .5 >> >> >> /Contents 4 0 R >>".to_vec(),
        stream("", page.as_bytes()),
        stream(&format!("/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Matrix [2 0 0 3 4 5] /Resources << /XObject << /Im 6 0 R >> >> {attributes}"), form.as_bytes()),
        stream("/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode", &pixels),
    ]
}

/// 参照を変えてもxrefとLengthを正しく再生成する。
fn document(objects: &[Vec<u8>]) -> Vec<u8> {
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

/// 通常のForm画像入力を所有object列から組み立てる。
fn fixture(page: &str, form: &str, attributes: &str) -> Vec<u8> {
    document(&objects(page, form, attributes))
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

/// グループ単位の合成を画像ごとのalphaで代用し、検証済みと扱わない。
#[test]
fn unsupported_group_compositing_keeps_replacement_diagnostic() {
    let pdf = fixture(
        "/Half gs /Fm Do",
        "8 0 0 6 1 2 cm /Im Do",
        "/Group << /S /Transparency >>",
    );
    let parsed = parse_pdf_bytes("owned-group-alpha.pdf", &pdf).unwrap();
    assert_eq!(parsed.image_elements.len(), 1);
    assert!(
        parsed.quality.issues.iter().any(|issue| matches!(
            issue,
            QualityIssue::ImageReplaced {
                object_number: Some(6),
                ..
            }
        )),
        "group compositing must not be guessed as per-image opacity"
    );
}

/// 間接Subtypeも検証済みのFormとして探索し、直接名の最適化で読み飛ばさない。
#[test]
fn indirect_form_subtype_is_not_skipped() {
    let mut objects = objects("/Fm Do", "8 0 0 6 1 2 cm /Im Do", "");
    objects[4] = stream(
        "/Type /XObject /Subtype 7 0 R /BBox [0 0 10 10] /Resources << /XObject << /Im 6 0 R >> >>",
        b"8 0 0 6 1 2 cm /Im Do",
    );
    objects.push(b"/Form".to_vec());
    let parsed = parse_pdf_bytes("owned-indirect-form.pdf", &document(&objects)).unwrap();
    assert_eq!(
        parsed.image_elements.len(),
        1,
        "indirect Form subtype must retain its image"
    );
}

/// 有限で小さい画像の行列を特異行列と誤認し、他の画像まで捨てない。
#[test]
fn tiny_partial_image_is_not_discarded_as_singular() {
    let pdf = fixture(
        "/Fm Do",
        "0 0 .00005 .00005 re W n .0001 0 0 .0001 0 0 cm /Im Do",
        "",
    );
    let parsed = parse_pdf_bytes("owned-small-crop.pdf", &pdf).unwrap();
    assert_eq!(
        parsed.image_elements.len(),
        1,
        "finite nonzero clipped image must survive"
    );
}

/// RGB画素を変換せず、Grayのグループ色空間を再現できたと扱わない。
#[test]
fn group_color_conversion_is_not_bypassed() {
    let pdf = fixture(
        "/Fm Do",
        "8 0 0 6 1 2 cm /Im Do",
        "/Group << /S /Transparency /CS /DeviceGray >>",
    );
    let parsed = parse_pdf_bytes("owned-gray-group.pdf", &pdf).unwrap();
    assert!(
        parsed.quality.issues.iter().any(|issue| matches!(
            issue,
            QualityIssue::ImageReplaced {
                object_number: Some(6),
                ..
            }
        )),
        "unimplemented group color conversion must be diagnosed"
    );
}

/// 表示寸法が小さくても、切り抜き前の元画像の画素密度を保つ。
#[test]
fn clipped_image_preserves_source_pixel_density() {
    let mut objects = objects("/Fm Do", "0 0 .5 1 re W n /Im Do", "");
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&[255, 0, 0].repeat(100 * 80)).unwrap();
    objects[5] = stream(
        "/Type /XObject /Subtype /Image /Width 100 /Height 80 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode",
        &encoder.finish().unwrap(),
    );
    let parsed = parse_pdf_bytes("owned-high-density-crop.pdf", &document(&objects)).unwrap();
    let pixels = decode_filter0_png_to_rgba(
        &DecodeBudget::default(),
        &parsed.image_elements[0].png_bytes,
    )
    .unwrap();
    assert_eq!(
        (pixels.width, pixels.height),
        (50, 80),
        "clipped bitmap must preserve source pixel density"
    );
}

/// Form内の画像は、本文inlineの順序へ置き換えず局所配置を保持する。
#[test]
fn form_image_without_crop_still_uses_floating_placement() {
    let parsed = parse_pdf_bytes(
        "owned-positioned-form-image.pdf",
        &fixture("/Fm Do", "/Im Do", ""),
    )
    .unwrap();
    assert!(
        parsed.image_elements[0].floating,
        "Form image must preserve positioned placement"
    );
}
