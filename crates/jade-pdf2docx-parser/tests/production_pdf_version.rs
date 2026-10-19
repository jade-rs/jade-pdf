//! CatalogのVersionによる更新とheaderとの優先順位を公開入口で固定する。
use jade_pdf2docx_parser::{
    ParseError, ParseLimits, ParseOptions, PdfVersion, parse_pdf_bytes,
    parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// 可視本文と正式なRootを持ち、Catalog属性だけを差し替えられるPDF。
fn objects(attributes: &str) -> Vec<String> {
    let mut objects = pages("/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> >>");
    objects[0] = format!("<< /Type /Catalog /Pages 2 0 R {attributes} >>");
    objects[4] = stream("BT /F 12 Tf 10 50 Td (PUBLIC) Tj ET");
    objects[5] = stream("");
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    objects
}

/// 正式なPDFのheaderを同じ長さのversionへ変更してxref offsetを保つ。
fn bytes(objects: &[String], major: u8, minor: u8) -> Vec<u8> {
    let mut bytes = pdf(objects);
    bytes[5] = b'0' + major;
    bytes[7] = b'0' + minor;
    bytes
}

/// Catalogの上位Versionを反映し、生のheaderと本文を維持する。
#[test]
fn catalog_version_upgrades_effective_version() {
    for attribute in ["/Version /2.0", "/Version /2#2E0", "/Ver#73ion /2.0"] {
        let parsed = parse_pdf_bytes("version.pdf", &bytes(&objects(attribute), 1, 7)).unwrap();
        assert_eq!(parsed.version, PdfVersion { major: 2, minor: 0 });
        assert_eq!(parsed.header, "%PDF-1.7");
        assert!(parsed.text_preview.contains("PUBLIC"));
    }
}

/// absent/null/下位/equal Versionはheaderを下げず、無関係なCatalogも見ない。
#[test]
fn catalog_version_does_not_downgrade_or_leak_from_other_objects() {
    for attribute in [
        "",
        "/Version null",
        "/Version /1.4",
        "/Version /1.7",
        "/Fake << /Version /2.0 >>",
    ] {
        let mut objects = objects(attribute);
        objects.push("<< /Type /Catalog /Version /2.0 >>".into());
        let parsed = parse_pdf_bytes("version.pdf", &bytes(&objects, 1, 7)).unwrap();
        assert_eq!(parsed.version, PdfVersion { major: 1, minor: 7 });
    }
}

/// 間接Versionとaliasを解決し、異なる世代の参照を構造エラーにする。
#[test]
fn indirect_catalog_version_is_resolved() {
    let mut objects = objects("/Version 11 0 R");
    objects.extend(["12 0 R".into(), "/2.0".into()]);
    let parsed = parse_pdf_bytes("version.pdf", &pdf(&objects)).unwrap();
    assert_eq!(parsed.version, PdfVersion { major: 2, minor: 0 });
    objects[0] = "<< /Type /Catalog /Pages 2 0 R /Version 11 1 R >>".into();
    assert!(matches!(
        parse_pdf_bytes("version.pdf", &pdf(&objects)),
        Err(ParseError::InvalidPdfStructure {
            reason: "missing or wrong-generation page attribute",
            ..
        })
    ));
}

/// 不正なVersionを無言でheaderへ戻さず、型と形式を構造エラーにする。
#[test]
fn malformed_catalog_version_is_rejected() {
    for value in [
        "(2.0)",
        "2.0",
        "[/2.0]",
        "<< /Version /2.0 >>",
        "/2",
        "/2.00",
        "/2#GG0",
        "/2.0suffix",
    ] {
        assert!(
            matches!(
                parse_pdf_bytes("version.pdf", &pdf(&objects(&format!("/Version {value}")))),
                Err(ParseError::InvalidPdfStructure {
                    reason: "Catalog Version is not a major.minor name",
                    ..
                })
            ),
            "{value}"
        );
    }
}

/// 循環Versionの参照は有限に拒否し、深度はinclusiveな共有上限に従う。
#[test]
fn catalog_version_reference_depth_is_bounded() {
    let mut objects = objects("/Version 11 0 R");
    objects.extend([
        "12 0 R".into(),
        "13 0 R".into(),
        "14 0 R".into(),
        "15 0 R".into(),
        "/2.0".into(),
    ]);
    for depth in [4, 5] {
        let result = parse_pdf_bytes_with_options(
            "version.pdf",
            &pdf(&objects),
            &ParseOptions {
                limits: ParseLimits {
                    max_container_nesting: depth,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        if depth == 4 {
            assert!(
                matches!(
                    result,
                    Err(ParseError::ResourceLimit {
                        resource: "page attribute depth",
                        limit: 4,
                        actual: 5
                    })
                ),
                "{result:?}"
            );
        } else {
            assert_eq!(result.unwrap().version, PdfVersion { major: 2, minor: 0 });
        }
    }
    objects[14] = "11 0 R".into();
    assert!(matches!(
        parse_pdf_bytes("version.pdf", &pdf(&objects)),
        Err(ParseError::InvalidPdfStructure {
            reason: "cyclic page attribute reference",
            ..
        })
    ));
}

/// 最新xrefのRootと更新済みCatalogだけを読み、古いVersionを混ぜない。
#[test]
fn latest_xref_selects_the_effective_catalog_version() {
    let mut base = bytes(&objects("/Version /2.0"), 1, 3);
    let previous = std::str::from_utf8(&base)
        .unwrap()
        .rsplit("startxref\n")
        .next()
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let catalog = base.len();
    base.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R /Version /1.4 >> endobj\n");
    let xref = base.len();
    base.extend_from_slice(format!("xref\n1 1\n{catalog:010} 00000 n \ntrailer << /Size 11 /Root 1 0 R /Prev {previous} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
    let parsed = parse_pdf_bytes("version.pdf", &base).unwrap();
    assert_eq!(parsed.version, PdfVersion { major: 1, minor: 4 });
    assert!(parsed.text_preview.contains("PUBLIC"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 有効VersionはheaderとCatalogをmajor/minor順で比較した最大値になる。
    #[test]
    fn effective_version_is_the_numeric_maximum(header in (1u8..=2, 0u8..=7), catalog in (1u8..=2, 0u8..=7)) {
        let parsed = parse_pdf_bytes_with_options("version.pdf", &bytes(&objects(&format!("/Version /{}.{}", catalog.0, catalog.1)), header.0, header.1), &ParseOptions { strict: true, ..Default::default() }).unwrap();
        let expected = header.max(catalog);
        prop_assert_eq!(parsed.version, PdfVersion { major: expected.0, minor: expected.1 });
    }
}
