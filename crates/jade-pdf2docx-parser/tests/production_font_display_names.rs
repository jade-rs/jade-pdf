use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;

/// 名前テーブルの対応だけを検査する、描画用ではない所有sfnt metadataを作る。
fn named_sfnt(mac: &str, windows: &str) -> Vec<u8> {
    let windows: Vec<u8> = windows.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut name = Vec::new();
    for value in [
        0u16,
        2,
        30,
        1,
        0,
        0,
        6,
        mac.len() as u16,
        0,
        3,
        1,
        0x409,
        6,
        windows.len() as u16,
        mac.len() as u16,
    ] {
        name.extend_from_slice(&value.to_be_bytes());
    }
    name.extend_from_slice(mac.as_bytes());
    name.extend_from_slice(&windows);
    let offset = 44u32;
    let mut font = vec![0; offset as usize + 10];
    font[..4].copy_from_slice(&[0, 1, 0, 0]);
    font[4..6].copy_from_slice(&2u16.to_be_bytes());
    font[12..16].copy_from_slice(b"OS/2");
    font[20..24].copy_from_slice(&offset.to_be_bytes());
    font[24..28].copy_from_slice(&10u32.to_be_bytes());
    font[28..32].copy_from_slice(b"name");
    font[36..40].copy_from_slice(&(offset + 10).to_be_bytes());
    font[40..44].copy_from_slice(&(name.len() as u32).to_be_bytes());
    font[offset as usize + 8..offset as usize + 10].copy_from_slice(&8u16.to_be_bytes());
    font.extend_from_slice(&name);
    font
}

/// 原本のbyte名と埋め込みsfntを別々に宣言した正式xref付きPDFを作る。
fn pdf(
    raw_name: &str,
    descriptor_name: &str,
    mac: &str,
    windows: &str,
    cid: bool,
    file_generation: u16,
) -> Vec<u8> {
    let font = named_sfnt(mac, windows);
    let content = if cid {
        "BT /F1 12 Tf 72 700 Td <0041> Tj ET"
    } else {
        "BT /F1 12 Tf 72 700 Td (A) Tj ET"
    };
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()).into_bytes(),
        if cid {
            format!("<< /Type /Font /Subtype /Type0 /BaseFont /{raw_name} /Encoding /Identity-H /DescendantFonts [8 0 R] /ToUnicode 9 0 R >>").into_bytes()
        } else {
            format!("<< /Type /Font /Subtype /TrueType /BaseFont /{raw_name} /Encoding /WinAnsiEncoding /FontDescriptor 6 0 R /FirstChar 65 /LastChar 65 /Widths [600] >>").into_bytes()
        },
        format!("<< /Type /FontDescriptor /FontName /{descriptor_name} /FontFile2 7 {file_generation} R >>").into_bytes(),
        [format!("<< /Length {} >>\nstream\n", font.len()).into_bytes(), font, b"\nendstream".to_vec()].concat(),
    ];
    if cid {
        objects.push(format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{raw_name} /FontDescriptor 6 0 R /DW 600 >>").into_bytes());
        let cmap = "1 beginbfchar\n<0041> <0041>\nendbfchar";
        objects
            .push(format!("<< /Length {} >>\nstream\n{cmap}\nendstream", cmap.len()).into_bytes());
    }
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(object);
        bytes.extend_from_slice(b"\nendobj\n");
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

/// 公開byte APIで文字とその表示Font名を読む。
fn family(bytes: &[u8]) -> Option<String> {
    let parsed = parse_pdf_bytes("owned-font-metadata.pdf", bytes).unwrap();
    let text = parsed.text_elements.iter().find(|t| t.text == "A").unwrap();
    text.font_name.clone()
}

/// 不明な文字コードを推測せず、同じdescriptorが持つ名前テーブルから表示名を得る。
#[test]
fn non_utf8_pdf_name_uses_verified_program_name_for_simple_and_cid_fonts() {
    let raw = "ABCDEF+#82#6c#82#72#96#be#92#a9";
    for cid in [false, true] {
        assert_eq!(
            family(&pdf(raw, raw, "OwnedCJK", "OwnedCJK", cid, 0)).as_deref(),
            Some("ABCDEF+OwnedCJK")
        );
    }
}

/// 有効なUTF-8の原本名は、内部の名前で書き換えない。
#[test]
fn valid_declared_name_retains_precedence() {
    assert_eq!(
        family(&pdf(
            "Declared",
            "Declared",
            "ProgramName",
            "ProgramName",
            false,
            0
        ))
        .as_deref(),
        Some("Declared")
    );
}

/// 不一致の名前・古い世代・不正なPostScript名から対応を発明しない。
#[test]
fn conflicting_or_unbound_font_metadata_retains_unresolved_resource_name() {
    let raw = "ABCDEF+#82#6c#82#72#96#be#92#a9";
    for bytes in [
        pdf(raw, raw, "FirstName", "OtherName", false, 0),
        pdf(raw, "UVWXYZ+#ff", "OwnedCJK", "OwnedCJK", false, 0),
        pdf(raw, raw, "OwnedCJK", "OwnedCJK", false, 1),
        pdf(raw, raw, "Bad/Name", "Bad/Name", false, 0),
        pdf(raw, raw, &"X".repeat(64), &"X".repeat(64), false, 0),
    ] {
        assert_eq!(family(&bytes).as_deref(), Some("F1"));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    /// 任意のsubset prefixと検証済みASCII名でも、resource名へ欠落させない。
    #[test]
    fn verified_program_names_preserve_subset_identity(prefix in "[A-Z]{6}", name in "[A-Za-z]{1,24}", cid in any::<bool>()) {
        let raw = format!("{prefix}+#82#6c#82#72#96#be#92#a9");
        let expected = format!("{prefix}+{name}");
        prop_assert_eq!(family(&pdf(&raw, &raw, &name, &name, cid, 0)), Some(expected));
    }
}
