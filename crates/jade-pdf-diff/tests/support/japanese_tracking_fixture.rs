//! 外部字体を使わず、日本語の字間・実空白・Unicode編集を分離する資料。
use super::{document, stream};

/// 自作長方形と空白のみ。生成元はtools/generate_japanese_tracking_font.py。
/// SHA-256: 08a2ab26a6f650b8b996b135cc2742c8ad0b3a55af9eb2f0b25a4fc33e2cf63e.
fn tracking_font() -> &'static str {
    concat!(
        "00010000000A0080000300204F532F3243D3D82B0000012800000060636D6170A5C0F264000001A000000054",
        "676C796655AB1EB80000020400000078686561645A1DA49C000000AC00000036686865610579051B000000E4",
        "00000024686D74781432000000000188000000186C6F63610084006C000001F40000000E6D61787000080006",
        "00000108000000206E616D65F85EB31B0000027C0000012F706F737400030000000003AC0000002000010000",
        "000100004DAC0BF25F0F3CF5000303E8000000007C25B080000000007C25B0800000000002BC02BC00000003",
        "00020000000000000001000002BC0000000003E80000012C02BC000100000000000000000000000000000006",
        "00010000000600040001000000000002000000000000000000000000000000000003035E0190000500040000",
        "0000000000000000000000000000000000000000000000000000000000000000000108020000000000000000",
        "00003F3F3F3F000000208ACB02BC0000000002BC000000000000000000000000000000000020000003E80000",
        "03E8000003E8000003E8000003E8000000AA0000000000020000000300000014000300010000001400040040",
        "0000000C0008000200040020306E66F875338ACBFFFF00000020306E66F875338ACBFFFFFFE5CF96990B8ACE",
        "753700010000000000000000000000000000000C001800240030003C003C000000010000000002BC02BC0003",
        "00003121112102BCFD4402BC00010000000002BC02BC000300003121112102BCFD4402BC00010000000002BC",
        "02BC000300003121112102BCFD4402BC00010000000002BC02BC000300003121112102BCFD4402BC00010000",
        "000002BC02BC000300003121112102BCFD4402BC0000000A007E0001000000000001000C0000000100000000",
        "00020007000C0001000000000003002800130001000000000004000C00000001000000000006000C00000003",
        "0001040900010018003B0003000104090002000E005300030001040900030050006100030001040900040018",
        "003B00030001040900060018003B4A616465547261636B696E67526567756C61724A61646520696E64657065",
        "6E64656E742072656374616E676C6520747261636B696E672074657374004A00610064006500540072006100",
        "63006B0069006E00670052006500670075006C00610072004A00610064006500200069006E00640065007000",
        "65006E00640065006E0074002000720065006300740061006E0067006C006500200074007200610063006B00",
        "69006E0067002000740065007300740000030000000000000000000000000000000000000000000000000000",
        "00000000>",
    )
}

/// 全角文字の四分アキ以内の字間と、原文に書いた空白を独立に変更する。
pub fn japanese_tracking(
    mode: &str,
    unit: f64,
    rotation: u16,
    reverse: bool,
    gap: f64,
    revised: bool,
) -> Vec<u8> {
    let width = if mode == "wide" { 2000. } else { 1000. };
    let step = 12. * (width / 1000. + gap);
    let mut parts = (0..4)
        .map(|i| {
            let suffix = if mode == "coded" && i < 3 { "0005" } else { "" };
            format!(
                "BT /F1 12 Tf {} 100 Td <{:04X}{suffix}> Tj ET",
                20. + f64::from(i) * step,
                i + 1
            )
        })
        .collect::<Vec<_>>();
    if reverse {
        parts.reverse();
    }
    let mut body = if mode == "single" {
        format!(
            "BT /F1 12 Tf {} Tc 20 100 Td <0001000200030004> Tj ET",
            12. * gap
        )
    } else {
        parts.join("\n")
    };
    if mode == "actual" {
        let text = if revised {
            "753300208ACB0020985E0020306E"
        } else {
            "753300208ACB002066F80020306E"
        };
        body = format!("/Span << /ActualText <FEFF{text}> >> BDC {body} EMC");
    }
    let content = format!("q {s:.17} 0 0 {s:.17} 0 0 cm {body} Q", s = 1. / unit);
    let values = if mode == "latin" {
        vec!["0041", "0042", "0043", "0044"]
    } else if mode == "hangul" {
        vec!["D55C", "AE00", "C2E0", "CCAD"]
    } else if mode == "supplementary" {
        vec!["D840DC00", "D840DC01", "D88DDC79", "306E"]
    } else if mode == "nfd" {
        vec!["304B3099", "8ACB", "66F8", "306E"]
    } else if mode == "nfc" {
        vec!["304C", "8ACB", "66F8", "306E"]
    } else {
        vec![
            "7533",
            "8ACB",
            if revised { "985E" } else { "66F8" },
            "306E",
        ]
    };
    let mappings = values
        .iter()
        .enumerate()
        .map(|(i, v)| format!("<{:04X}> <{v}>", i + 1))
        .collect::<Vec<_>>()
        .join(" ");
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",200./unit,200./unit),
        "<< /Type /Font /Subtype /Type0 /BaseFont /JadeTracking /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 9 0 R >>".into(),
        stream(&content,""),
        format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /JadeTracking /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 7 0 R /CIDToGIDMap /Identity /DW {width} /W [5 [170]] >>"),
        "<< /Type /FontDescriptor /FontName /JadeTracking /Flags 32 /FontBBox [0 0 700 700] /ItalicAngle 0 /Ascent 700 /Descent 0 /CapHeight 700 /StemV 80 /FontFile2 8 0 R >>".into(),
        stream(tracking_font(),"/Filter /ASCIIHexDecode"),
        stream(&format!("/CIDInit /ProcSet findresource begin 12 dict begin begincmap 1 begincodespacerange <0000> <FFFF> endcodespacerange 5 beginbfchar {mappings} <0005> <0020> endbfchar endcmap end end"),""),
    ])
}
