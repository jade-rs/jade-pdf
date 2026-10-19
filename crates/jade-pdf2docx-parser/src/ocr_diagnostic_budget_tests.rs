//! OCR再判定の保存容量不足で既存診断を変更しない回帰。
use super::*;
use crate::{ParseError, decode::DecodeBudget};

/// 画像置換とOCR候補のある最小文書を独立に用意する。
fn document() -> ParsedDocument {
    crate::parser::parse_pdf_bytes_pure("ocr-budget.pdf".into(), b"%PDF-1.7\n1 0 obj << /Type /Page /MediaBox [0 0 100 100] /Resources << /XObject << /Im 3 0 R >> >> /Contents 2 0 R >> endobj\n2 0 obj << /Length 6 >>\nstream\n/Im Do\nendstream\nendobj\n3 0 obj << /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream\nendobj\n%%EOF").unwrap()
}

/// metadataだけを制限する予算を作る。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    })
}

/// 文字の存在するページを文書へ追加する。
fn add_text(parsed: &mut ParsedDocument) {
    parsed.text_elements.push(crate::ParsedTextElement {
        page: 0,
        bounds: crate::ParsedBounds {
            x0: 0.,
            y0: 0.,
            x1: 1.,
            y1: 1.,
        },
        font_size: 1.,
        font_name: None,
        fill_color: None,
        text: "Recovered".into(),
    });
}

/// 無効なページ・非有限座標・不可視文字には文字ページ集合の容量を使わない。
#[test]
fn invisible_text_needs_no_slots_and_keeps_ocr_requirement() {
    for (page, x0, x1) in [
        (99, 0., 1.),
        (0, f32::NAN, 1.),
        (0, 0., f32::INFINITY),
        (0, 0., 0.),
        (0, 100., 110.),
    ] {
        let mut parsed = document();
        add_text(&mut parsed);
        parsed.text_elements[0].page = page;
        parsed.text_elements[0].bounds.x0 = x0;
        parsed.text_elements[0].bounds.x1 = x1;
        let original = parsed.quality.clone();
        let image_capacity = (4 * std::mem::size_of::<Option<usize>>()) as u64;
        refresh_ocr_diagnostics(&budget(image_capacity), &mut parsed).unwrap();
        assert_eq!(parsed.quality, original, "page={page}, bounds={x0}..{x1}");
    }
}

/// 画像ページ集合は保存容量なしでは診断を変更しない。
#[test]
fn image_pages_require_capacity_before_refresh() {
    let mut parsed = document();
    add_text(&mut parsed);
    let original = parsed.quality.clone();
    assert!(matches!(
        refresh_ocr_diagnostics(&budget(0), &mut parsed),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
    assert_eq!(parsed.quality, original);
}

/// 文字レイヤの有無によらず可視画像集合一つの容量で再判定する。
#[test]
fn visible_text_needs_no_additional_capacity() {
    let mut parsed = document();
    add_text(&mut parsed);
    let original = parsed.quality.clone();
    let target = budget((4 * std::mem::size_of::<Option<usize>>()) as u64);
    refresh_ocr_diagnostics(&target, &mut parsed).unwrap();
    assert_eq!(parsed.quality, original);
}

/// エラーを消費した停止状態では、空文書でも再判定を始めない。
#[test]
fn consumed_error_does_not_restart_refresh() {
    let target = budget(0);
    assert!(target.metadata_vector::<u8>(1).is_none());
    assert!(target.check().is_err());
    let mut parsed = document();
    parsed.text_elements.clear();
    parsed.image_elements.clear();
    let original = parsed.quality.clone();
    assert!(refresh_ocr_diagnostics(&target, &mut parsed).is_err());
    assert_eq!(parsed.quality, original);
}

/// 同一ページの文字と画像の重複は追加slotを必要とせず、再作成は累積する。
#[test]
fn duplicate_candidates_and_repeated_refresh_share_budget() {
    let mut parsed = document();
    add_text(&mut parsed);
    for _ in 0..8 {
        parsed.text_elements.push(parsed.text_elements[0].clone());
        parsed.image_elements.push(parsed.image_elements[0].clone());
    }
    let cost = (4 * std::mem::size_of::<Option<usize>>()) as u64;
    let target = budget(2 * cost);
    for _ in 0..2 {
        refresh_ocr_diagnostics(&target, &mut parsed).unwrap();
        assert_eq!(parsed.quality.issues.len(), 2);
    }
    let original = parsed.quality.clone();
    assert!(matches!(
        refresh_ocr_diagnostics(&target, &mut parsed),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
    assert_eq!(parsed.quality, original);
}

/// 無効な候補にはslotを作らず、診断を順序通りに保持する。
#[test]
fn invisible_and_blank_candidates_need_no_slots() {
    let mut parsed = document();
    add_text(&mut parsed);
    parsed.text_elements[0].text = " \t\r\n\u{0}".into();
    let original = parsed.image_elements[0].clone();
    parsed.image_elements.clear();
    for (page, bounds) in [
        (
            0,
            crate::ParsedBounds {
                x0: 0.,
                x1: 0.,
                y0: 0.,
                y1: 10.,
            },
        ),
        (
            0,
            crate::ParsedBounds {
                x0: 100.,
                x1: 110.,
                y0: 0.,
                y1: 10.,
            },
        ),
        (
            0,
            crate::ParsedBounds {
                x0: f32::NAN,
                x1: 10.,
                y0: 0.,
                y1: 10.,
            },
        ),
        (99, original.bounds),
    ] {
        let mut image = original.clone();
        image.page = page;
        image.bounds = bounds;
        parsed.image_elements.push(image);
    }
    parsed
        .quality
        .issues
        .push(QualityIssue::UnsupportedInlineImage { page: 0 });
    let target = budget(0);
    refresh_ocr_diagnostics(&target, &mut parsed).unwrap();
    assert_eq!(
        parsed.quality.issues,
        [
            QualityIssue::ImageReplaced {
                object_number: Some(3),
                reason: crate::ImageReplacementReason::UnsupportedFilter
            },
            QualityIssue::UnsupportedInlineImage { page: 0 }
        ]
    );
    target.check().unwrap();
}

/// 半分占有の集合が各拡張で要求する全slotの累積容量を独立に数える。
fn set_cost(count: usize) -> u64 {
    let mut slots = 0;
    let mut total = 0;
    for length in 0..count {
        if length >= slots / 2 {
            slots = (slots * 2).max(4);
            total += slots;
        }
    }
    (total * std::mem::size_of::<Option<usize>>()) as u64
}

use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    /// 任意の重複・別ページ・不可視候補で、容量境界と診断保持を集合モデルと照合する。
    #[test]
    fn candidate_sets_match_budget_and_quality_model(candidates in prop::collection::vec((0usize..10, 0u8..16), 0..40)) {
        let mut parsed = document();
        let image = parsed.image_elements[0].clone();
        add_text(&mut parsed);
        let text = parsed.text_elements[0].clone();
        let page = parsed.pages[0].clone();
        parsed.pages = (0..8).map(|page_number| { let mut page = page.clone(); page.page_number = page_number; page }).collect();
        parsed.text_elements.clear();
        parsed.image_elements.clear();
        let mut image_pages = std::collections::BTreeSet::new();
        for (page, flags) in candidates {
            let mut element = text.clone(); element.page = page;
            if flags & 2 == 0 { element.bounds.x1 = element.bounds.x0; }
            if flags & 1 == 0 { element.text = " \n\u{0}".into(); }
            if flags & 8 != 0 { element.bounds.y0 = 2.0; element.bounds.y1 = 3.0; }
            parsed.text_elements.push(element);
            let mut element = image.clone(); element.page = page;
            if flags & 4 == 0 { element.bounds.x1 = element.bounds.x0; } else if page < 8 { image_pages.insert(page); }
            parsed.image_elements.push(element);
        }
        parsed.quality.issues.truncate(1);
        parsed.quality.issues.push(QualityIssue::UnsupportedInlineImage { page: 3 });
        parsed.quality.issues.extend((0..10).rev().map(|page| QualityIssue::OcrRequired { page }));
        let original = parsed.quality.clone();
        let cost = set_cost(image_pages.len());
        if cost > 0 {
            prop_assert!(matches!(refresh_ocr_diagnostics(&budget(cost - 1), &mut parsed), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", .. })), "不足した保存容量を公開エラーにする");
            prop_assert_eq!(&parsed.quality, &original);
        }
        let target = budget(cost);
        refresh_ocr_diagnostics(&target, &mut parsed).unwrap();
        target.check().unwrap();
        let expected = original.issues.into_iter().filter(|issue| match issue { QualityIssue::OcrRequired { page } => image_pages.contains(page), _ => true }).collect::<Vec<_>>();
        prop_assert_eq!(&parsed.quality.issues, &expected);
        let completed = parsed.quality.clone();
        let repeated = refresh_ocr_diagnostics(&target, &mut parsed);
        if cost > 0 { prop_assert!(repeated.is_err()); } else { prop_assert!(repeated.is_ok()); }
        prop_assert_eq!(parsed.quality, completed);
    }
}
