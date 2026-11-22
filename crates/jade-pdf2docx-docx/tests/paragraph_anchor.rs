use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use jade_pdf2docx_parser::parse_pdf_bytes;
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

/// PDF由来の実中心と、出力Word段落領域の中心を独立に比較する。
fn check_centre(left: f32) {
    let text = format!("Shifted{}", "x".repeat(53));
    let bytes = owned_pdf(left, &text);
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
        (actual - expected).abs() <= 0.05,
        "source centre {expected}pt moved to {actual}pt; {paragraph:?}"
    );
}

/// ページ寸法・余白と段落indentを同じOOXML座標系で読む。
fn geometry(bytes: Vec<u8>) -> (f64, f64, f64, Vec<ParagraphGeometry>) {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
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
    let content =
        format!("BT /F1 10 Tf 72 740 Td (Anchor) Tj ET BT /F1 10 Tf {left} 650 Td ({text}) Tj ET");
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
