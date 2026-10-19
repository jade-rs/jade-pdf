//! 描画画像と補助mask・未描画resourceを区別する公開API fixture。
use std::io::Write;

/// 小さい画像の画素列をFlateで圧縮する。
fn compressed(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// 正確なLengthを持つstreamを生成する。
fn stream(attributes: &str, data: &[u8]) -> Vec<u8> {
    let mut result = format!("<< {attributes} /Length {} >>\nstream\n", data.len()).into_bytes();
    result.extend_from_slice(data);
    result.extend_from_slice(b"\nendstream");
    result
}

/// 可視画像数と未描画画像数を独立に指定し、各可視画像へ任意でSMaskを付ける。
pub fn image_count_pdf(visible: usize, unpainted: usize, masks: bool) -> Vec<u8> {
    let mask_count = if masks { visible } else { 0 };
    let content_number = 4 + visible + mask_count + unpainted;
    let font_number = content_number + 1;
    let mut resources = String::new();
    let mut content = Vec::new();
    for index in 0..visible {
        resources.push_str(&format!("/Im{index} {} 0 R ", index + 4));
        content.extend_from_slice(format!("q 2 0 0 1 {index} 0 cm /Im{index} Do Q\n").as_bytes());
    }
    for index in 0..unpainted {
        resources.push_str(&format!(
            "/Unused{index} {} 0 R ",
            4 + visible + mask_count + index
        ));
    }
    // 枚数制限の検査にOCR診断を混在させず、可視画像全体へ文字レイヤを重ねる。
    content.extend_from_slice(b"BT /F1 12 Tf 0 .5 Td (retained text) Tj ET");
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << {resources} >> /Font << /F1 {font_number} 0 R >> >> /Contents {content_number} 0 R >>").into_bytes(),
    ];
    let rgb = compressed(&[10, 20, 30, 40, 50, 60]);
    let alpha = compressed(&[0, 77]);
    for index in 0..visible {
        let mask = if masks {
            format!(" /SMask {} 0 R", 4 + visible + index)
        } else {
            String::new()
        };
        objects.push(stream(&format!("/Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode{mask}"), &rgb));
    }
    for _ in 0..mask_count {
        objects.push(stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /FlateDecode", &alpha));
    }
    for _ in 0..unpainted {
        objects.push(stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode", &rgb));
    }
    objects.push(stream("", &content));
    objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let size = objects.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer << /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}
