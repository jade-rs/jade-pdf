//! 回転後に下端へ来た本文の数字を、未確認のページ番号として消さない。
use jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes;
use std::io::{Cursor, Read};

#[allow(dead_code)]
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod fixture;

/// 文字とCropBoxを同時に平行移動した、同じ見た目の所有PDFを作る。
fn source(rotation: i32, cropped: bool) -> Vec<u8> {
    shifted_source(rotation, cropped, 0)
}

/// 本文を少し左右へ動かし、下端の数字判定をまたぐ入力も同じ所有原本で作る。
fn shifted_source(rotation: i32, cropped: bool, shift: i32) -> Vec<u8> {
    let (dx, dy) = if cropped { (10, 50) } else { (0, 0) };
    let media = if cropped {
        "[0 0 600 800]"
    } else {
        "[0 0 400 600]"
    };
    let crop = if cropped {
        "/CropBox [10 50 410 650]"
    } else {
        ""
    };
    let content = format!(
        "BT /F 18 Tf 1 0 0 1 {} {} Tm (A) Tj /F 10 Tf 1 0 0 1 {} {} Tm (2) Tj /F 18 Tf 1 0 0 1 {} {} Tm (B) Tj ET",
        30 + dx + shift,
        80 + dy,
        41 + dx + shift,
        86 + dy,
        48 + dx + shift,
        80 + dy
    );
    fixture::pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!(
            "<< /Type /Pages /Count 1 /Kids [3 0 R] /MediaBox {media} {crop} /Rotate {rotation} >>"
        ),
        "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>"
            .into(),
        fixture::stream(&content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".into(),
    ])
}

/// 本文に含まれる各文字を一度ずつ数え、XMLやstyleの記号と混同しない。
fn word_glyphs(input: &[u8]) -> Vec<char> {
    let output =
        convert_minimum_pdf_bytes_to_docx_bytes("owned-rotated-body-digits.pdf", input).unwrap();
    glyphs_from_word(output)
}

/// 公開file/bytes両経路の生成物を同じ方法で照合する。
fn glyphs_from_word(output: Vec<u8>) -> Vec<char> {
    let mut zip = zip::ZipArchive::new(Cursor::new(output)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let mut reader = quick_xml::Reader::from_str(&xml);
    let mut in_text = false;
    let mut glyphs = Vec::new();
    loop {
        match reader.read_event().unwrap() {
            quick_xml::events::Event::Start(e) if e.name().as_ref() == b"w:t" => in_text = true,
            quick_xml::events::Event::End(e) if e.name().as_ref() == b"w:t" => in_text = false,
            quick_xml::events::Event::Text(t) if in_text => {
                glyphs.extend(t.decode().unwrap().chars().filter(|c| !c.is_whitespace()))
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    glyphs.sort_unstable();
    glyphs
}

/// 回転後に下端12%へ来た上付き数字も、前後の本文字形と一緒に残す。
#[test]
fn rotated_superscript_between_body_glyphs_is_not_erased_as_a_footer() {
    for cropped in [false, true] {
        assert_eq!(word_glyphs(&source(270, cropped)), ['2', 'A', 'B']);
    }
}

/// 下端のページ番号推測へ入らない三方向の対照も、同じ3文字を保持する。
#[test]
fn other_rotations_keep_the_same_authored_glyph_multiset() {
    for rotation in [0, 90, 180] {
        for cropped in [false, true] {
            assert_eq!(word_glyphs(&source(rotation, cropped)), ['2', 'A', 'B']);
        }
    }
}

/// native file経路でも同じ原本の3文字を維持し、元の入力を変更しない。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_file_keeps_the_rotated_body_digit() {
    use jade_pdf2docx_docx::{MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx};
    for rotation in [0, 90, 180, 270] {
        for cropped in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let input = source(rotation, cropped);
            let path = dir.path().join("owned-source.pdf");
            let output = dir.path().join("owned-word.docx");
            std::fs::write(&path, &input).unwrap();
            convert_minimum_pdf_to_docx(&MinimumPdfToDocxRequest {
                input_pdf_path: path.clone(),
                output_docx_path: output.clone(),
                ocr_text_lines: None,
                ocr_table: None,
                clean_output: true,
            })
            .unwrap();
            assert_eq!(std::fs::read(path).unwrap(), input);
            assert_eq!(
                glyphs_from_word(std::fs::read(output).unwrap()),
                ['2', 'A', 'B']
            );
        }
    }
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 下端への移動・四方向の回転・切り抜きの有無を変えても、本文の数字を一度だけ残す。
    #[test]
    fn transformed_body_keeps_every_authored_glyph(turn in 0_u8..=3,cropped in any::<bool>(),shift in -10_i32..=10) {
        prop_assert_eq!(word_glyphs(&shifted_source(i32::from(turn)*90,cropped,shift)),['2','A','B']);
    }
}
