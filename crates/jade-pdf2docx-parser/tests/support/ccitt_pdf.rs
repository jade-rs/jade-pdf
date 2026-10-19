//! 行数の不足を持つCCITT画像と本文を、正式xref付きPDFで検証する。
use fax::{BitWriter, Bits, Color, VecWriter};

/// 3画素の白黒行をGroup 3またはGroup 4で符号化する。
pub fn encoded_rows(group_four: bool, rows: usize) -> Vec<u8> {
    if group_four {
        let mut encoder = fax::encoder::Encoder::new(VecWriter::new());
        for row in 0..rows {
            let color = if row % 2 == 0 {
                Color::Black
            } else {
                Color::White
            };
            encoder.encode_line([color; 3].into_iter(), 3).unwrap();
        }
        return encoder.finish().unwrap().finish();
    }
    let mut writer = VecWriter::new();
    writer.write(fax::maps::EOL).unwrap();
    for row in 0..rows {
        if row % 2 == 0 {
            writer
                .write(Bits {
                    data: 0b00110101,
                    len: 8,
                })
                .unwrap();
            writer.write(Bits { data: 0b10, len: 2 }).unwrap();
        } else {
            writer
                .write(Bits {
                    data: 0b1000,
                    len: 4,
                })
                .unwrap();
        }
        writer.write(fax::maps::EOL).unwrap();
    }
    for _ in 0..5 {
        writer.write(fax::maps::EOL).unwrap();
    }
    writer.finish()
}

/// 正確なLengthを持つstreamを生成する。
fn stream(attributes: &str, data: &[u8]) -> Vec<u8> {
    let mut bytes = format!("<< {attributes} /Length {} >>\nstream\n", data.len()).into_bytes();
    bytes.extend_from_slice(data);
    bytes.extend_from_slice(b"\nendstream");
    bytes
}

/// 可視画像の宣言行数と実データを独立に指定する。
pub fn ccitt_pdf(group_four: bool, declared_rows: usize, data: &[u8]) -> Vec<u8> {
    let k = if group_four { -1 } else { 0 };
    ccitt_pdf_with_parameters(declared_rows, data, &format!("/K {k}"))
}

/// 行数と追加DecodeParmsを指定し、xref offsetを再計算する。
pub fn ccitt_pdf_with_parameters(declared_rows: usize, data: &[u8], parameters: &str) -> Vec<u8> {
    ccitt_pdf_with_dimensions(
        3,
        declared_rows,
        data,
        &format!("{parameters} /Columns 3 /Rows {declared_rows}"),
    )
}

/// 画像寸法と完全なDecodeParmsを独立に指定し、省略値も検査する。
pub fn ccitt_pdf_with_dimensions(
    width: usize,
    height: usize,
    data: &[u8],
    parameters: &str,
) -> Vec<u8> {
    ccitt_pdf_with_objects(width, height, data, parameters, &[])
}

/// DecodeParmsが参照する追加objectを正式xrefへ登録する。
pub fn ccitt_pdf_with_objects(
    width: usize,
    height: usize,
    data: &[u8],
    parameters: &str,
    extra_objects: &[&[u8]],
) -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Im 4 0 R >> /Font << /F1 6 0 R >> >> /Contents 5 0 R >>".to_vec(),
        stream(&format!("/Type /XObject /Subtype /Image /Width {width} /Height {height} /BitsPerComponent 1 /ColorSpace /DeviceGray /Filter /CCITTFaxDecode /DecodeParms << {parameters} >>"), data),
        // 圧縮形式・復号画素の検査をOCR診断から分離するため文字を画像へ重ねる。
        stream("", b"q 20 0 0 10 0 0 cm /Im Do Q BT /F1 12 Tf 10 5 Td (retained text) Tj ET"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    objects.extend(extra_objects.iter().map(|object| object.to_vec()));
    let object_count = objects.len() + 1;
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {object_count}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer << /Size {object_count} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n")
            .as_bytes(),
    );
    pdf
}
