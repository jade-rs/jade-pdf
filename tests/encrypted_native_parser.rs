#![cfg(not(target_arch = "wasm32"))]

use jade_pdf2docx_parser::{
    ParseError, ParseLimits, parse_pdf, parse_pdf_bytes, parse_pdf_with_limits,
};
use std::{fs, path::Path};

/// 読取対象を固定hashと照合し、入力byteを後から変更していないことを検証する。
fn verified(path: &Path, digest: &str) -> Vec<u8> {
    let bytes = fs::read(path).expect("audited encrypted input must exist");
    let mut hash = hmac_sha256::Hash::new();
    hash.update(&bytes);
    let actual: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(actual, digest.trim(), "input SHA-256 changed");
    bytes
}

/// 原本を復号保存せず、空パスワード・コピー許可付きの公開PDFをnative解析へ渡す。
#[test]
#[ignore = "requires the hash-pinned external corpus and native Pdfium"]
fn public_copy_permitted_encrypted_source_opens_natively() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corpus/cache/nta-invoice-domestic-english-2026.pdf");
    let bytes = verified(
        &path,
        "b59b930ddeebd71a50aac74417af7ebf37521f26d5b58094e58a5d7af90652f0",
    );
    assert!(matches!(
        parse_pdf_bytes("encrypted-bytes.pdf", &bytes),
        Err(ParseError::UnsupportedEncryption { .. })
    ));
    let parsed = parse_pdf(&path)
        .expect("native file parser must open copy-permitted empty-user-password PDF");
    assert_eq!(parsed.page_count, 5);
    assert_eq!(parsed.pages.len(), 5);
    assert_eq!(
        parsed.byte_len,
        bytes.len(),
        "report must retain original input length"
    );
    assert!(
        parsed.image_elements.len() >= 2,
        "both scanned form pages must be retained"
    );
    let limited = ParseLimits {
        max_input_bytes: bytes.len() as u64,
        ..Default::default()
    };
    assert!(matches!(
        parse_pdf_with_limits(&path, &limited),
        Err(ParseError::ResourceLimit {
            resource: "normalized input bytes",
            ..
        })
    ));
    let limited = ParseLimits {
        max_pages: 0,
        ..Default::default()
    };
    assert!(matches!(
        parse_pdf_with_limits(&path, &limited),
        Err(ParseError::ResourceLimit {
            resource: "page count",
            ..
        })
    ));
    assert_eq!(
        fs::read(&path).unwrap(),
        bytes,
        "original encrypted PDF changed"
    );

    // 同じ申請書の日本語3ページ版でも、原本と画像本文の未検証診断を維持する。
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("corpus/cache/nta-invoice-domestic-case2-2026.pdf");
    let bytes = verified(
        &path,
        "7980238f8eb424fb6af5716c404a7c5942636af91f7df76603918f56942b4bc9",
    );
    assert!(matches!(
        parse_pdf_bytes("encrypted-japanese-bytes.pdf", &bytes),
        Err(ParseError::UnsupportedEncryption { .. })
    ));
    let parsed = parse_pdf(&path).expect("copy-permitted Japanese source must open natively");
    assert_eq!(parsed.page_count, 3);
    assert_eq!(parsed.pages.len(), 3);
    assert_eq!(parsed.byte_len, bytes.len());
    for page in 0..3 {
        assert!(
            parsed
                .quality
                .issues
                .contains(&jade_pdf2docx_parser::QualityIssue::OcrRequired { page }),
            "opening encryption must not hide unverified image text on page {page}"
        );
    }
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

/// 独立generatorの暗号方式・認証・コピー許可を検証し、bytes APIの境界も維持する。
#[test]
#[ignore = "requires tools/generate_encrypted_parser_controls.py output and native Pdfium"]
fn owned_encryption_controls_preserve_authentication_and_permissions() {
    let directory = std::env::var_os("JADE_ENCRYPTED_PARSER_CONTROLS")
        .expect("JADE_ENCRYPTED_PARSER_CONTROLS required");
    let directory = Path::new(&directory);
    for name in [
        "rc4-40",
        "aes-128",
        "rc4-128",
        "aes-256-r5",
        "aes-256",
        "aes-256-pdf20",
        "aes-256-catalog20",
    ] {
        let path = directory.join(format!("{name}.pdf"));
        let digest = fs::read_to_string(directory.join(format!("{name}.pdf.sha256"))).unwrap();
        let bytes = verified(&path, &digest);
        assert!(matches!(
            parse_pdf_bytes("encrypted-bytes.pdf", &bytes),
            Err(ParseError::UnsupportedEncryption { .. })
        ));
        let parsed = parse_pdf(&path).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(parsed.page_count, 1);
        assert_eq!(parsed.byte_len, bytes.len());
        assert_eq!(
            parsed.header,
            if name.ends_with("pdf20") {
                "%PDF-2.0"
            } else {
                "%PDF-1.7"
            }
        );
        assert_eq!(
            parsed.version.major,
            if name.ends_with("20") { 2 } else { 1 }
        );
        assert_eq!(
            parsed.version.minor,
            if name.ends_with("20") { 0 } else { 7 }
        );
        let compact: String = parsed
            .text_preview
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert!(
            compact.contains("Ownedencryptioncontrol"),
            "{name}: {}",
            parsed.text_preview
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    for name in ["user-password", "copy-denied"] {
        let path = directory.join(format!("{name}.pdf"));
        let bytes = fs::read(&path).unwrap();
        assert!(
            matches!(
                parse_pdf(&path),
                Err(ParseError::UnsupportedEncryption { .. })
            ),
            "{name}"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
