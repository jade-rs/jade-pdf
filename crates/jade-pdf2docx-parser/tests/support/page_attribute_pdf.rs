//! 表示属性と resource の検証に使う正式な二ページ PDF。
/// 連番 object と正式な Root/xref を持つ PDF を生成する。
pub fn pdf(objects: &[String]) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    bytes
}

/// 直接 Length を持つ content stream を生成する。
pub fn stream(content: &str) -> String {
    format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// 共通の親属性と別々の描画寸法を持つ二つのページを生成する。
pub fn pages(attributes: &str) -> Vec<String> {
    vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] {attributes} >>"),
        "<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".into(),
        "<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>".into(),
        stream("q 10 0 0 10 0 0 cm /Im Do Q"),
        stream("q 20 0 0 20 0 0 cm /Im Do Q"),
        "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Length 3 >>\nstream\nabc\nendstream".into(),
        "<< /XObject 9 0 R >>".into(),
        "<< /Im 7 0 R >>".into(),
    ]
}
