use std::path::Path;

#[path = "pdfium_library.rs"]
mod library;

#[path = "pdfium_preview.rs"]
mod preview;
use preview::{append as append_text_preview, normalize as normalize_pdfium_text_preview};

#[path = "pdfium_pages.rs"]
mod pages;

#[cfg(test)]
#[path = "pdfium_need_tests.rs"]
mod need_tests;

#[cfg(test)]
#[path = "pdfium_preview_budget_tests.rs"]
mod preview_budget_tests;

#[cfg(test)]
#[path = "pdfium_reconcile_budget_tests.rs"]
mod reconcile_budget_tests;

#[path = "pdfium_reconcile.rs"]
mod reconciliation;
use reconciliation::reconcile as reconcile_pdfium_characters;

use crate::{
    decode::DecodeBudget,
    limits::{ParseLimits, check_limit},
    model::{ParseError, ParsedDocument, ParsedPage, ParsedTextElement},
    parser::parse_pdf_bytes_pure_with_budget,
};

/// Pdfium を必須バックエンドとして PDF ファイルを解析する。
///
/// Pdfium が使えない場合は pure Rust 実装へフォールバックせず、呼び出し元へ
/// `ParseError::Pdfium` を返す。既存 pure Rust parser は graphics/image など
/// Pdfium IR へ未移植の構造情報を合成する目的でだけ利用する。
pub(crate) fn parse_pdf(
    path: &Path,
    bytes: &[u8],
    limits: &ParseLimits,
    decode_budget: &DecodeBudget,
) -> Result<ParsedDocument, ParseError> {
    let mut parsed =
        parse_pdf_bytes_pure_with_budget(path.to_path_buf(), bytes, limits, decode_budget)?;
    let needs_text = needs_pdfium_text_extraction(decode_budget, bytes, &parsed);
    decode_budget.check()?;
    if !needs_text {
        return Ok(parsed);
    }
    let tags = if !decode_budget.is_pdf20()
        && parsed.quality.issues.iter().any(|issue| {
            matches!(
                issue,
                crate::QualityIssue::UnsupportedFont {
                    reason: crate::UnsupportedFontReason::Type3,
                    ..
                }
            )
        }) {
        let result = crate::pdfium_font_tags::Tagged::prepare(decode_budget, bytes);
        decode_budget.check()?;
        result.ok_or_else(|| pdfium_error(path, "Type3 font tagging failed"))?
    } else {
        crate::pdfium_font_tags::Tagged::default()
    };
    let native_bytes = if tags.bytes.is_empty() {
        bytes
    } else {
        &tags.bytes
    };
    decode_budget.check()?;
    let pdfium = library::bind(path)?;
    decode_budget.check()?;
    let document = pdfium
        .load_pdf_from_byte_slice(native_bytes, None)
        .map_err(|err| pdfium_error(path, err))?;

    decode_budget.check()?;

    check_limit(
        "page count",
        document.pages().len() as u64,
        limits.max_pages as u64,
    )?;
    let mut pdfium_pages = pages::reserve(decode_budget, document.pages().len() as usize)?;
    let mut text_elements = Vec::new();
    let mut text_preview = String::new();
    for (page_number, page) in document.pages().iter().enumerate() {
        decode_budget.check()?;
        let width = page.width().value;
        let height = page.height().value;
        let rotation = page
            .rotation()
            .map_err(|err| pdfium_error(path, err))?
            .as_degrees() as i32;
        pdfium_pages.push(ParsedPage {
            page_number,
            width,
            height,
            rotation,
        });

        crate::pdfium_clip_overlap::check(decode_budget, path, page.objects(), &tags)?;
        crate::pdfium_visible_text::suppress_hidden(decode_budget, path, page.objects(), &tags, 0)?;
        let page_text = page.text().map_err(|err| pdfium_error(path, err))?;
        let characters = crate::pdfium_text::characters(decode_budget, &page_text)?;
        let excluded = crate::pdfium_clip::excluded(decode_budget, page.objects(), &page_text)?;
        if crate::pdfium_visible_text::has_hidden(&characters, &tags)
            || !tags.bytes.is_empty()
            || excluded.iter().any(|excluded| *excluded)
        {
            let (preview, elements) = crate::pdfium_visible_text::extract(
                decode_budget,
                path,
                page_number,
                &characters,
                &tags,
                &excluded,
            )?;
            append_text_preview(decode_budget, &mut text_preview, &preview);
            decode_budget.check()?;
            crate::text_result_storage::append(decode_budget, &mut text_elements, elements);
            decode_budget.check()?;
        } else {
            append_text_preview(decode_budget, &mut text_preview, &page_text.all());
            decode_budget.check()?;
            let elements =
                crate::pdfium_text::extract(decode_budget, page_number, &page_text, &characters)?;
            crate::text_result_storage::append(decode_budget, &mut text_elements, elements);
            decode_budget.check()?;
        }
        decode_budget.check()?;
    }

    parsed.page_count = document.pages().len() as usize;
    parsed.pages = pages::merge(decode_budget, &parsed.pages, pdfium_pages)?;
    let normalized = normalize_pdfium_text_preview(decode_budget, &text_preview);
    decode_budget.check()?;
    let normalized = normalized.ok_or_else(|| pdfium_error(path, "preview allocation failed"))?;
    apply_pdfium_text(decode_budget, &mut parsed, normalized, text_elements)?;
    decode_budget.check()?;
    Ok(parsed)
}

/// Pdfiumの文字抽出結果をpure Rust parserのIRへ反映する。
///
/// pure Rust側が制御文字を含まない文字列を返していても、埋め込みfontのglyph codeを
/// 別のUnicode文字へ誤対応させる場合がある。native pathの文字内容はPdfiumを正とし、
/// Pdfiumが空でない結果を返したときは文字だけを常に置き換える。
fn apply_pdfium_text(
    decode_budget: &DecodeBudget,
    parsed: &mut ParsedDocument,
    text_preview: String,
    text_elements: Vec<ParsedTextElement>,
) -> Result<(), ParseError> {
    crate::metadata_workspace::ensure_active(decode_budget)?;
    let replacement = if text_elements.is_empty() {
        None
    } else if text_elements_have_invalid_geometry(parsed) {
        Some(text_elements)
    } else {
        let reconciled = reconcile_pdfium_characters(
            decode_budget,
            &parsed.text_elements,
            &text_elements,
            &parsed.quality,
        );
        decode_budget.check()?;
        crate::metadata_workspace::ensure_active(decode_budget)?;
        Some(reconciled.unwrap_or(text_elements))
    };
    if !text_preview.is_empty() {
        parsed.text_preview = text_preview;
    }
    if let Some(elements) = replacement {
        parsed.text_elements = elements;
    }
    crate::quality::refresh_ocr_diagnostics(decode_budget, parsed)
}

/// Pdfium の text extraction が必要な PDF かを安価に判定する。
fn needs_pdfium_text_extraction(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
    parsed: &ParsedDocument,
) -> bool {
    if !parsed.text_elements.is_empty() || !parsed.text_preview.trim().is_empty() {
        return true;
    }
    bytes
        .windows(b"/Font".len())
        .any(|window| window == b"/Font")
        && crate::parser::pdf_bytes_have_text_showing_operators(decode_budget, bytes)
}

/// pure Rust 抽出結果の text geometry がページ範囲から破綻しているかを判定する。
fn text_elements_have_invalid_geometry(parsed: &ParsedDocument) -> bool {
    parsed.text_elements.iter().any(|element| {
        let Some(page) = parsed
            .pages
            .iter()
            .find(|page| page.page_number == element.page)
        else {
            return true;
        };
        !text_bounds_are_near_page(element, page)
    })
}

/// text bounds が対応ページの近傍に収まっているかを判定する。
fn text_bounds_are_near_page(element: &ParsedTextElement, page: &ParsedPage) -> bool {
    let bounds = element.bounds;
    bounds.x0.is_finite()
        && bounds.x1.is_finite()
        && bounds.y0.is_finite()
        && bounds.y1.is_finite()
        && bounds.x0 >= -page.width
        && bounds.x1 <= page.width * 2.0
        && bounds.y0 >= -page.height
        && bounds.y1 <= page.height * 2.0
}

/// Pdfium のエラーを parser 共通エラーへ変換する。
fn pdfium_error(path: &Path, err: impl std::fmt::Display) -> ParseError {
    ParseError::Pdfium {
        path: path.to_path_buf(),
        message: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_pdf_bytes_pure;
    use crate::{ParsedBounds, PdfVersion};

    /// native文字の回復だけでは画像本文の完全性を保証せず、既存診断を保持する。
    #[test]
    fn native_text_recovery_keeps_image_verification_requirement() {
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Page /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 2 0 R >> >> /Contents 3 0 R >> endobj\n2 0 obj << /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream\nendobj\n3 0 obj << /Length 31 >>\nstream\nq 200 0 0 200 0 0 cm /Im1 Do Q\nendstream\nendobj\n";
        let mut parsed = parse_pdf_bytes_pure("scan.pdf".into(), pdf).unwrap();
        assert_eq!(parsed.quality.issues.len(), 2);
        assert!(
            parsed
                .quality
                .issues
                .contains(&crate::QualityIssue::OcrRequired { page: 0 })
        );
        apply_pdfium_text(
            &DecodeBudget::new(&ParseLimits::default()),
            &mut parsed,
            "Recovered text".into(),
            vec![text_element("Recovered text")],
        )
        .unwrap();
        assert_eq!(
            parsed.quality.issues,
            vec![
                crate::QualityIssue::ImageReplaced {
                    object_number: Some(2),
                    reason: crate::ImageReplacementReason::UnsupportedFilter,
                },
                crate::QualityIssue::OcrRequired { page: 0 }
            ]
        );
    }

    /// native文字の回復後も同じ予算の不足を返し、既存の診断を消さない。
    #[test]
    fn native_ocr_refresh_propagates_shared_budget_failure() {
        let pdf = b"%PDF-1.7\n1 0 obj << /Type /Page /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 2 0 R >> >> /Contents 3 0 R >> endobj\n2 0 obj << /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream\nendobj\n3 0 obj << /Length 31 >>\nstream\nq 200 0 0 200 0 0 cm /Im1 Do Q\nendstream\nendobj\n";
        for capacity in [0, (4 * std::mem::size_of::<Option<usize>>()) as u64 - 1] {
            let mut parsed = parse_pdf_bytes_pure("native-budget.pdf".into(), pdf).unwrap();
            let original = parsed.quality.clone();
            let target = DecodeBudget::new(&ParseLimits {
                max_total_metadata_workspace_bytes: capacity,
                ..Default::default()
            });
            assert!(matches!(
                apply_pdfium_text(
                    &target,
                    &mut parsed,
                    "Recovered text".into(),
                    vec![text_element("Recovered text")]
                ),
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    ..
                })
            ));
            assert_eq!(parsed.quality, original);
            assert!(target.is_stopped());
        }
    }

    #[test]
    fn pdfium_text_replaces_readable_but_wrong_pure_text() {
        let mut parsed = ParsedDocument {
            quality: Default::default(),
            path: "libreoffice.pdf".into(),
            byte_len: 0,
            header: "%PDF-1.7".to_string(),
            version: PdfVersion { major: 1, minor: 7 },
            page_count: 1,
            pages: vec![ParsedPage {
                page_number: 0,
                width: 200.0,
                height: 200.0,
                rotation: 0,
            }],
            graphics: Vec::new(),
            text_elements: vec![text_element("表と容")],
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: "表と容".to_string(),
        };

        apply_pdfium_text(
            &DecodeBudget::new(&ParseLimits::default()),
            &mut parsed,
            "（と乙".to_string(),
            vec![text_element("（"), text_element("と"), text_element("乙")],
        )
        .unwrap();

        assert_eq!(parsed.text_preview, "（と乙");
        assert_eq!(parsed.text_elements.len(), 1);
        assert_eq!(parsed.text_elements[0].text, "（と乙");
        assert_eq!(
            parsed.text_elements[0].bounds,
            ParsedBounds {
                x0: 10.0,
                y0: 10.0,
                x1: 50.0,
                y1: 20.0,
            }
        );
    }

    /// Formとページ本文の抽出順が違うときはnativeの文字と属性を一緒に採用する。
    #[test]
    fn reordered_form_characters_keep_native_geometry_and_style() {
        let mut parsed = parse_pdf_bytes_pure(
            "form-order.pdf".into(),
            b"%PDF-1.4\n1 0 obj << /Type /Page /MediaBox [0 0 200 200] >> endobj\n",
        )
        .unwrap();
        parsed
            .quality
            .issues
            .push(crate::QualityIssue::UnsupportedXObject {
                page: 0,
                object_number: 5,
                reason: crate::UnsupportedXObjectReason::Form,
            });
        let mut parent = text_element("PARENT");
        parent.bounds.y0 = 100.;
        parent.bounds.y1 = 110.;
        let mut form = text_element("AA");
        form.font_name = Some("Courier".into());
        form.font_size = 24.;
        parsed.text_elements = vec![parent.clone(), form.clone()];
        let native = vec![form, parent];
        apply_pdfium_text(
            &DecodeBudget::default(),
            &mut parsed,
            "AA\nPARENT".into(),
            native.clone(),
        )
        .unwrap();
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .map(|t| (&t.text, t.bounds, t.font_size, &t.font_name))
                .collect::<Vec<_>>(),
            native
                .iter()
                .map(|t| (&t.text, t.bounds, t.font_size, &t.font_name))
                .collect::<Vec<_>>()
        );
    }

    /// 文字数が一致してもページ外へ破綻した座標は native 座標に置き換える。
    #[test]
    fn native_recovery_does_not_reuse_invalid_pure_geometry() {
        let mut parsed = parse_pdf_bytes_pure(
            "invalid-geometry.pdf".into(),
            b"%PDF-1.4\n1 0 obj << /Type /Page /MediaBox [0 0 200 200] >> endobj\n",
        )
        .unwrap();
        let mut invalid = text_element("A");
        invalid.bounds.x0 = 10000.;
        invalid.bounds.x1 = 10040.;
        parsed.text_elements = vec![invalid];
        let recovered = text_element("B");
        apply_pdfium_text(
            &DecodeBudget::new(&ParseLimits::default()),
            &mut parsed,
            "B".into(),
            vec![recovered.clone()],
        )
        .unwrap();
        assert_eq!(parsed.text_elements[0].bounds, recovered.bounds);
        assert_eq!(parsed.text_elements[0].text, "B");
    }

    #[test]
    fn text_extraction_is_skipped_when_pdf_has_no_font_resource() {
        let bytes = b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\n";

        let parsed = parse_pdf_bytes_pure("no-font.pdf".into(), bytes).unwrap();

        assert!(!needs_pdfium_text_extraction(
            &Default::default(),
            bytes,
            &parsed
        ));
    }

    /// 定義済みFontと正確なstream長を持つRootなしのnative判定fixtureを作る。
    fn pdf_with_font_content(content: &str) -> Vec<u8> {
        format!("%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj\n4 0 obj << /Length {} >> stream\n{content}\nendstream endobj\n5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n", content.len()).into_bytes()
    }

    /// Fontを使用する本文ではnative文字抽出を要求する。
    #[test]
    fn text_extraction_is_needed_when_pdf_has_font_resource() {
        let bytes = pdf_with_font_content("BT /F1 10 Tf 1 0 0 1 20 20 Tm (text) Tj ET");

        let parsed = parse_pdf_bytes_pure("text.pdf".into(), &bytes).unwrap();

        assert!(needs_pdfium_text_extraction(
            &Default::default(),
            &bytes,
            &parsed
        ));
    }

    /// 未使用Fontだけではnative文字抽出を要求しない。
    #[test]
    fn text_extraction_is_skipped_when_font_resource_is_unused() {
        let bytes = pdf_with_font_content("20 20 m 40 20 l S");

        let parsed = parse_pdf_bytes_pure("unused-font.pdf".into(), &bytes).unwrap();

        assert!(!needs_pdfium_text_extraction(
            &Default::default(),
            &bytes,
            &parsed
        ));
    }

    /// 空白描画でも文字演算子があればnative抽出を要求する。
    #[test]
    fn text_extraction_is_delegated_to_pdfium_when_text_operator_is_present() {
        let bytes = pdf_with_font_content("BT /F1 10 Tf 1 0 0 1 20 20 Tm ( ) Tj ET");
        let parsed = parse_pdf_bytes_pure("space-text.pdf".into(), &bytes).unwrap();

        assert!(needs_pdfium_text_extraction(
            &Default::default(),
            &bytes,
            &parsed
        ));
    }

    #[test]
    fn text_elements_have_invalid_geometry_detects_out_of_page_elements() {
        let parsed = ParsedDocument {
            quality: Default::default(),
            path: "broken.pdf".into(),
            byte_len: 0,
            header: "%PDF-1.7".to_string(),
            version: PdfVersion { major: 1, minor: 7 },
            page_count: 1,
            pages: vec![ParsedPage {
                page_number: 0,
                width: 100.0,
                height: 200.0,
                rotation: 0,
            }],
            graphics: Vec::new(),
            text_elements: vec![ParsedTextElement {
                page: 3,
                bounds: ParsedBounds {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 10.0,
                    y1: 10.0,
                },
                font_size: 10.0,
                font_name: None,
                fill_color: None,
                text: "off-page".to_string(),
            }],
            image_elements: Vec::new(),
            embedded_fonts: Vec::new(),
            text_preview: String::new(),
        };

        assert!(text_elements_have_invalid_geometry(&parsed));
    }

    #[test]
    fn missing_pure_page_uses_unrotated_pdfium_dimensions() {
        let pages = pages::merge(
            &DecodeBudget::new(&ParseLimits::default()),
            &[],
            vec![ParsedPage {
                page_number: 0,
                width: 150.0,
                height: 200.0,
                rotation: 90,
            }],
        )
        .unwrap();

        assert_eq!(
            pages,
            vec![ParsedPage {
                page_number: 0,
                width: 200.0,
                height: 150.0,
                rotation: 90,
            }]
        );
    }

    /// テスト用の座標付き文字要素を作る。
    fn text_element(text: &str) -> ParsedTextElement {
        ParsedTextElement {
            page: 0,
            bounds: ParsedBounds {
                x0: 10.0,
                y0: 10.0,
                x1: 50.0,
                y1: 20.0,
            },
            font_size: 10.0,
            font_name: None,
            fill_color: None,
            text: text.to_string(),
        }
    }
}
