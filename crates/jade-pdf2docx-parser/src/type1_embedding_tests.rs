//! 外形とadvanceが既知の所有Type1を、ファイルfixtureを増やさず検査する。
use crate::parse_pdf_bytes;

/// 仕様の二つのseedを使い、所有のcharstring/eexecを暗号化する。
fn encrypt(bytes: &[u8], mut seed: u16) -> Vec<u8> {
    bytes
        .iter()
        .map(|plain| {
            let cipher = plain ^ (seed >> 8) as u8;
            seed = u16::from(cipher)
                .wrapping_add(seed)
                .wrapping_mul(52845)
                .wrapping_add(22719);
            cipher
        })
        .collect()
}

/// lenIV四byteと既知のType1演算だけからglyphを作る。
fn charstring(program: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 4];
    bytes.extend_from_slice(program);
    encrypt(&bytes, 4330)
}

/// 幅600・高さ700の三角形Aと空の.notdefを持つ、所有のType1を作る。
pub(crate) fn program() -> (Vec<u8>, usize, usize, usize) {
    let clear = b"%!PS-AdobeFont-1.0: JadeOwnedType1 1.0\n\
        11 dict begin /FontName /JadeOwnedType1 def /FontType 1 def\n\
        /FontMatrix [0.001 0 0 0.001 0 0] readonly def /FontBBox [0 0 600 700] readonly def\n\
        /FontInfo 6 dict dup begin /FullName (JadeOwnedType1) def /FamilyName (JadeOwned) def\n\
        /Weight (Regular) def /ItalicAngle 0 def /isFixedPitch true def /FSType 0 def end readonly def\n\
        /Encoding 256 array 0 1 255 {1 index exch /.notdef put} for dup 65 /A put readonly def\n\
        currentdict end currentfile eexec\n";
    let notdef = charstring(&[139, 248, 236, 13, 14]);
    let a = charstring(&[
        139, 248, 236, 13, 139, 139, 21, 247, 192, 249, 80, 5, 247, 192, 253, 80, 5, 252, 236, 139,
        5, 9, 14,
    ]);
    let mut private = b"abcddup /Private 8 dict dup begin\n\
        /RD {string currentfile exch readstring pop} executeonly def\n\
        /ND {noaccess def} executeonly def /NP {noaccess put} executeonly def\n\
        /lenIV 4 def /Subrs 0 array def end readonly put\n\
        dup /CharStrings 2 dict dup begin\n"
        .to_vec();
    for (name, bytes) in [(".notdef", notdef), ("A", a)] {
        private.extend_from_slice(format!("/{name} {} RD ", bytes.len()).as_bytes());
        private.extend_from_slice(&bytes);
        private.extend_from_slice(b" ND\n");
    }
    private.extend_from_slice(
        b"end readonly put dup /FontName get exch definefont pop mark currentfile closefile\n",
    );
    let encrypted = encrypt(&private, 55665);
    let trailer = format!("\n{}\ncleartomark\n", "0".repeat(512));
    let mut bytes = clear.to_vec();
    bytes.extend_from_slice(&encrypted);
    bytes.extend_from_slice(trailer.as_bytes());
    (bytes, clear.len(), encrypted.len(), trailer.len())
}

/// 正式なRoot/xrefと、Lengthで区切ったバイナリFontFileを持つPDFを作る。
fn owned_pdf() -> Vec<u8> {
    let (font, length1, length2, length3) = program();
    let content = b"BT /F1 20 Tf 20 40 Td (AA) Tj ET";
    let mut stream = format!(
        "<< /Length {} /Length1 {length1} /Length2 {length2} /Length3 {length3} >>\nstream\n",
        font.len()
    )
    .into_bytes();
    stream.extend_from_slice(&font);
    stream.extend_from_slice(b"\nendstream");
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 120] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), std::str::from_utf8(content).unwrap()).into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /JadeOwnedType1 /FirstChar 65 /LastChar 65 /Widths [600] /FontDescriptor 6 0 R >>".to_vec(),
        b"<< /Type /FontDescriptor /FontName /JadeOwnedType1 /Flags 32 /FontBBox [0 0 600 700] /ItalicAngle 0 /Ascent 700 /Descent 0 /CapHeight 700 /StemV 80 /FontFile 7 0 R >>".to_vec(),
        stream,
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        bytes.extend_from_slice(body);
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

/// Unicode本文が読めても、代替fontに頼らず元の外形をDOCXへ渡せなければ未完了とする。
#[test]
fn owned_type1_preserves_drawable_font_for_docx() {
    let parsed = parse_pdf_bytes("owned-type1-outline.pdf", &owned_pdf()).unwrap();
    assert_eq!(parsed.page_count, 1);
    assert_eq!(parsed.text_elements[0].text, "AA");
    assert_eq!(parsed.text_elements[0].font_size, 20.0);
    assert_eq!(
        parsed.text_elements[0].bounds.x1 - parsed.text_elements[0].bounds.x0,
        24.0
    );
    assert_eq!(
        parsed.embedded_fonts.len(),
        1,
        "Type1 glyph outlines must survive as a portable embedded font"
    );
    assert_eq!(
        parsed.embedded_fonts[0].bytes.get(..4),
        Some(b"OTTO".as_slice())
    );
}
