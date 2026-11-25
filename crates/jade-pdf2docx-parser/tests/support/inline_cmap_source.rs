//! 外部fontを複製せず、descriptorの重さだけを指定する所有PDFを作る。
/// simple/CIDの同じ本文へ、直接値または間接数値のFontWeightを与える。
pub fn source(entries: &str, name: &str, scalar: Option<&str>, cid: bool) -> Vec<u8> {
    make(entries, name, scalar, cid, None, None)
}
/// 同じ所有PDFのCMapと文字codeだけを差し替え、xrefとLengthを再構成する。
pub fn mapped(cmap: &str, encoded: &str) -> Vec<u8> {
    make(
        "/FontWeight 700",
        "Vendor-Regular",
        None,
        true,
        Some(cmap),
        Some(encoded),
    )
}
/// 固定した所有FontWeightテストとbyte単位で同じ元CMap。
pub const CMAP: &str = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> def /CMapName /OwnedWeight def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange 6 beginbfchar <0057> <0057> <0065> <0065> <0069> <0069> <0067> <0067> <0068> <0068> <0074> <0074> endbfchar endcmap CMapName currentdict /CMap defineresource pop end end";
/// 任意の所有CMapから同じ構造の一ページを作る。
fn make(
    entries: &str,
    name: &str,
    scalar: Option<&str>,
    cid: bool,
    cmap_override: Option<&str>,
    show_override: Option<&str>,
) -> Vec<u8> {
    let show = if cid {
        "<005700650069006700680074>"
    } else {
        "(Weight)"
    };
    let show = show_override.unwrap_or(show);
    let content = format!("BT /F1 12 Tf 72 700 Td {show} Tj ET");
    let cmap = cmap_override.unwrap_or(CMAP);
    let font = if cid {
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H /DescendantFonts [7 0 R] /ToUnicode 8 0 R >>"
        )
    } else {
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /{name} /Encoding /StandardEncoding /FontDescriptor 6 0 R >>"
        )
    };
    let objects=vec!["<< /Type /Catalog /Pages 2 0 R >>".to_owned(),"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),format!("<< /Length {} >>\nstream\n{content}\nendstream",content.len()),font,format!("<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [-100 -200 1000 800] /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 {entries} >>"),if cid {format!("<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor 6 0 R /DW 600 >>")}else{"null".into()},if cid{format!("<< /Length {} >>\nstream\n{cmap}\nendstream",cmap.len())}else{"null".into()},scalar.unwrap_or("null").to_owned()];
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
