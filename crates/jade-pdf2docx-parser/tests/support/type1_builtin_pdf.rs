/// 正確なLengthを持つ、所有の非圧縮メタデータstreamを作る。
pub fn stream(data: &str, attributes: &str) -> Vec<u8> {
    format!(
        "<< /Length {} {attributes} >>\nstream\n{data}\nendstream",
        data.len()
    )
    .into_bytes()
}

/// 埋め込みType1のEncodingだけを持つメタデータcontrolを作り、描画字体の妥当性とは分離する。
pub fn program(encoding: &str) -> String {
    format!(
        "%!PS-AdobeFont-1.0: OwnedBuiltin 1.0\n11 dict begin\n/FontName /OwnedBuiltin def\n/FontType 1 def\n/FontBBox [0 0 600 700] def\n/FontMatrix [0.001 0 0 0.001 0 0] def\n/Encoding {encoding} readonly def\ncurrentdict end\ncurrentfile eexec\n"
    )
}

/// PDF側Encoding・ToUnicodeと、描画するfontを独立に指定できる正式xref付きPDF。
pub fn objects(
    builtin: &str,
    encoding: &str,
    unicode: bool,
    selected: &str,
    text: &str,
) -> Vec<Vec<u8>> {
    let font_program = program(builtin);
    let mapping = if unicode { "/ToUnicode 8 0 R" } else { "" };
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R /F2 9 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream(&format!("BT /{selected} 12 Tf 72 700 Td ({text}) Tj ET"), ""),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /OwnedBuiltin /FontDescriptor 6 0 R {encoding} {mapping} >>").into_bytes(),
        b"<< /Type /FontDescriptor /FontName /OwnedBuiltin /Flags 32 /FontBBox [0 0 600 700] /Ascent 700 /Descent -200 /CapHeight 700 /StemV 80 /FontFile 7 0 R >>".to_vec(),
        stream(&font_program, &format!("/Length1 {} /Length2 0 /Length3 0", font_program.len())),
        stream("begincmap\n1 begincodespacerange\n<00> <ff>\nendcodespacerange\n1 beginbfchar\n<41> <005a>\nendbfchar\nendcmap", ""),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
    ];
    objects
}

/// PDF側Encoding・ToUnicodeと、描画するfontを独立に指定できる正式xref付きPDF。
pub fn serialize(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut data = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        data.extend_from_slice(object);
        data.extend_from_slice(b"\nendobj\n");
    }
    let xref = data.len();
    data.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        data.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    data.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    data
}

/// PDF側Encoding・ToUnicodeと、描画するfontを独立に指定できる正式xref付きPDF。
pub fn pdf(builtin: &str, encoding: &str, unicode: bool, selected: &str, text: &str) -> Vec<u8> {
    serialize(&objects(builtin, encoding, unicode, selected, text))
}
