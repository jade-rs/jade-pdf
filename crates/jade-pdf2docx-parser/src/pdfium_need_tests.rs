//! native抽出要否のheap非確保・旧判定との意味一致を検証する。
use super::*;
use proptest::prelude::*;

use crate::test_allocations as allocations;

/// 本文と破損座標を持つ最小のIRを作る。
fn parsed(preview: String, texts: Vec<String>) -> ParsedDocument {
    let mut parsed = parse_pdf_bytes_pure_with_budget(
        "need.pdf".into(),
        b"%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\n",
        &ParseLimits::default(),
        &DecodeBudget::new(&ParseLimits::default()),
    )
    .unwrap();
    parsed.text_preview = preview;
    parsed.text_elements = texts
        .into_iter()
        .map(|text| ParsedTextElement {
            page: usize::MAX,
            bounds: crate::ParsedBounds {
                x0: f32::NAN,
                y0: 0.,
                x1: 1.,
                y1: 1.,
            },
            font_size: 10.,
            font_name: None,
            fill_color: None,
            text,
        })
        .collect();
    parsed
}

/// fixture生成とassertionを除き、巨大な最初の要素の判定中にheapを要求しない。
#[test]
fn huge_first_element_requires_no_heap_requests() {
    let parsed = parsed(String::new(), vec!["日".repeat(1024 * 1024)]);
    let budget = DecodeBudget::new(&ParseLimits::default());
    let (needs, requests) =
        allocations::measure(|| needs_pdfium_text_extraction(&budget, b"", &parsed));
    assert!(needs);
    assert_eq!(requests, (0, 0));
}

/// 多数の空/短い要素もheapサンプルを作らず、要素があればnativeへ委譲する。
#[test]
fn empty_and_short_elements_require_no_heap_requests() {
    for texts in [vec![String::new(); 8192], vec!["a".into(); 8192]] {
        let parsed = parsed(String::new(), texts);
        let budget = DecodeBudget::new(&ParseLimits::default());
        let (needs, requests) =
            allocations::measure(|| needs_pdfium_text_extraction(&budget, b"", &parsed));
        assert!(needs);
        assert_eq!(requests, (0, 0));
    }
}

/// 測定器は初回/ゼロ初期化/再確保を捕捉し、測定外の確保を数えない。
#[test]
fn allocation_probe_detects_requests_and_resets_scope() {
    let (bytes, requests) = allocations::measure(|| {
        let mut bytes = vec![0u8; 32];
        bytes.reserve_exact(128);
        bytes
    });
    assert!(requests.0 >= 2 && requests.1 >= 192);
    drop(bytes);
    let (value, requests) = allocations::measure(|| 42);
    assert_eq!((value, requests), (42, (0, 0)));
}

/// 破損文字・長い空白の後の本文も、previewの存在だけでheap確保なしに委譲する。
#[test]
fn preview_presence_requires_no_heap_requests() {
    for preview in [
        "ウィキペディア Wikipedia 日本語の本文".into(),
        "\0\u{000c}ﾔ\u{000e}�\u{0007}�".into(),
        "abc���def".into(),
        format!("{}日", " ".repeat(8192)),
        "\t\r\n\u{2003}".into(),
        String::new(),
    ] {
        let expected = !preview.trim().is_empty();
        let parsed = parsed(preview, Vec::new());
        let budget = DecodeBudget::new(&ParseLimits::default());
        let (needs, requests) =
            allocations::measure(|| needs_pdfium_text_extraction(&budget, b"", &parsed));
        assert_eq!(needs, expected);
        assert_eq!(requests, (0, 0));
    }
}

/// 空白preview・空要素・正常/破損幾何でも存在判定とFont fallbackの意味を保持する。
#[test]
fn empty_text_and_font_fallback_match_legacy() {
    for texts in [Vec::new(), vec![String::new()], vec!["日".into()]] {
        for valid_geometry in [false, true] {
            let mut parsed = parsed("\t\r\n\u{2003}".into(), texts.clone());
            if valid_geometry {
                parsed.pages.push(ParsedPage {
                    page_number: 0,
                    width: 100.,
                    height: 100.,
                    rotation: 0,
                });
                for element in &mut parsed.text_elements {
                    element.page = 0;
                    element.bounds = crate::ParsedBounds {
                        x0: 0.,
                        y0: 0.,
                        x1: 1.,
                        y1: 1.,
                    };
                }
            }
            for font in [false, true] {
                for show in [false, true] {
                    let bytes = content_bytes(font, show);
                    let budget = DecodeBudget::new(&ParseLimits::default());
                    let expected = !texts.is_empty() || (font && show);
                    assert_eq!(legacy(&budget, &bytes, &parsed), expected);
                    assert_eq!(
                        needs_pdfium_text_extraction(&budget, &bytes, &parsed),
                        expected
                    );
                }
            }
        }
    }
}

/// Font有無と文字描画演算子を切り替え、正確なstream長を持つPDF byteを作る。
fn content_bytes(font: bool, show: bool) -> Vec<u8> {
    let content = if show {
        "BT /F 12 Tf (a) Tj ET"
    } else {
        "BT /F 12 Tf ET"
    };
    format!(
        "%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R /Resources << {} >> >> endobj\n4 0 obj << /Length {} >> stream\n{content}\nendstream\nendobj\n{}",
        if font { "/Font << /F 5 0 R >>" } else { "" },
        content.len(),
        if font { "5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n" } else { "" }
    )
    .into_bytes()
}

/// 旧実装が先頭4096文字から判定する破損割合を比較用に固定する。
fn legacy_corrupt(text: &str) -> bool {
    let mut total = 0;
    let mut bad = 0;
    let mut replacements = 0;
    let mut nul = 0;
    for ch in text.chars().take(4096) {
        if ch.is_whitespace() {
            continue;
        }
        total += 1;
        if ch == '\u{fffd}' {
            replacements += 1;
            bad += 1;
            continue;
        }
        if ch == '\0' {
            nul += 1;
            bad += 1;
            continue;
        }
        if ch.is_control() {
            bad += 1;
        }
    }
    total != 0 && (nul > 0 || replacements >= 3 || bad * 20 >= total)
}

/// 旧判定の本文存在・破損・座標・未使用Font fallbackをそのまま比較する。
fn legacy(budget: &DecodeBudget, bytes: &[u8], parsed: &ParsedDocument) -> bool {
    let mut sample = String::new();
    for element in &parsed.text_elements {
        if !sample.is_empty() {
            sample.push('\n');
        }
        sample.push_str(&element.text);
        if sample.chars().count() >= 4096 {
            break;
        }
    }
    if legacy_corrupt(&parsed.text_preview)
        || legacy_corrupt(&sample)
        || text_elements_have_invalid_geometry(parsed)
    {
        return true;
    }
    if !parsed.text_elements.is_empty() || !parsed.text_preview.trim().is_empty() {
        return true;
    }
    bytes
        .windows(b"/Font".len())
        .any(|window| window == b"/Font")
        && crate::parser::pdf_bytes_have_text_showing_operators(budget, bytes)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// Unicode/制御文字/空白/空要素とFont・文字演算子の組み合わせで旧判定と一致する。
    #[test]
    fn decisions_match_legacy_semantics(
        preview in prop_oneof![Just(Vec::new()), Just(vec![' ', '\t', '\n', '\u{2003}']), prop::collection::vec(any::<char>(), 0..128)],
        texts in prop_oneof![Just(Vec::new()), Just(vec![Vec::new()]), prop::collection::vec(prop::collection::vec(any::<char>(), 0..128), 0..12)],
        font in any::<bool>(),
        show in any::<bool>(),
    ) {
        let parsed = parsed(preview.into_iter().collect(), texts.into_iter().map(|t| t.into_iter().collect()).collect());
        let bytes = content_bytes(font, show);
        let expected = legacy(&DecodeBudget::new(&ParseLimits::default()), &bytes, &parsed);
        prop_assert_eq!(needs_pdfium_text_extraction(&DecodeBudget::new(&ParseLimits::default()), &bytes, &parsed), expected);
    }
}
