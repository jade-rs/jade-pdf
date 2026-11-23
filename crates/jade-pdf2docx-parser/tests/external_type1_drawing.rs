//! 既存の公開fixtureからToUnicodeだけを外した一時PDFで、完全な描画用Type1を検証する。
use jade_pdf2docx_parser::{ParseOptions, parse_pdf_bytes_with_options};
use std::{fs, path::Path};

/// 独立goldとの比較では空白とUnicodeの明示的なpresentation ligatureだけを正規化する。
fn compact(text: &str) -> String {
    text.replace('\u{fb00}', "ff")
        .replace('\u{fb01}', "fi")
        .replace('\u{fb02}', "fl")
        .replace('\u{fb03}', "ffi")
        .replace('\u{fb04}', "ffl")
        .chars()
        .filter(|scalar| !scalar.is_whitespace())
        .collect()
}

/// レビュー済みの一時入力は外部指定とし、追加PDFやfont programをGitへ保存しない。
#[test]
#[ignore = "requires reviewed ToUnicode-removed PDFs in JADE_TYPE1_DRAWING_CONTROLS"]
fn real_embedded_fonts_recover_all_source_pages_without_tounicode() {
    let directory =
        std::env::var_os("JADE_TYPE1_DRAWING_CONTROLS").expect("reviewed input directory");
    let directory = Path::new(&directory);
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    for (name, pages) in [("pdflatex-text", 1), ("pdflatex-four-pages", 4)] {
        let input = directory.join(format!("{name}-without-ToUnicode.pdf"));
        let bytes = fs::read(&input).unwrap();
        let expected =
            fs::read_to_string(fixtures.join("expected").join(format!("{name}.txt"))).unwrap();
        let parsed =
            parse_pdf_bytes_with_options(&input, &bytes, &ParseOptions::default()).unwrap();
        let actual = (0..pages)
            .map(|page| {
                parsed
                    .text_elements
                    .iter()
                    .filter(|element| element.page == page)
                    .map(|element| element.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(
            parsed.quality.issues.is_empty(),
            "{name}: {:?}",
            parsed.quality
        );
        assert_eq!(parsed.pages.len(), pages);
        let expected = expected
            .split('\u{c}')
            .filter(|page| !page.trim().is_empty())
            .collect::<Vec<_>>();
        assert_eq!(expected.len(), pages);
        for (page, (actual, expected)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(
                compact(actual),
                compact(expected),
                "{name}, source page {}",
                page + 1
            );
        }
        parse_pdf_bytes_with_options(
            &input,
            &bytes,
            &ParseOptions {
                strict: true,
                ..Default::default()
            },
        )
        .unwrap();
    }
}

/// 空白を全削除した比較だけでは見逃す、完全な原文の単語境界も検査する。
#[test]
#[ignore = "requires reviewed ToUnicode-removed PDFs in JADE_TYPE1_DRAWING_CONTROLS"]
fn real_embedded_fonts_preserve_source_word_boundaries() {
    let directory =
        std::env::var_os("JADE_TYPE1_DRAWING_CONTROLS").expect("reviewed input directory");
    let directory = Path::new(&directory);
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    for name in ["pdflatex-text", "pdflatex-four-pages"] {
        let input = directory.join(format!("{name}-without-ToUnicode.pdf"));
        let bytes = fs::read(&input).unwrap();
        let expected =
            fs::read_to_string(fixtures.join("expected").join(format!("{name}.txt"))).unwrap();
        let parsed =
            parse_pdf_bytes_with_options(&input, &bytes, &ParseOptions::default()).unwrap();
        let expected = expected
            .split('\u{c}')
            .filter(|page| !page.trim().is_empty())
            .collect::<Vec<_>>();
        for (page, expected) in expected.iter().enumerate() {
            let actual = parsed
                .text_elements
                .iter()
                .filter(|element| element.page == page)
                .map(|element| element.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let actual = actual
                .replace('\u{fb00}', "ff")
                .replace('\u{fb01}', "fi")
                .replace('\u{fb02}', "fl")
                .replace('\u{fb03}', "ffi")
                .replace('\u{fb04}', "ffl");
            assert_eq!(
                actual.split_whitespace().collect::<Vec<_>>(),
                expected.split_whitespace().collect::<Vec<_>>(),
                "{name}, source page {}",
                page + 1
            );
        }
    }
}

/// native経路も同じ原文の印刷された行末hyphenを保持し、制御文字へ置き換えない。
#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires reviewed Type1 controls and the pinned native Pdfium SDK"]
fn native_file_preserves_printed_line_end_hyphens() {
    let directory =
        std::env::var_os("JADE_TYPE1_DRAWING_CONTROLS").expect("reviewed input directory");
    let input = Path::new(&directory).join("pdflatex-text-without-ToUnicode.pdf");
    let parsed = jade_pdf2docx_parser::parse_pdf(&input).unwrap();
    let actual = parsed
        .text_elements
        .iter()
        .map(|element| element.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let actual = actual
        .replace('\u{fb00}', "ff")
        .replace('\u{fb01}', "fi")
        .replace('\u{fb02}', "fl")
        .replace('\u{fb03}', "ffi")
        .replace('\u{fb04}', "ffl");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let expected = fs::read_to_string(fixtures.join("expected/pdflatex-text.txt")).unwrap();
    assert_eq!(
        actual.split_whitespace().collect::<Vec<_>>(),
        expected.split_whitespace().collect::<Vec<_>>()
    );
    assert!(!parsed.text_preview.contains('\u{2}'));
    assert!(parsed.text_preview.contains("taki-"));
}
