use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;
use quick_xml::{Reader, events::Event};
use std::io::{Cursor, Read};

/// 中央寄せ段落の実際の領域をOOXMLから読む。
#[derive(Default, Debug)]
struct ParagraphGeometry {
    /// runをそのまま連結した本文。
    text: String,
    /// 段落配置。
    alignment: String,
    /// 左indent（twips）。
    left: f64,
    /// 右indent（twips）。
    right: f64,
}

/// 真のページ中央は保持する。
#[test]
fn exact_page_centre_remains_fixed() {
    check_centre(126.0);
}

/// ページ中央より左にある行をページ中央へ吸着させない。
#[test]
fn near_centre_line_keeps_its_left_shift() {
    check_centre(90.0);
}

/// セクション原点より右への補正も正の左indentで表現できる。
#[test]
fn near_centre_line_keeps_its_right_shift() {
    check_centre(170.0);
}

/// 中心の補正だけで、同じフォントが使える書き込み領域を狭めない。
#[test]
fn centre_shift_keeps_legacy_writable_width() {
    let text = format!("Shifted{}", "x".repeat(53));
    let bytes = owned_pdf(90.0, &text);
    let docx = convert_minimum_pdf_bytes_to_docx_bytes("owned-width.pdf", &bytes).unwrap();
    let (width, margin_left, margin_right, paragraphs) = geometry(docx);
    let paragraph = paragraphs.iter().find(|p| p.text == text).unwrap();
    let legacy_width = width - margin_right - 2.0 * margin_left;
    let actual_width = width - margin_right - margin_left - paragraph.left - paragraph.right;
    assert!(
        actual_width >= legacy_width,
        "centre correction narrowed the writable region from {legacy_width} to {actual_width} twips"
    );
}

/// 中央見出しが本文より左へ張り出しても、実際のセクション余白を使う。
#[test]
fn centred_heading_outside_body_anchor_keeps_its_centre() {
    let text = "x".repeat(75);
    check_geometry(owned_pdf(60.0, &text), &text);
}

/// OOXML余白と同じf32丸めを使い、境界で中心誤差を重ねない。
#[test]
fn fractional_section_margin_uses_serialized_rounding() {
    let text = format!("Shifted{}", "x".repeat(53));
    for anchor in [72.025, 72.075] {
        check_geometry(owned_pdf_with_anchor(90.0255, &text, anchor), &text);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// 見出しが本文より左へ出ても、本文の物理座標と通常配置の対照を保つ。
    #[test]
    fn body_start_survives_a_heading_overhang(
        heading_left in 50.0_f32..70.0,
        body_left in 72.0_f32..108.0,
    ) {
        let heading = "x".repeat(75);
        check_body_start(owned_pdf_with_anchor(126.0, &heading, body_left));
        check_body_start(owned_pdf_with_anchor(heading_left, &heading, body_left));
    }
}

/// 左側の装飾線による余白が、本文を同じ位置へ吸着させない。
#[test]
fn body_start_survives_a_graphic_to_its_left() {
    let bytes = owned_pdf_content("BT /F1 10 Tf 72 740 Td (Anchor) Tj ET 30 700 m 40 700 l S");
    let parsed = parse_pdf_bytes("owned-graphic.pdf", &bytes).unwrap();
    assert!(
        !parsed.graphics.is_empty(),
        "fixture must contain a real graphic"
    );
    check_body_start(bytes);
}

/// 見出しが余白を変えても、表のグリッド開始位置を独立に保持する。
#[test]
fn table_start_survives_a_heading_overhang() {
    for heading in [
        String::new(),
        format!("BT /F1 10 Tf 60 650 Td ({}) Tj ET", "x".repeat(75)),
    ] {
        let content = format!(
            "BT /F1 10 Tf 72 740 Td (Anchor) Tj ET {heading} \
             100 500 m 300 500 l S 100 460 m 300 460 l S \
             100 460 m 100 500 l S 200 460 m 200 500 l S 300 460 m 300 500 l S \
             BT /F1 10 Tf 110 480 Td (A) Tj ET BT /F1 10 Tf 210 480 Td (B) Tj ET"
        );
        let bytes = owned_pdf_content(&content);
        let docx = convert_minimum_pdf_bytes_to_docx_bytes("owned-table.pdf", &bytes).unwrap();
        let xml = document_xml(&docx);
        let (_, margin_left, _, _) = geometry(docx);
        let mut reader = Reader::from_str(&xml);
        let mut table_seen = false;
        let mut indent = 0.0_f64;
        loop {
            match reader.read_event().unwrap() {
                Event::Start(e) if e.name().as_ref() == b"w:tbl" => table_seen = true,
                Event::Empty(e) if e.name().as_ref() == b"w:tblInd" => {
                    let value = e
                        .attributes()
                        .map(Result::unwrap)
                        .find(|a| a.key.as_ref() == b"w:w")
                        .unwrap();
                    indent = std::str::from_utf8(&value.value).unwrap().parse().unwrap();
                }
                Event::Eof => break,
                _ => {}
            }
        }
        assert!(table_seen, "owned ruled grid must be an actual DOCX table");
        let actual = (margin_left + indent) / 20.0;
        assert!(
            (actual - 100.0).abs() <= 0.05,
            "authored table x100 moved to {actual}pt"
        );
    }
}

/// 実際のOOXML余白と段落indentから本文のページ内座標を独立に読む。
fn check_body_start(bytes: Vec<u8>) {
    let parsed = parse_pdf_bytes("owned-body.pdf", &bytes).unwrap();
    let source = parsed
        .text_elements
        .iter()
        .find(|e| e.text == "Anchor")
        .unwrap();
    let expected = f64::from(source.bounds.x0);
    let docx = convert_minimum_pdf_bytes_to_docx_bytes("owned-body.pdf", &bytes).unwrap();
    let (_, margin_left, _, paragraphs) = geometry(docx);
    let body = paragraphs.iter().find(|p| p.text == "Anchor").unwrap();
    assert_ne!(
        body.alignment, "center",
        "body control must remain left aligned"
    );
    assert_ne!(
        body.alignment, "right",
        "body control must remain left aligned"
    );
    let actual = (margin_left + body.left) / 20.0;
    assert!(
        (actual - expected).abs() <= 0.05001,
        "source body left {expected}pt moved to {actual}pt; margin {margin_left}twips; {body:?}"
    );
}

/// PDF由来の実中心と、出力Word段落領域の中心を独立に比較する。
fn check_centre(left: f32) {
    let text = format!("Shifted{}", "x".repeat(53));
    let bytes = owned_pdf(left, &text);
    check_geometry(bytes, &text);
}

/// 生成原本の行中心と、実際に保存されたOOXMLの段落中心を比較する。
fn check_geometry(bytes: Vec<u8>, text: &str) {
    let parsed = parse_pdf_bytes("owned-centre.pdf", &bytes).unwrap();
    let source = parsed
        .text_elements
        .iter()
        .find(|e| e.text == text)
        .unwrap();
    let expected = f64::from(source.bounds.x0 + source.bounds.x1) / 2.0;
    let docx = convert_minimum_pdf_bytes_to_docx_bytes("owned-centre.pdf", &bytes).unwrap();
    let (width, margin_left, margin_right, paragraphs) = geometry(docx);
    let paragraph = paragraphs.iter().find(|p| p.text == text).unwrap();
    assert_eq!(
        paragraph.alignment, "center",
        "fixture must exercise the existing centre inference"
    );
    let actual = (margin_left + paragraph.left + width - margin_right - paragraph.right) / 40.0;
    assert!(
        (actual - expected).abs() <= 0.0251,
        "source centre {expected}pt moved to {actual}pt; {paragraph:?}"
    );
}

/// OOXML本文を借用したZIPバイト列から読む。
fn document_xml(bytes: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// ページ寸法・余白と段落indentを同じOOXML座標系で読む。
fn geometry(bytes: Vec<u8>) -> (f64, f64, f64, Vec<ParagraphGeometry>) {
    let xml = document_xml(&bytes);
    let mut reader = Reader::from_str(&xml);
    let mut current: Option<ParagraphGeometry> = None;
    let mut paragraphs = Vec::new();
    let (mut width, mut left, mut right) = (0.0, 0.0, 0.0);
    let mut in_text = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) | Event::Empty(e) => {
                let attrs = e
                    .attributes()
                    .map(|a| {
                        let a = a.unwrap();
                        (
                            a.key.as_ref().to_vec(),
                            String::from_utf8(a.value.into_owned()).unwrap(),
                        )
                    })
                    .collect::<Vec<_>>();
                let value = |name: &[u8]| {
                    attrs
                        .iter()
                        .find(|(k, _)| k == name)
                        .map(|(_, v)| v.as_str())
                        .unwrap_or("0")
                };
                match e.name().as_ref() {
                    b"w:p" => current = Some(ParagraphGeometry::default()),
                    b"w:t" => in_text = true,
                    b"w:jc" => {
                        if let Some(p) = current.as_mut() {
                            p.alignment = value(b"w:val").into();
                        }
                    }
                    b"w:ind" => {
                        if let Some(p) = current.as_mut() {
                            p.left = value(b"w:left").parse().unwrap();
                            p.right = value(b"w:right").parse().unwrap();
                        }
                    }
                    b"w:pgSz" => width = value(b"w:w").parse().unwrap(),
                    b"w:pgMar" => {
                        left = value(b"w:left").parse().unwrap();
                        right = value(b"w:right").parse().unwrap();
                    }
                    _ => {}
                }
            }
            Event::Text(e) if in_text => {
                if let Some(p) = current.as_mut() {
                    p.text.push_str(&e.decode().unwrap());
                }
            }
            Event::End(e) if e.name().as_ref() == b"w:t" => in_text = false,
            Event::End(e) if e.name().as_ref() == b"w:p" => {
                if let Some(p) = current.take() {
                    paragraphs.push(p);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    (width, left, right, paragraphs)
}

/// Courierの既知600/1000幅を持つ所有PDFをメモリ内で作る。入力をGitへ保存しない。
fn owned_pdf(left: f32, text: &str) -> Vec<u8> {
    owned_pdf_with_anchor(left, text, 72.0)
}

/// 本文の開始位置も独立に指定した所有PDFを作る。
fn owned_pdf_with_anchor(left: f32, text: &str, anchor: f32) -> Vec<u8> {
    let content = format!(
        "BT /F1 10 Tf {anchor} 740 Td (Anchor) Tj ET BT /F1 10 Tf {left} 650 Td ({text}) Tj ET"
    );
    owned_pdf_content(&content)
}

/// 単純な自作描画命令を持つ正式なxref付きPDFをメモリ内で作る。
fn owned_pdf_content(content: &str) -> Vec<u8> {
    let objects=["<< /Type /Catalog /Pages 2 0 R >>".into(),"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".into(),format!("<< /Length {} >>\nstream\n{content}\nendstream",content.len()),"<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>".into()];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let start = bytes.len();
    bytes.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer << /Size 6 /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n").as_bytes(),
    );
    bytes
}
