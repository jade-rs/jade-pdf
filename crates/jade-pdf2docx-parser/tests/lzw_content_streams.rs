//! 有効なLZW本文を読み飛ばさず、非圧縮本文と同じIRと資源制限を保つ回帰案。
use jade_pdf2docx_parser::{ParseError, ParseLimits, ParseOptions, parse_pdf_bytes_with_options};
use proptest::prelude::*;

/// dictionaryを毎byte clearする正規の9bit LZWを独立に作り、幅変更と分離する。
fn literal_lzw(data: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut pending = 0u32;
    let mut bits = 0u8;
    let codes = data
        .iter()
        .flat_map(|byte| [256u16, u16::from(*byte)])
        .chain([257]);
    for code in codes {
        pending = (pending << 9) | u32::from(code);
        bits += 9;
        while bits >= 8 {
            bits -= 8;
            output.push((pending >> bits) as u8);
        }
        pending &= (1u32 << bits) - 1;
    }
    if bits > 0 {
        output.push((pending << (8 - bits)) as u8);
    }
    output
}

/// フィルター以外を同一にした、正式xrefとCatalogを持つ一ページの所有PDF。
fn pdf(content: &[u8], lzw: bool, extra: &str) -> Vec<u8> {
    let data = if lzw {
        literal_lzw(content)
    } else {
        content.to_vec()
    };
    let filter = if lzw { "/Filter /LZWDecode" } else { "" };
    let mut stream =
        format!("<< /Length {} {filter} {extra} >>\nstream\n", data.len()).into_bytes();
    stream.extend_from_slice(&data);
    stream.extend_from_slice(b"\nendstream");
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
    ];
    let mut bytes = b"%PDF-1.2\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(object);
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

/// OCRを使わないnative文字でも、LZW本文の欠落を検出する。
#[test]
fn native_lzw_text_is_not_silently_dropped() {
    let content = b"BT /F1 12 Tf 72 700 Td (LZW CONTENT SENTINEL) Tj ET\n";
    let parsed = parse_pdf_bytes_with_options(
        "owned-LZW.pdf",
        &pdf(content, true, ""),
        &ParseOptions::default(),
    )
    .unwrap();
    assert_eq!(parsed.page_count, 1);
    assert!(
        parsed
            .text_elements
            .iter()
            .any(|text| text.text == "LZW CONTENT SENTINEL")
    );
    assert!(parsed.quality.issues.is_empty());
}

/// 別の文字列内のFlateDecodeを、実際のLZW Filterより優先しない。
#[test]
fn filter_selection_ignores_names_inside_unrelated_strings() {
    let content = b"BT /F1 12 Tf 72 700 Td (FILTER SENTINEL) Tj ET\n";
    let parsed = parse_pdf_bytes_with_options(
        "filter-LZW.pdf",
        &pdf(content, true, "/Note (FlateDecode)"),
        &ParseOptions::default(),
    )
    .unwrap();
    assert!(
        parsed
            .text_elements
            .iter()
            .any(|text| text.text == "FILTER SENTINEL")
    );
}

/// 展開後本文の一件上限はinclusiveで、圧縮形式により検査を回避しない。
#[test]
fn decoded_lzw_stream_limit_is_inclusive() {
    let content = b"BT /F1 12 Tf 72 700 Td (BOUNDED LZW) Tj ET\n";
    let source = pdf(content, true, "");
    let exact = ParseOptions {
        limits: ParseLimits {
            max_decoded_stream_bytes: content.len() as u64,
            ..Default::default()
        },
        ..Default::default()
    };
    let parsed = parse_pdf_bytes_with_options("exact-LZW.pdf", &source, &exact).unwrap();
    assert!(
        parsed
            .text_elements
            .iter()
            .any(|text| text.text == "BOUNDED LZW")
    );
    let short = ParseOptions {
        limits: ParseLimits {
            max_decoded_stream_bytes: content.len() as u64 - 1,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(matches!(
        parse_pdf_bytes_with_options("short-LZW.pdf", &source, &short),
        Err(ParseError::ResourceLimit {
            resource: "decoded stream bytes",
            ..
        })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 非圧縮とLZW本文は位置・font size・文字列・品質を変えない。
    #[test]
    fn literal_lzw_preserves_native_ir(
        text in "[A-Za-z0-9_-]{1,64}",
        x in 12u16..100, y in 100u16..700, size in 6u8..16,
    ) {
        let content = format!("BT /F1 {size} Tf {x} {y} Td ({text}) Tj ET\n");
        let plain = parse_pdf_bytes_with_options("plain.pdf", &pdf(content.as_bytes(), false, ""), &ParseOptions::default()).unwrap();
        let compressed = parse_pdf_bytes_with_options("lzw.pdf", &pdf(content.as_bytes(), true, ""), &ParseOptions::default()).unwrap();
        prop_assert!(!plain.text_elements.is_empty());
        prop_assert_eq!(compressed.text_elements, plain.text_elements);
        prop_assert_eq!(compressed.pages, plain.pages);
        prop_assert_eq!(compressed.graphics, plain.graphics);
        prop_assert_eq!(compressed.quality, plain.quality);
    }
}
