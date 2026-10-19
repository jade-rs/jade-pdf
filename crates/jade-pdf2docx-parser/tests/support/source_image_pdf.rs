//! Decodeの画素とstrict拒否をparser/DOCX共通の正式xref PDFで検査する。
use std::io::Write;

/// sampleをFlate streamへ圧縮する。
pub fn compressed(samples: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(samples).unwrap();
    encoder.finish().unwrap()
}

/// 正確なLengthを持つstream objectを生成する。
fn stream(attributes: &str, data: &[u8]) -> Vec<u8> {
    let mut bytes = format!("<< {attributes} /Length {} >>\nstream\n", data.len()).into_bytes();
    bytes.extend_from_slice(data);
    bytes.extend_from_slice(b"\nendstream");
    bytes
}

/// 指定した画像属性・圧縮済みsampleと任意Matteの二画素PDFを生成する。
pub fn source_image_pdf(attributes: &str, encoded: &[u8], mask_attributes: &str) -> Vec<u8> {
    source_image_pdf_with_bits(attributes, encoded, mask_attributes, 8)
}

/// 圧縮形式と独立にBitsPerComponentを指定し、1 bit画像も正式xrefへ登録する。
pub fn source_image_pdf_with_bits(
    attributes: &str,
    encoded: &[u8],
    mask_attributes: &str,
    bits: u8,
) -> Vec<u8> {
    source_image_pdf_with_content(
        attributes,
        encoded,
        mask_attributes,
        bits,
        b"q 20 0 0 10 0 0 cm /Im Do Q BT /F1 12 Tf 10 5 Td (retained text) Tj ET",
    )
}

/// 同じ画像を異なる塗り色で描くため、正式xrefのContentsを指定する。
pub fn source_image_pdf_with_content(
    attributes: &str,
    encoded: &[u8],
    mask_attributes: &str,
    bits: u8,
    content: &[u8],
) -> Vec<u8> {
    source_image_pdf_with_dimensions(attributes, encoded, mask_attributes, bits, content, 2, 1)
}

/// packed画像の行境界を検証できるよう、正式xrefの画素寸法を指定する。
pub fn source_image_pdf_with_dimensions(
    attributes: &str,
    encoded: &[u8],
    mask_attributes: &str,
    bits: u8,
    content: &[u8],
    width: u32,
    height: u32,
) -> Vec<u8> {
    assemble(&image_objects(
        attributes,
        encoded,
        mask_attributes,
        bits,
        content,
        width,
        height,
    ))
}

/// 画像・本文・参照scalarのobjectを作り、xref構築と分離する。
fn image_objects(
    attributes: &str,
    encoded: &[u8],
    mask_attributes: &str,
    bits: u8,
    content: &[u8],
    width: u32,
    height: u32,
) -> Vec<Vec<u8>> {
    vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /Im 4 0 R >> /Font << /F1 7 0 R >> >> /Contents 6 0 R >>".to_vec(),
        stream(&format!("/Type /XObject /Subtype /Image /Width {width} /Height {height} /BitsPerComponent {bits} {attributes}"), encoded),
        stream(&format!("/Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /FlateDecode {mask_attributes}"), &compressed(&[85,255])),
        stream("", content),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"[9 0 R 10 0 R]".to_vec(),
        b"1".to_vec(),
        b"0".to_vec(),
        b"null".to_vec(),
    ]
}

/// stencilに直接/間接/継承Resourceと追加objectを与える。
#[allow(dead_code)]
pub fn stencil_with_resources(
    encoded: &[u8],
    content: &[u8],
    resources: &str,
    inherited: bool,
    additional: &[&[u8]],
) -> Vec<u8> {
    image_with_resources(
        "/ImageMask true /Filter /FlateDecode",
        encoded,
        1,
        content,
        resources,
        inherited,
        additional,
    )
}

/// 通常画像とstencilの両方へ、描画状態を解決するResourceを与える。
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub fn image_with_resources(
    attributes: &str,
    encoded: &[u8],
    bits: u8,
    content: &[u8],
    resources: &str,
    inherited: bool,
    additional: &[&[u8]],
) -> Vec<u8> {
    let mut objects = image_objects(attributes, encoded, "", bits, content, 2, 1);
    if inherited {
        objects[1] = format!("<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources {resources} >>")
            .into_bytes();
        objects[2] =
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 6 0 R >>".to_vec();
    } else {
        objects[2] = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources {resources} /Contents 6 0 R >>").into_bytes();
    }
    objects.extend(additional.iter().map(|object| object.to_vec()));
    assemble(&objects)
}

/// 正確なoffsetとSizeを持つ正式xrefを任意object列から構築する。
fn assemble(objects: &[Vec<u8>]) -> Vec<u8> {
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

/// 自前PNGのIDATを独立に展開し、filter byteを含むscanlineを返す。
pub fn scanline(png: &[u8]) -> Vec<u8> {
    use std::io::Read;
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut cursor = 8;
    let mut idat = Vec::new();
    while cursor < png.len() {
        let length = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
        if &png[cursor + 4..cursor + 8] == b"IDAT" {
            idat.extend_from_slice(&png[cursor + 8..cursor + 8 + length]);
        }
        cursor += length + 12;
    }
    let mut bytes = Vec::new();
    flate2::read::ZlibDecoder::new(idat.as_slice())
        .take(64)
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}
