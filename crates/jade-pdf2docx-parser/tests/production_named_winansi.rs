//! 所有Standard14 PDFで、明示WinAnsiのglyph名をUTF-8やShift_JISと混同しない。
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;

/// Font programなしの標準Helveticaと任意の文字描画operandだけを持つ所有PDFを作る。
fn pdf(show: &str, encoding: bool, unicode: bool) -> Vec<u8> {
    let content = format!("BT /F1 12 Tf 72 700 Td 14 TL {show} ET");
    let cmap = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /OwnedOverride def /CMapType 2 def 1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<97> <2603>\nendbfchar endcmap CMapName currentdict /CMap defineresource pop end end";
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica {} {} >>", if encoding { "/Encoding /WinAnsiEncoding" } else { "" }, if unicode { "/ToUnicode 6 0 R" } else { "" }),
    ];
    if unicode {
        objects.push(format!(
            "<< /Length {} >>\nstream\n{cmap}\nendstream",
            cmap.len()
        ));
    }
    let mut data = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
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

/// 全byteをoctalまたはhexへ表し、文字描画命令の種類を変える。
fn show(bytes: &[u8], route: usize) -> String {
    let literal = format!(
        "({})",
        bytes
            .iter()
            .map(|b| format!("\\{b:03o}"))
            .collect::<String>()
    );
    let hex = format!(
        "<{}>",
        bytes.iter().map(|b| format!("{b:02X}")).collect::<String>()
    );
    match route {
        0 => format!("{literal} Tj"),
        1 => format!("{hex} Tj"),
        2 => format!("[{literal}] TJ"),
        3 => format!("[{hex}] TJ"),
        4 => format!("{literal} '"),
        5 => format!("0 0 {literal} \""),
        _ => unreachable!(),
    }
}

/// 公開byte parserの配置文字を順に読み、preview用の正規化を混ぜない。
fn extracted(show: &str, encoding: bool, unicode: bool) -> String {
    let parsed = parse_pdf_bytes(
        std::path::Path::new("owned-winansi.pdf"),
        &pdf(show, encoding, unicode),
    )
    .unwrap();
    parsed
        .text_elements
        .iter()
        .map(|t| t.text.as_str())
        .collect()
}

/// AnnexDの同じcodeはliteral・hex・Tj・TJ・quoteでも同じ文字になる。
#[test]
fn named_winansi_recovers_punctuation_currency_and_latin_glyphs() {
    for route in 0..6 {
        assert_eq!(
            extracted(&show(&[0x97, 0x80, 0xe9, 0xd7, 0xf7], route), true, false),
            "—€é×÷",
            "route={route}"
        );
    }
}

/// 原本ToUnicodeは明示WinAnsiより優先し、encodingがないUTF-8の従来fallbackは変えない。
#[test]
fn authored_unicode_and_unspecified_encoding_are_not_overridden() {
    for route in 0..6 {
        assert_eq!(extracted(&show(&[0x97], route), true, true), "☃");
    }
    assert_eq!(extracted(&show("é".as_bytes(), 0), false, false), "é");
    assert_eq!(extracted(&show("é".as_bytes(), 0), true, false), "Ã©");
}

proptest! {
    #![proptest_config(ProptestConfig {cases:24,..ProptestConfig::default()})]
    /// 独立した既知値を連結し、raw byteの並びやoperandの表現でUnicodeが変わらない。
    #[test]
    fn named_winansi_sequences_keep_their_literal_unicode(indices in prop::collection::vec(0usize..12,0..24),route in 0usize..6) {
        let pairs=[(0x41,"A"),(0x97,"—"),(0x80,"€"),(0xe9,"é"),(0xd7,"×"),(0xf7,"÷"),(0x91,"‘"),(0x92,"’"),(0x93,"“"),(0x94,"”"),(0xa0," "),(0xad,"-")];
        let mut bytes=vec![b'['];let mut expected=String::from("[");
        for index in indices { bytes.push(pairs[index].0);expected.push_str(pairs[index].1); }
        bytes.push(b']');expected.push(']');
        prop_assert_eq!(extracted(&show(&bytes,route),true,false),expected);
    }
}
