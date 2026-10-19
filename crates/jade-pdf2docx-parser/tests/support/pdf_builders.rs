#![allow(dead_code)]

/// 正確な stream Length と xref offset を持つ text fixture を作る。
fn build_minimal_pdf(stream_contents: &str) -> String {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{stream_contents}\nendstream", stream_contents.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut source = "%PDF-1.4\n".to_owned();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = source.len();
    source.push_str("xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        source.push_str(&format!("{offset:010} 00000 n \n"));
    }
    source.push_str(&format!(
        "trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    source
}

pub fn minimal_pdf_with_text(text: &str) -> String {
    build_minimal_pdf(&format!("BT /F1 12 Tf 10 100 Td ({text}) Tj ET"))
}

/// 空text objectで設定したfont stateを次のtext objectが使う最小PDFを作る。
pub fn minimal_pdf_with_inherited_text_font_state(text: &str) -> String {
    build_minimal_pdf(&format!("BT /F1 24 Tf ET BT 10 100 Td ({text}) Tj ET"))
}

pub fn minimal_pdf_with_two_text_ops(first: &str, second: &str) -> String {
    build_minimal_pdf(&format!(
        "BT /F1 12 Tf 10 120 Td ({first}) Tj 0 -20 Td ({second}) Tj ET"
    ))
}

pub fn minimal_pdf_with_hex_text(hex: &str) -> String {
    build_minimal_pdf(&format!("BT /F1 12 Tf 10 100 Td <{hex}> Tj ET"))
}

/// font `/Encoding` に external CMap 名を持つ最小 PDF を作る。
pub fn minimal_pdf_with_external_cmap_hex_text(encoding: &str, hex: &str) -> String {
    let stream_contents = format!("BT /F1 12 Tf 10 100 Td <{hex}> Tj ET");
    let stream_length = stream_contents.len();
    format!(
        "%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>
endobj
4 0 obj
<< /Length {stream_length} >>
stream
{stream_contents}
endstream
endobj
5 0 obj
<< /Type /Font /Subtype /Type0 /BaseFont /DummyCID /Encoding /{encoding} >>
endobj
%%EOF
"
    )
}

pub fn minimal_pdf_with_tounicode_bfchar_text(hex: &str) -> String {
    let stream_contents = format!("BT /F1 12 Tf 10 100 Td <{hex}> Tj ET");
    let stream_length = stream_contents.len();
    let cmap = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfchar
<0001> <65E5>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
    let cmap_length = cmap.len();
    format!(
        "%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>
endobj
4 0 obj
<< /Length {stream_length} >>
stream
{stream_contents}
endstream
endobj
5 0 obj
<< /Type /Font /Subtype /Type0 /BaseFont /DummyCID /ToUnicode 6 0 R >>
endobj
6 0 obj
<< /Length {cmap_length} >>
stream
{cmap}
endstream
endobj
%%EOF
"
    )
}

/// 2つの親Pages nodeで同名resourceが異なるFont objectとToUnicodeを指す最小PDFを作る。
pub fn minimal_two_page_pdf_with_colliding_font_resource_names() -> String {
    let first_content = "BT /F1 12 Tf 10 100 Td <01> Tj ET";
    let second_content = "BT /F1 12 Tf 10 100 Td <01> Tj ET";
    let first_cmap = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfchar
<01> <0041>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
    let second_cmap = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfchar
<01> <0042>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
    format!(
        "%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [11 0 R 12 0 R] /Count 2 >> endobj
3 0 obj << /Type /Page /Parent 11 0 R /MediaBox [0 0 200 200] /Contents 5 0 R >> endobj
4 0 obj << /Type /Page /Parent 12 0 R /MediaBox [0 0 200 200] /Contents 6 0 R >> endobj
5 0 obj << /Length {} >> stream
{first_content}
endstream endobj
6 0 obj << /Length {} >> stream
{second_content}
endstream endobj
7 0 obj << /Type /Font /Subtype /Type0 /BaseFont /PageOneFont /Encoding /Identity-H /ToUnicode 9 0 R >> endobj
8 0 obj << /Type /Font /Subtype /Type0 /BaseFont /PageTwoFont /Encoding /Identity-H /ToUnicode 10 0 R >> endobj
9 0 obj << /Length {} >> stream
{first_cmap}
endstream endobj
10 0 obj << /Length {} >> stream
{second_cmap}
endstream endobj
11 0 obj << /Type /Pages /Parent 2 0 R /Kids [3 0 R] /Count 1 /Resources << /Font << /F1 7 0 R >> >> >> endobj
12 0 obj << /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 /Resources << /Font << /F1 << /Type /Font /Subtype /Type0 /BaseFont /PageTwoFont /Encoding /Identity-H /ToUnicode 10 0 R >> >> >> >> endobj
%%EOF
",
        first_content.len(),
        second_content.len(),
        first_cmap.len(),
        second_cmap.len(),
    )
}

pub fn minimal_pdf_with_tounicode_bfrange_text(hex: &str) -> String {
    let stream_contents = format!("BT /F1 12 Tf 10 100 Td <{hex}> Tj ET");
    let stream_length = stream_contents.len();
    let cmap = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfrange
<0001> <0002> <65E5>
endbfrange
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
    let cmap_length = cmap.len();
    format!(
        "%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>
endobj
4 0 obj
<< /Length {stream_length} >>
stream
{stream_contents}
endstream
endobj
5 0 obj
<< /Type /Font /Subtype /Type0 /BaseFont /DummyCID /ToUnicode 6 0 R >>
endobj
6 0 obj
<< /Length {cmap_length} >>
stream
{cmap}
endstream
endobj
%%EOF
"
    )
}

/// 配列形式の ToUnicode bfrange を持つ最小 PDF を作る。
pub fn minimal_pdf_with_tounicode_bfrange_array_text(hex: &str) -> String {
    let stream_contents = format!("BT /F1 12 Tf 10 100 Td <{hex}> Tj ET");
    let stream_length = stream_contents.len();
    let cmap = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 beginbfrange
<0001> <0002> [<65E5> <672C>]
endbfrange
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
    let cmap_length = cmap.len();
    format!(
        "%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>
endobj
4 0 obj
<< /Length {stream_length} >>
stream
{stream_contents}
endstream
endobj
5 0 obj
<< /Type /Font /Subtype /Type0 /BaseFont /DummyCID /ToUnicode 6 0 R >>
endobj
6 0 obj
<< /Length {cmap_length} >>
stream
{cmap}
endstream
endobj
%%EOF
"
    )
}

/// 1 byte CID の ToUnicode bfchar を持つ最小 PDF を作る。
pub fn minimal_pdf_with_tounicode_one_byte_bfchar_text(hex: &str) -> String {
    let stream_contents = format!("BT /F1 12 Tf 10 100 Td <{hex}> Tj ET");
    let stream_length = stream_contents.len();
    let cmap = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 begincodespacerange
<00> <FF>
endcodespacerange
2 beginbfchar
<01> <0041>
<02> <0042>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
    let cmap_length = cmap.len();
    format!(
        "%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>
endobj
4 0 obj
<< /Length {stream_length} >>
stream
{stream_contents}
endstream
endobj
5 0 obj
<< /Type /Font /Subtype /Type0 /BaseFont /DummyCID /ToUnicode 6 0 R >>
endobj
6 0 obj
<< /Length {cmap_length} >>
stream
{cmap}
endstream
endobj
%%EOF
"
    )
}

/// 1 byte / 2 byte CID が混在する ToUnicode bfchar を持つ最小 PDF を作る。
pub fn minimal_pdf_with_tounicode_mixed_width_bfchar_text(hex: &str) -> String {
    let stream_contents = format!("BT /F1 12 Tf 10 100 Td <{hex}> Tj ET");
    let stream_length = stream_contents.len();
    let cmap = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
2 begincodespacerange
<00> <7F>
<8000> <FFFF>
endcodespacerange
3 beginbfchar
<01> <0041>
<02> <0042>
<8001> <65E5>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
    let cmap_length = cmap.len();
    format!(
        "%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>
endobj
4 0 obj
<< /Length {stream_length} >>
stream
{stream_contents}
endstream
endobj
5 0 obj
<< /Type /Font /Subtype /Type0 /BaseFont /DummyCID /ToUnicode 6 0 R >>
endobj
6 0 obj
<< /Length {cmap_length} >>
stream
{cmap}
endstream
endobj
%%EOF
"
    )
}

/// 2 byte mapping はあるが codespacerange では 1 byte だけを許可する最小 PDF を作る。
pub fn minimal_pdf_with_tounicode_disallowed_width_bfchar_text(hex: &str) -> String {
    let stream_contents = format!("BT /F1 12 Tf 10 100 Td <{hex}> Tj ET");
    let stream_length = stream_contents.len();
    let cmap = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 begincodespacerange
<00> <7F>
endcodespacerange
2 beginbfchar
<01> <0041>
<8001> <65E5>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
    let cmap_length = cmap.len();
    format!(
        "%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>
endobj
4 0 obj
<< /Length {stream_length} >>
stream
{stream_contents}
endstream
endobj
5 0 obj
<< /Type /Font /Subtype /Type0 /BaseFont /DummyCID /ToUnicode 6 0 R >>
endobj
6 0 obj
<< /Length {cmap_length} >>
stream
{cmap}
endstream
endobj
%%EOF
"
    )
}

/// mapping はあるが codespacerange の値域外 CID を持つ最小 PDF を作る。
pub fn minimal_pdf_with_tounicode_disallowed_range_bfchar_text(hex: &str) -> String {
    let stream_contents = format!("BT /F1 12 Tf 10 100 Td <{hex}> Tj ET");
    let stream_length = stream_contents.len();
    let cmap = "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
1 begincodespacerange
<00> <7F>
endcodespacerange
2 beginbfchar
<01> <0041>
<80> <65E5>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";
    let cmap_length = cmap.len();
    format!(
        "%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>
endobj
4 0 obj
<< /Length {stream_length} >>
stream
{stream_contents}
endstream
endobj
5 0 obj
<< /Type /Font /Subtype /Type0 /BaseFont /DummyCID /ToUnicode 6 0 R >>
endobj
6 0 obj
<< /Length {cmap_length} >>
stream
{cmap}
endstream
endobj
%%EOF
"
    )
}

pub fn minimal_pdf_with_literal_text(literal: &str) -> String {
    build_minimal_pdf(&format!("BT /F1 12 Tf 10 100 Td ({literal}) Tj ET"))
}

pub fn minimal_pdf_with_ordered_hex_and_literal(hex: &str, literal: &str) -> String {
    build_minimal_pdf(&format!(
        "BT /F1 12 Tf 10 100 Td <{hex}> Tj ({literal}) Tj ET"
    ))
}

pub fn minimal_pdf_with_tj_array(hex1: &str, literal: &str, hex2: &str) -> String {
    build_minimal_pdf(&format!(
        "BT /F1 12 Tf 10 100 Td [<{hex1}> ({literal}) -120 <{hex2}>] TJ ET"
    ))
}

pub fn minimal_pdf_with_tj_array_numeric_tokens(
    left: &str,
    right: &str,
    token1: &str,
    token2: &str,
) -> String {
    build_minimal_pdf(&format!(
        "BT /F1 12 Tf 10 100 Td [({left}) {token1} ({right}) {token2}] TJ ET"
    ))
}

pub fn minimal_pdf_with_tj_array_utf16_and_numeric_tokens(
    left: &str,
    right: &str,
    token1: &str,
    token2: &str,
) -> String {
    build_minimal_pdf(&format!(
        "BT /F1 12 Tf 10 100 Td [({left}) {token1} <FEFF65E5672C> {token2} ({right})] TJ ET"
    ))
}

pub fn minimal_pdf_with_two_text_objects(first: &str, second: &str) -> String {
    build_minimal_pdf(&format!(
        "BT /F1 12 Tf 10 120 Td ({first}) Tj ET
BT /F1 12 Tf 10 100 Td ({second}) Tj ET"
    ))
}

pub fn minimal_pdf_with_malformed_tj_then_valid(broken: &str, valid: &str) -> String {
    build_minimal_pdf(&format!(
        "% intentionally malformed TJ: no closing bracket
BT /F1 12 Tf 10 130 Td [({broken}) -120 ({broken}) Tj ET
BT /F1 12 Tf 10 100 Td ({valid}) Tj ET"
    ))
}

pub fn minimal_pdf_with_malformed_literal_then_valid(broken: &str, valid: &str) -> String {
    build_minimal_pdf(&format!(
        "BT /F1 12 Tf 10 100 Td ({broken} Tj ({valid}) Tj ET"
    ))
}

pub fn minimal_pdf_with_malformed_hex_then_valid(broken_hex: &str, valid: &str) -> String {
    build_minimal_pdf(&format!(
        "BT /F1 12 Tf 10 100 Td <{broken_hex} Tj ({valid}) Tj ET"
    ))
}
