//! 出典を固定した公開PDFを実CLIで変換し、独立抽出した本文とページ境界を検証する。
use quick_xml::{Reader, events::Event};
use std::{fs, io::Read, path::Path, process::Command};

/// pdfLaTeXの単一ページ文書を変換する。
#[test]
fn public_pdflatex_text_to_docx() {
    assert_public_conversion("pdflatex-text", 1);
}

/// LibreOfficeの単一ページ文書を変換する。
#[test]
fn public_libreoffice_text_to_docx() {
    assert_public_conversion("libreoffice-text", 1);
}

/// 四ページ文書の本文とページ順序を最後まで保持する。
#[test]
fn public_four_page_text_to_docx() {
    assert_public_conversion("pdflatex-four-pages", 4);
}

/// 実CLIの成功、DOCX構造、ページ別本文の一致、入力の不変性を確認する。
fn assert_public_conversion(name: &str, page_count: usize) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let input = root.join("input").join(format!("{name}.pdf"));
    let original = fs::read(&input).expect("audited public PDF must exist");
    let expected = fs::read_to_string(root.join("expected").join(format!("{name}.txt")))
        .expect("independently extracted text must exist");
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("converted.docx");
    let result = Command::new(env!("CARGO_BIN_EXE_jade-pdf2docx"))
        .arg(&input)
        .arg(&output)
        .args(["--timeout-ms", "30000"])
        .output()
        .expect("conversion CLI must start");
    assert!(
        result.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(&input).unwrap(), original, "source PDF changed");
    let mut archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    for part in ["[Content_Types].xml", "_rels/.rels", "word/styles.xml"] {
        assert!(archive.by_name(part).is_ok(), "missing DOCX part: {part}");
    }
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let actual_pages = document_pages(&xml);
    let expected_pages: Vec<_> = expected
        .split('\u{c}')
        .filter(|page| !page.trim().is_empty())
        .collect();
    assert_eq!(
        expected_pages.len(),
        page_count,
        "independent PDF page count"
    );
    assert_eq!(actual_pages.len(), page_count, "DOCX page boundaries");
    for (page, (actual, expected)) in actual_pages.iter().zip(expected_pages).enumerate() {
        assert!(!actual.trim().is_empty(), "empty output page {}", page + 1);
        assert_eq!(
            compact(actual),
            compact(&expected.replace("-\n", "")),
            "{name}, page {}",
            page + 1
        );
    }
}

/// OOXML本文のテキストを段落順で読み、明示的な改ページで分割する。
fn document_pages(xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(xml);
    let mut pages = vec![String::new()];
    loop {
        match reader.read_event().expect("valid document XML") {
            Event::Start(element) if element.name().as_ref() == b"w:t" => {
                let raw = reader.read_text(element.name()).unwrap();
                pages
                    .last_mut()
                    .unwrap()
                    .push_str(&quick_xml::escape::unescape(&raw.decode().unwrap()).unwrap());
            }
            Event::Empty(element) if element.name().as_ref() == b"w:br" => {
                if element.attributes().any(|attribute| {
                    let attribute = attribute.unwrap();
                    attribute.key.as_ref() == b"w:type" && attribute.value.as_ref() == b"page"
                }) {
                    pages.push(String::new());
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    pages
}

/// PDFの行折返しやDOCXのrun分割による空白を比較から除外する。
fn compact(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}
