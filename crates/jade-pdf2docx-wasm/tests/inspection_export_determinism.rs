//! 公開WASM APIのプレビューで表を検査しても、後のDOCX生成を変えない。
use jade_pdf2docx_wasm::{convert_pdf_to_docx, convert_pdf_to_docx_strict, inspect_pdf_json};
use wasm_bindgen::JsValue;

/// 横結合した見出しと同期する本文三行を持ち、外部資産を使わない所有原本。
fn source() -> Vec<u8> {
    let mut content = String::from(
        "1 w 72 575 m 272 575 l S 72 670 m 272 670 l S 72 700 m 272 700 l S 72 575 m 72 700 l S 172 575 m 172 670 l S 272 575 m 272 700 l S BT /F1 10 Tf 82 680 Td (Summary) Tj ET",
    );
    for index in 0..3 {
        let y = 650 - index * 25;
        content.push_str(&format!(" BT /F1 10 Tf 82 {y} Td (Label-{index}) Tj ET BT /F1 10 Tf 182 {y} Td (Value-{index}) Tj ET"));
    }
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
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

/// 同じ原本を何回検査しても、同じexport入口の出力byteを変えないことを確認する。
fn check(convert: impl Fn(&[u8]) -> Result<Vec<u8>, JsValue>) {
    let bytes = source();
    let direct = convert(&bytes).unwrap();
    let preview = inspect_pdf_json(&bytes).unwrap();
    assert!(preview.contains("\"inferred_table_rows\":4"));
    assert_eq!(inspect_pdf_json(&bytes).unwrap(), preview);
    let after_inspection = convert(&bytes).unwrap();
    assert!(
        after_inspection == direct,
        "read-only layout inspection must not advance generated DOCX paragraph IDs"
    );
}

/// default変換の前にプレビューしても、段落IDを含め同じDOCXになる。
#[test]
fn preview_does_not_change_default_export_bytes() {
    check(convert_pdf_to_docx);
}

/// strict変換にも同じ検査と出力の分離を適用する。
#[test]
fn preview_does_not_change_strict_export_bytes() {
    check(convert_pdf_to_docx_strict);
}
