/// 保存fixtureを使わず、独立した可視本文と非描画文字を持つ正式xref付きPDFを生成する。
pub fn source(mode: u8, recorded: &str) -> Vec<u8> {
    let content = format!("BT /F1 18 Tf 20 130 Td (Visible 1/8) Tj ET\nq BT /F1 18 Tf {mode} Tr 20 70 Td ({recorded}) Tj ET Q");
    document(content)
}

/// 著者が全ての文字を空のActualTextへ置き換えたowned入力を生成する。
#[cfg(not(target_arch = "wasm32"))]
pub fn empty_actualtext_source() -> Vec<u8> {
    document("/Span << /ActualText () >> BDC BT /F1 18 Tf 3 Tr 20 70 Td (Hidden) Tj ET EMC".into())
}

/// 固定5objectの内容streamへxrefを付ける。通常のmode入力のbytesは変えない。
fn document(content: String) -> Vec<u8> {
    let objects=["<< /Type /Catalog /Pages 2 0 R >>".into(),"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),format!("<< /Length {} >>\nstream\n{content}\nendstream",content.len()),"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into()];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
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
