use jade_pdf2docx_parser::{ParseError, parse_pdf_bytes};
use proptest::prelude::*;

/// 不使用XRef streamを保持し、現在のxref tableだけがページを選ぶPDFをメモリー上に作る。
fn document(trailer: &str, orphan_encrypted: bool) -> (Vec<u8>, usize) {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let payload = b"BT /F1 12 Tf 10 10 Td (Allowed) Tj ET";
    let encryption = if orphan_encrypted {
        " /Encrypt 7 0 R"
    } else {
        ""
    };
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        [format!("<< /Length {} >>\nstream\n", payload.len()).into_bytes(), payload.to_vec(), b"\nendstream".to_vec()].concat(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        format!("<< /Type /XRef /Size 8 /W [1 4 2] /Length 0{encryption} >>\nstream\n\nendstream").into_bytes(),
        b"<< /Filter /Standard /V 4 /R 4 /Length 128 >>".to_vec(),
    ];
    let mut offsets = vec![0usize];
    for (index, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 8\n0000000000 65535 f \n");
    for offset in &offsets[1..] {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer << /Size 8 /Root 1 0 R{trailer} >>\nstartxref\n{xref}\n%%EOF\n")
            .as_bytes(),
    );
    (pdf, offsets[6])
}

/// 現行xrefから参照しない過去のXRef辞書は、本文を暗号化済みと誤判定させない。
#[test]
fn unused_xref_encryption_does_not_reject_plain_document() {
    for trailer in ["", " /Encrypt null"] {
        let (pdf, _) = document(trailer, true);
        let parsed = parse_pdf_bytes("plain-with-old-xref.pdf", &pdf);
        assert!(parsed.is_ok(), "{parsed:?}");
        assert!(parsed.unwrap().text_preview.contains("Allowed"));
    }
}

/// 現行trailerの実設定は不使用XRefと区別し、従来どおり暗号化として拒否する。
#[test]
fn active_trailer_encryption_is_still_rejected() {
    let (pdf, _) = document(" /Encrypt 7 0 R", false);
    assert!(matches!(
        parse_pdf_bytes("encrypted.pdf", &pdf),
        Err(ParseError::UnsupportedEncryption { .. })
    ));
}

/// Prev経由で実際に読むXRef streamの暗号化指定は無視しない。
#[test]
fn referenced_xref_encryption_is_still_rejected() {
    let (_, old_offset) = document("", true);
    let (pdf, _) = document(&format!(" /Prev {old_offset}"), true);
    assert!(matches!(
        parse_pdf_bytes("encrypted-prev.pdf", &pdf),
        Err(ParseError::UnsupportedEncryption { .. })
    ));
}

/// 有効なxrefを確認できない旧形式では、暗号化指定を保守的に拒否する。
#[test]
fn unindexed_encryption_is_still_rejected() {
    let (mut pdf, _) = document("", true);
    let at = pdf.windows(5).position(|bytes| bytes == b"xref\n").unwrap();
    pdf.truncate(at);
    pdf.extend_from_slice(b"%%EOF\n");
    assert!(matches!(
        parse_pdf_bytes("legacy.pdf", &pdf),
        Err(ParseError::UnsupportedEncryption { .. })
    ));
}

/// 暗号化というname値や入れ子の同名keyは、現行trailerの設定にはならない。
#[test]
fn names_and_nested_keys_do_not_change_trailer_encryption() {
    let (pdf, _) = document(" /Custom /Encrypt /Nested << /Encrypt 7 0 R >>", false);
    let parsed = parse_pdf_bytes("plain-custom.pdf", &pdf);
    assert!(parsed.is_ok(), "{parsed:?}");
}

proptest! {
    /// header起点のxref offsetと先頭余白でも、不使用の設定は現行Rootを覆わない。
    #[test]
    fn leading_padding_keeps_unused_encryption_inactive(padding in 0usize..64, explicit_null in any::<bool>()) {
        let (pdf, _) = document(if explicit_null { " /Encrypt null" } else { "" }, true);
        let mut prefixed = vec![b' '; padding];
        prefixed.extend_from_slice(&pdf);
        let parsed = parse_pdf_bytes("prefixed-plain.pdf", &prefixed);
        prop_assert!(parsed.is_ok(), "{parsed:?}");
        prop_assert!(parsed.unwrap().text_preview.contains("Allowed"));
    }
}
