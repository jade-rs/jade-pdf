//! 未確定のSymbol部品を含む、自作矩形字体と独立したPDF表示命令。
use super::{document, stream};

/// 外部字体を使わず、20個の中立部品と既知・未確定の符号を別の矩形字形で作る。
/// SHA-256: 9a93d038b6475b6d71820d2a6a07404dca3d8d383afdae3c0c5a70918d9a6206. tools/generate_symbol_glyph_font.py --profile components で再生成する。
fn symbol_component_font() -> &'static str {
    concat!(
        "00010000000A0080000300204F532F3233E044F20000012800000060636D6170D21EC2FB000001BC0000005C",
        "676C7966BFDF72880000024C00000258686561645955A49C000000AC000000366868656104B101F6000000E4",
        "00000024686D747801F4000000000188000000346C6F6361075007EC00000218000000346D617870001B0006",
        "00000108000000206E616D6540146A45000004A40000011A706F737400030000000005C00000002000010000",
        "00010000CAE16BB35F0F3CF5000303E8000000007C25B080000000007C25B0800000000001F402BC00000003",
        "00020000000000000001000002BC0000000001F40000000001F4000100000000000000000000000000000001",
        "0001000000190004000100000000000200000000000000000000000000000000000301F40190000500040000",
        "0000000000000000000000000000000000000000000000000000000000000000000010000000000000000000",
        "00003F3F3F3F0000F02DF0FE02BC0000000002BC000000000000000000000000000000000020000001F40000",
        "0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
        "0000000000000001000300000000000C0004005000000010001000030000F02DF03CF060F0A3F0EFF0F4F0FE",
        "FFFF0000F02DF03CF060F0A3F0E6F0F4F0F6FFFF0FD60FC60FB80F5E0F1E0F1A0F1900010000000000000000",
        "00000000000000000000000C001800240030003C004800540060006C007800840090009C00A800B400C000CC",
        "00D800E400F000FC010801140120012C00010000000001F402BC000300003121112101F4FE0C02BC00010000",
        "000001F402BC000300003121112101F4FE0C02BC00010000000001F402BC000300003121112101F4FE0C02BC",
        "00010000000001F402BC000300003121112101F4FE0C02BC00010000000001F402BC000300003121112101F4",
        "FE0C02BC00010000000001F402BC000300003121112101F4FE0C02BC00010000000001F402BC000300003121",
        "112101F4FE0C02BC00010000000001F402BC000300003121112101F4FE0C02BC00010000000001F402BC0003",
        "00003121112101F4FE0C02BC00010000000001F402BC000300003121112101F4FE0C02BC00010000000001F4",
        "02BC000300003121112101F4FE0C02BC00010000000001F402BC000300003121112101F4FE0C02BC00010000",
        "000001F402BC000300003121112101F4FE0C02BC00010000000001F402BC000300003121112101F4FE0C02BC",
        "00010000000001F402BC000300003121112101F4FE0C02BC00010000000001F402BC000300003121112101F4",
        "FE0C02BC00010000000001F402BC000300003121112101F4FE0C02BC00010000000001F402BC000300003121",
        "112101F4FE0C02BC00010000000001F402BC000300003121112101F4FE0C02BC00010000000001F402BC0003",
        "00003121112101F4FE0C02BC00010000000001F402BC000300003121112101F4FE0C02BC00010000000001F4",
        "02BC000300003121112101F4FE0C02BC00010000000001F402BC000300003121112101F4FE0C02BC00010000",
        "000001F402BC000300003121112101F4FE0C02BC00010000000001F402BC000300003121112101F4FE0C02BC",
        "0000000A007E0001000000000001000600000001000000000002000700060001000000000003001F000D0001",
        "0000000000040006000000010000000000060008002C0003000104090001000C00340003000104090002000E",
        "00400003000104090003003E004E0003000104090004000C003400030001040900060010008C53796D626F6C",
        "526567756C61724A61646520696E646570656E64656E742072656374616E676C65207465737453796D626F6C",
        "4D5400530079006D0062006F006C0052006500670075006C00610072004A00610064006500200069006E0064",
        "006500700065006E00640065006E0074002000720065006300740061006E0067006C00650020007400650073",
        "007400530079006D0062006F006C004D00540000000300000000000000000000000000000000000000000000",
        "0000000000000000>",
    )
}

/// Adobeの通知付き表で形が特定できる20部品だけを列挙する。
pub fn symbol_component_codes() -> &'static [u8] {
    &[
        0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xeb, 0xec, 0xed, 0xee, 0xef, 0xf4, 0xf6, 0xf7, 0xf8, 0xf9,
        0xfa, 0xfb, 0xfc, 0xfd, 0xfe,
    ]
}

/// 未確定部品と既知記号を別の自作字形に割り当て、原文・字体・セルの衝突を生成する。
pub fn symbol_component_cells(mode: &str, unit: f64, rotation: u16, shear: bool) -> Vec<u8> {
    let mut aliases = vec![0xf0a3u32, 0xf03c, 0xf02d];
    aliases.extend(
        symbol_component_codes()
            .iter()
            .map(|code| 0xf000 + u32::from(*code)),
    );
    aliases.push(0xf060);
    if mode == "wrong_cid" {
        aliases[0] = 0xf0e9;
    }
    let cmap=format!("/CIDInit /ProcSet findresource begin 12 dict begin begincmap 1 begincodespacerange <0000> <FFFF> endcodespacerange {} beginbfchar {} endbfchar endcmap end end",aliases.len(),aliases.iter().enumerate().map(|(i,raw)|format!("<{:04X}> <{raw:04X}>",i+1)).collect::<Vec<_>>().join(" "));
    let mut codes = vec![if mode == "edit" { 2u16 } else { 1 }, 3];
    codes.extend(4..=23);
    if mode == "unknown" {
        codes.push(24);
    }
    let text = codes
        .iter()
        .map(|code| format!("{code:04X}"))
        .collect::<String>();
    let extra = match mode {
        "unowned" => "BT /F2 12 Tf 220 150 Td (A) Tj ET",
        "actual_collision" => {
            "/Span << /ActualText <FEFFF0E9> >> BDC BT /F1 12 Tf 220 150 Td <0002> Tj ET EMC"
        }
        "overlap" => "BT /F1 12 Tf 40 150 Td <0001> Tj ET",
        _ => "",
    };
    let body = if mode == "same_cell" {
        "BT /F1 12 Tf 40 150 Td [<0001> 500 <0007> <0003>] TJ ET".to_string()
    } else {
        format!("BT /F1 12 Tf 40 150 Td <{text}> Tj ET {extra}")
    };
    let content = format!(
        "q {s:.17} 0 {h:.17} {s:.17} 0 0 cm {body} Q",
        s = 1. / unit,
        h = if shear { 0.2 / unit } else { 0. }
    );
    let mut program = symbol_component_font().to_owned();
    if mode == "wrong_program" {
        program = program
            .replace("53796D626F6C", "437573746F6D")
            .replace("00530079006D0062006F006C", "0043007500730074006F006D");
    } else if mode == "unicode_cmap" {
        program = program.replace(
            "00000001000300000000000C0004",
            "00000001000300010000000C0004",
        );
    }
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {0} {0}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 4 0 R /F2 10 0 R >> >> /Contents 5 0 R >>",400./unit),
        "<< /Type /Font /Subtype /Type0 /BaseFont /Symbol /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 9 0 R >>".into(),
        stream(&content,""),
        format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Symbol /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 7 0 R /CIDToGIDMap /{} /DW 500 >>",if mode=="wrong_cidmap"{"Other"}else{"Identity"}),
        "<< /Type /FontDescriptor /FontName /Symbol /Flags 32 /FontBBox [0 0 500 700] /ItalicAngle 0 /Ascent 700 /Descent 0 /CapHeight 700 /StemV 80 /FontFile2 8 0 R >>".into(),
        stream(&program,"/Filter /ASCIIHexDecode"),
        stream(&cmap,""),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 11 0 R >>".into(),
        stream("/CIDInit /ProcSet findresource begin 12 dict begin begincmap 1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfchar <41> <F0E9> endbfchar endcmap end end",""),
    ])
}
