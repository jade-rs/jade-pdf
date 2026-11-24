//! 所有するPDFの小数point寸法を、PNGの解像度と分離してDrawingMLへ保つ。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use proptest::prelude::*;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// 単一のRGB画素を小数pointの全ページへ描く、外部資産のないPDFを作る。
fn source(width: f32, height: f32) -> Vec<u8> {
    source_with_crop(width, height, None)
}

/// CropBoxも所有するobject生成時に指定し、binary streamとxrefを壊さず組み立てる。
fn source_with_crop(width: f32, height: f32, crop: Option<(f32, f32)>) -> Vec<u8> {
    let paint = format!("q {width} 0 0 {height} 0 0 cm /Im0 Do Q");
    let crop = crop
        .map(|(w, h)| format!(" /CropBox [0 0 {w} {h}]"))
        .unwrap_or_default();
    let objects=vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]{crop} /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>").into_bytes(),
        format!("<< /Length {} >>\nstream\n{paint}\nendstream",paint.len()).into_bytes(),
        [b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length 3 >>\nstream\n".as_slice(), &[16,128,240],b"\nendstream"].concat(),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, obj) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(obj);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

/// 実際のZIP内から、画像の外枠と変換行列が使う二つの寸法を読む。
fn extents(width: f32, height: f32) -> Vec<(u64, u64)> {
    extents_pdf(&source(width, height))
}

/// 原本PDFを公開byte変換へ通し、ZIPから寸法を独立に読む。
fn extents_pdf(pdf: &[u8]) -> Vec<(u64, u64)> {
    let bytes = convert_minimum_pdf_bytes_to_docx_bytes("owned-image.pdf", pdf).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = Vec::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_end(&mut xml)
        .unwrap();
    let mut reader = Reader::from_reader(xml.as_slice());
    let mut out = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            Event::Empty(e)
                if e.name().as_ref() == b"wp:extent" || e.name().as_ref() == b"a:ext" =>
            {
                let attr = |name| {
                    std::str::from_utf8(e.try_get_attribute(name).unwrap().unwrap().value.as_ref())
                        .unwrap()
                        .parse::<u64>()
                        .unwrap()
                };
                out.push((attr(b"cx"), attr(b"cy")));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

/// CropBoxで矩形だけが短くなった場合、未cropのPNGを小さい矩形へ押し込まない。
/// この場合の既存clip配置の忠実度は別課題とし、従来のdisplay寸法を維持する。
#[test]
fn cropped_bounds_do_not_claim_uncropped_payload_physical_extent() {
    let pdf = source_with_crop(150., 210., Some((100., 180.)));
    assert_eq!(
        extents_pdf(&pdf),
        vec![(1_905_000, 2_667_000); 2],
        "clipped bounds must not rescale the uncropped PNG payload"
    );
}

/// 整数pixelにすると高さが0.25pointずれる画像でも、元の物理寸法を使う。
#[test]
fn fractional_pdf_image_extent_does_not_round_to_display_pixels() {
    let expected = (1_590_675, 2_203_450);
    assert_eq!(
        extents(125.25, 173.5),
        vec![expected; 2],
        "source image extent must retain fractional PDF points instead of 96dpi pixels"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 原本の全ページ画像は、縦横を独立してEMUへ丸め、pixel幅へ量子化しない。
    #[test]
    fn source_image_fractional_width_and_height_remain_independent(w in 1_u16..1200,h in 1_u16..1600) {
        let width=f32::from(w)/4.;let height=f32::from(h)/4.;
        let expected=((f64::from(width)*12700.).round() as u64,(f64::from(height)*12700.).round() as u64);
        prop_assert_eq!(extents(width,height),vec![expected;2],"source image extent must retain fractional PDF points instead of 96dpi pixels");
    }
}
