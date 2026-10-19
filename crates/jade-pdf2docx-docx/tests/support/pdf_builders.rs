#![allow(dead_code)]

/// 正確な Length・xref offset を持つ単一ページ PDF を作る。
fn pdf(contents: &str, font: bool) -> String {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R {} >>",
            if font {
                "/Resources << /Font << /F1 5 0 R >> >>"
            } else {
                ""
            }
        ),
        format!(
            "<< /Length {} >>\nstream\n{contents}\nendstream",
            contents.len()
        ),
    ];
    if font {
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned());
    }
    let mut source = "%PDF-1.4\n".to_owned();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = source.len();
    source.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        source.push_str(&format!("{offset:010} 00000 n \n"));
    }
    source.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    source
}

/// 1 text object を含む PDF を生成する。
pub fn minimal_pdf_with_text(text: &str) -> String {
    pdf(&format!("BT /F1 12 Tf 10 100 Td ({text}) Tj ET"), true)
}

/// text を含まない PDF を生成する。
pub fn minimal_pdf_without_text() -> String {
    pdf("q Q", false)
}

/// 独立した 2 text object を含む PDF を生成する。
pub fn minimal_pdf_with_two_text_objects(first: &str, second: &str) -> String {
    pdf(
        &format!("BT /F1 12 Tf 10 120 Td ({first}) Tj ET\nBT /F1 12 Tf 10 100 Td ({second}) Tj ET"),
        true,
    )
}
