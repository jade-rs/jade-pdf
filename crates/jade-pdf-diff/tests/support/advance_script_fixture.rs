//! 送りセルと字面の幅を分けた、自作長方形の添字読み順資料。
use super::{document, stream, symbol_glyph_font};

/// 別オブジェクトの二桁添字を作り、実際の送り間隔とUnicodeだけの編集を独立に変える。
pub fn advance_script_layout(
    unit: f64,
    rotation: u16,
    reverse: bool,
    gap: f64,
    revised: bool,
) -> Vec<u8> {
    let mut parts = vec![
        "BT /F1 12 Tf 20 100 Td <0001> Tj ET".to_string(),
        "BT /F1 8 Tf 28 98 Td <0002> Tj ET".to_string(),
        format!("BT /F1 8 Tf {} 98 Td <0003> Tj ET", 36. + gap),
        "BT /F1 12 Tf 60 100 Td <0004> Tj ET".to_string(),
    ];
    if gap > 0. {
        parts[1] = "BT /F1 8 Tf 28 98 Td <000200050003> Tj ET".into();
        parts.remove(2);
    }
    let body = if reverse {
        parts.into_iter().rev().collect::<Vec<_>>().join("\n")
    } else {
        parts.join("\n")
    };
    let content = format!("q {s:.17} 0 0 {s:.17} 0 0 cm {body} Q", s = 1. / unit);
    document(vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /UserUnit {unit} /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",200./unit,200./unit),
        "<< /Type /Font /Subtype /Type0 /BaseFont /OwnedAdvance /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 9 0 R >>".into(),
        stream(&content,""),
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /OwnedAdvance /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 7 0 R /CIDToGIDMap /Identity /DW 1000 /W [5 [750]] >>".into(),
        "<< /Type /FontDescriptor /FontName /OwnedAdvance /Flags 32 /FontBBox [0 0 500 700] /ItalicAngle 0 /Ascent 700 /Descent -200 /CapHeight 700 /StemV 80 /FontFile2 8 0 R >>".into(),
        stream(symbol_glyph_font(),"/Filter /ASCIIHexDecode"),
        stream(&format!("/CIDInit /ProcSet findresource begin 12 dict begin begincmap 1 begincodespacerange <0000> <FFFF> endcodespacerange 5 beginbfchar <0001> <0062> <0002> <0031> <0003> <{}> <0004> <003D> <0005> <0020> endbfchar endcmap end end",if revised {"0032"} else {"0030"}),""),
    ])
}
