//! 同じUnicode文字列でも異なる元glyph列を、実PDFから失わずに出力へ渡す。
use crate::type1_embedding_tests::{charstring, encrypt};

/// 既知の小整数を、テスト側の独立Type1 encoderで書く。
fn number(out: &mut Vec<u8>, value: i32) {
    match value {
        -107..=107 => out.push((value + 139) as u8),
        108..=1131 => {
            let v = value - 108;
            out.extend_from_slice(&[(247 + v / 256) as u8, (v % 256) as u8]);
        }
        -1131..=-108 => {
            let v = -value - 108;
            out.extend_from_slice(&[(251 + v / 256) as u8, (v % 256) as u8]);
        }
        _ => panic!("owned range"),
    }
}

/// 元advanceと同じ幅の所有三角形を、一つのType1 glyphにする。
fn triangle(width: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    number(&mut bytes, 0);
    number(&mut bytes, width);
    bytes.push(13);
    number(&mut bytes, 0);
    number(&mut bytes, 0);
    bytes.push(21);
    for [x, y] in [[width / 2, 700], [width - width / 2, -700], [-width, 0]] {
        number(&mut bytes, x);
        number(&mut bytes, y);
        bytes.push(5);
    }
    bytes.extend_from_slice(&[9, 14]);
    charstring(&bytes)
}

/// f幅300とff幅500を持ち、埋め込み許可も明示する所有fontを作る。
pub(crate) fn font_program() -> (Vec<u8>, usize, usize, usize) {
    let clear=b"%!PS-AdobeFont-1.0: JadeOwnedClusters 1.0\n\
        % Copyright Jade owned; these test outlines may be embedded and modified.\n\
        11 dict begin /FontType 1 def /FontName /JadeOwnedClusters def\n\
        /FontMatrix [0.001 0 0 0.001 0 0] readonly def /FontBBox [0 0 500 700] readonly def\n\
        /FontInfo 4 dict dup begin /FSType 0 def /FullName (JadeOwnedClusters) def\n\
        /FamilyName (JadeOwnedClusters) def /Notice (Copyright Jade owned; owned test font.) def end readonly def\n\
        /Encoding 256 array 0 1 255 {1 index exch /.notdef put} for dup 65 /f put dup 66 /ff put readonly def\n\
        currentdict end currentfile eexec\n";
    let mut private = b"abcddup /Private 8 dict dup begin\n\
        /RD {string currentfile exch readstring pop} executeonly def\n\
        /ND {noaccess def} executeonly def /NP {noaccess put} executeonly def\n\
        /lenIV 4 def /Subrs 0 array def end readonly put\n\
        dup /CharStrings 3 dict dup begin\n"
        .to_vec();
    for (name, program) in [
        (".notdef", charstring(&[139, 248, 236, 13, 14])),
        ("f", triangle(300)),
        ("ff", triangle(500)),
    ] {
        private.extend_from_slice(format!("/{name} {} RD ", program.len()).as_bytes());
        private.extend_from_slice(&program);
        private.extend_from_slice(b" ND\n");
    }
    private.extend_from_slice(
        b"end readonly put dup /FontName get exch definefont pop mark currentfile closefile\n",
    );
    let cipher = encrypt(&private, 55665);
    let tail = format!("\n{}\ncleartomark\n", "0".repeat(512));
    let mut source = clear.to_vec();
    source.extend_from_slice(&cipher);
    source.extend_from_slice(tail.as_bytes());
    (source, clear.len(), cipher.len(), tail.len())
}

/// Lengthを持つ所有streamを作り、バイナリ内の改行を境界に使わない。
fn stream(bytes: &[u8], attributes: &str) -> Vec<u8> {
    let mut out = format!("<< /Length {} {attributes} >>\nstream\n", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\nendstream");
    out
}

/// 上の行はcode B一個、下の行はcode A二個で、両方の原文Unicodeをffとする。
fn pdf() -> Vec<u8> {
    let (font, first, second, third) = font_program();
    let content = b"BT /F1 20 Tf 20 80 Td (B) Tj 0 -40 Td (AA) Tj ET";
    let cmap=b"begincmap\n1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<41> <0066>\n<42> <00660066>\nendbfchar\nendcmap";
    let objects=[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 120] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        stream(content,""),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /JadeOwnedClusters /FirstChar 65 /LastChar 66 /Widths [300 500] /FontDescriptor 6 0 R /ToUnicode 8 0 R >>".to_vec(),
        b"<< /Type /FontDescriptor /FontName /JadeOwnedClusters /Flags 32 /FontBBox [0 0 500 700] /Ascent 700 /Descent 0 /CapHeight 700 /StemV 80 /FontFile 7 0 R >>".to_vec(),
        stream(&font,&format!("/Length1 {first} /Length2 {second} /Length3 {third}")),stream(cmap,"")];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        bytes.extend_from_slice(obj);
        bytes.extend_from_slice(b"\nendobj\n");
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 9\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer << /Size 9 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}

/// 現在のIRのfont選択はfamily名なので、一つの合字と二つのfへ同じrequired GSUBを当てない。
#[test]
fn identical_unicode_retains_distinct_encoded_glyph_clusters_for_docx() {
    let parsed = crate::parse_pdf_bytes("owned-source-clusters.pdf", &pdf()).unwrap();
    assert_eq!(parsed.text_elements.len(), 2);
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>(),
        ["ff", "ff"]
    );
    assert_ne!(
        parsed.text_elements[0].font_name, parsed.text_elements[1].font_name,
        "one source ff glyph and two source f glyphs need distinct effective font routes"
    );
    for element in &parsed.text_elements {
        let font = parsed
            .embedded_fonts
            .iter()
            .find(|font| Some(&font.font_name) == element.font_name.as_ref())
            .expect("each routed original glyph cluster must have a portable font");
        assert_eq!(font.bytes.get(..4), Some(b"OTTO".as_slice()));
    }
    for (element, expected) in parsed.text_elements.iter().zip([10., 12.]) {
        assert!(
            (element.bounds.x1 - element.bounds.x0 - expected).abs() < 0.001,
            "original width must survive: {element:?}"
        );
    }
}
