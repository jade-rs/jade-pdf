//! nativeの文字index予約と通常segment保存の容量・意味保持を確認する。
use super::*;
use crate::{
    ParseLimits,
    page_attribute_fixture::{pdf, stream},
};

/// 指定容量だけを持つ独立したmetadata予算を作る。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    })
}

/// 正式なxref/Rootを持つPDFをnativeへ読み込み、借用ページ本文を検査する。
fn with_page(content: &str, check: impl FnOnce(&PdfPageText<'_>)) {
    with_native_page(content, |page| check(&page.text().unwrap()));
}

/// ページobjectと文字indexを同じnative読み込みから検査する。
fn with_native_page(content: &str, check: impl FnOnce(&PdfPage<'_>)) {
    let bytes = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F 5 0 R >> >> >>".into(),
        stream(content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ]);
    // assertionのunwind中にnativeをdropすると、依存先の共有Mutexをpoisonする。
    let outcome = {
        let native = pdfium_auto::bind_pdfium_silent().unwrap();
        let document = native.load_pdf_from_byte_slice(&bytes, None).unwrap();
        let page = document.pages().get(0).unwrap();
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&page)))
    };
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
}

/// 部分clipのindex取得も予算不足・消費済み停止から再開しない。
#[test]
fn partial_clip_preserves_metadata_stop() {
    with_native_page(
        "q 20 70 100 30 re W n BT /F 12 Tf 3.992 80 Td (XXalpha) Tj ET Q",
        |page| {
            let text = page.text().unwrap();
            let run =
                |budget: &DecodeBudget| crate::pdfium_clip::excluded(budget, page.objects(), &text);
            let expected = run(&budget(65536)).unwrap();
            assert!(expected.iter().any(|excluded| *excluded));
            assert!(expected.iter().any(|excluded| !*excluded));
            let mut low = 0;
            let mut high = 65536;
            while low < high {
                let middle = low + (high - low) / 2;
                if run(&budget(middle)).is_ok() {
                    high = middle;
                } else {
                    low = middle + 1;
                }
            }
            assert_eq!(run(&budget(low)).unwrap(), expected);
            assert!(matches!(
                run(&budget(low - 1)),
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    ..
                })
            ));
            let stopped = budget(0);
            stopped.charge_metadata_workspace_bytes(1);
            assert!(stopped.check().is_err());
            assert!(run(&stopped).is_err());
        },
    );
}

/// 重なり検査の配列もmetadata容量へ算入し、消費済み停止から再開しない。
#[test]
fn clip_overlap_preserves_metadata_stop() {
    with_native_page(
        "q 20 70 100 30 re W n BT /F 12 Tf 3.992 80 Td (XXalpha) Tj ET Q BT /F 12 Tf 10 150 Td (Next) Tj ET",
        |page| {
            let tags = crate::pdfium_font_tags::Tagged::default();
            let run = |budget: &DecodeBudget| {
                crate::pdfium_clip_overlap::check(
                    budget,
                    std::path::Path::new("clip.pdf"),
                    page.objects(),
                    &tags,
                )
            };
            let mut low = 0;
            let mut high = 65536;
            run(&budget(high)).unwrap();
            while low < high {
                let middle = low + (high - low) / 2;
                if run(&budget(middle)).is_ok() {
                    high = middle;
                } else {
                    low = middle + 1;
                }
            }
            assert!(low > 0);
            run(&budget(low)).unwrap();
            assert!(matches!(
                run(&budget(low - 1)),
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    ..
                })
            ));
            let stopped = budget(0);
            stopped.charge_metadata_workspace_bytes(1);
            assert!(stopped.check().is_err());
            assert!(run(&stopped).is_err());
        },
    );
}

/// clipのないページでは重なり検査用の配列を確保しない。
#[test]
fn unclipped_overlap_check_needs_no_metadata() {
    with_native_page("BT /F 12 Tf 10 80 Td (alpha) Tj ET", |page| {
        crate::pdfium_clip_overlap::check(
            &budget(0),
            std::path::Path::new("plain.pdf"),
            page.objects(),
            &crate::pdfium_font_tags::Tagged::default(),
        )
        .unwrap();
    });
}

/// 全ページindexはi32の要求容量を確保前に予約し、1 byte不足を公開エラーにする。
#[test]
fn page_characters_reserve_exact_index_bytes() {
    with_page("BT /F 12 Tf 10 80 Td (alpha) Tj ET", |text| {
        let count = text.len() as u64;
        assert!(count > 0);
        let required = count * std::mem::size_of::<i32>() as u64;
        assert_eq!(
            characters(&budget(required), text).unwrap().len() as u64,
            count
        );
        assert!(matches!(
            characters(&budget(required - 1), text),
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                ..
            })
        ));
    });
}

/// 消費済み停止後は空ページのindex取得も再開しない。
#[test]
fn page_characters_preserve_consumed_stop() {
    with_page("", |text| {
        assert!(characters(&budget(0), text).unwrap().is_empty());
        let stopped = budget(0);
        stopped.charge_metadata_workspace_bytes(1);
        assert!(stopped.check().is_err());
        assert!(characters(&stopped, text).is_err());
    });
}

/// 旧segment APIで得られる本文・先頭文字属性を比較の基準にする。
fn legacy(text: &PdfPageText<'_>) -> Vec<ParsedTextElement> {
    text.segments()
        .iter()
        .filter_map(|segment| {
            let raw = segment.text();
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            let bounds = parsed_bounds_from_pdf_rect(segment.bounds());
            let mut result = ParsedTextElement {
                page: 7,
                bounds,
                font_size: bounds.y1 - bounds.y0,
                font_name: None,
                fill_color: None,
                text: trimmed.into(),
            };
            if let Ok(chars) = segment.chars()
                && let Some(first) = chars.iter().next()
            {
                result.font_size = first.scaled_font_size().value;
                let name = first.font_name();
                result.font_name = (!name.is_empty()).then_some(name);
                result.fill_color = first.fill_color().ok().map(|color| color.to_hex());
            }
            Some(result)
        })
        .collect()
}

/// 本番と同じindex予約・共有経路で通常文字抽出を実行する。
fn run(
    budget: &DecodeBudget,
    page: usize,
    text: &PdfPageText<'_>,
) -> Result<Vec<ParsedTextElement>, ParseError> {
    let chars = characters(budget, text)?;
    super::extract(budget, page, text, &chars)
}

/// 公開結果から保存に必要な容量の境界を探す。
fn minimum_budget(text: &PdfPageText<'_>) -> u64 {
    let mut lower = 0;
    let mut upper = 65536;
    assert!(run(&budget(upper), 7, text).is_ok());
    while lower < upper {
        let middle = lower + (upper - lower) / 2;
        if run(&budget(middle), 7, text).is_ok() {
            upper = middle;
        } else {
            lower = middle + 1;
        }
    }
    assert!(
        lower > 0,
        "ordinary native extraction must reserve result storage"
    );
    lower
}

/// 後続segmentで容量不足になっても完成した前半だけを返さない。
#[test]
fn ordinary_segments_reserve_storage_and_discard_partial_output() {
    with_page(
        "BT /F 12 Tf 10 80 Td ( alpha ) Tj /F 18 Tf 0 -40 Td ( beta ) Tj ET",
        |text| {
            let expected = legacy(text);
            assert!(expected.len() >= 2);
            assert!(matches!(
                run(&budget(0), 7, text),
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    ..
                })
            ));
            let required = minimum_budget(text);
            assert!(required > text.len() as u64 * std::mem::size_of::<i32>() as u64);
            let shared = characters(&budget(65536), text).unwrap();
            assert!(matches!(
                super::extract(&budget(0), 7, text, &shared),
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    ..
                })
            ));
            assert_eq!(run(&budget(required), 7, text).unwrap(), expected);
            assert!(matches!(
                run(&budget(required - 1), 7, text),
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    ..
                })
            ));
        },
    );
}

/// 空白しかないsegmentは保存せず、停止済み予算では空結果も返さない。
#[test]
fn ordinary_segments_preserve_empty_and_stopped_behavior() {
    with_page("BT /F 12 Tf 10 80 Td (   ) Tj ET", |text| {
        assert!(run(&budget(65536), 7, text).unwrap().is_empty());
        let stopped = budget(0);
        stopped.charge_metadata_workspace_bytes(1);
        assert!(stopped.check().is_err());
        assert!(run(&stopped, 7, text).is_err());
    });
}

/// 共有indexを使う不可視抽出も、消費済み停止後の空ページを成功にしない。
#[test]
fn visible_extraction_preserves_consumed_stop_on_empty_page() {
    with_page("", |text| {
        let chars = characters(&budget(0), text).unwrap();
        let tags = crate::pdfium_font_tags::Tagged::default();
        assert!(!crate::pdfium_visible_text::has_hidden(&chars, &tags));
        let path = std::path::Path::new("empty-native.pdf");
        assert!(
            crate::pdfium_visible_text::extract(&budget(0), path, 7, &chars, &tags, &[]).is_ok()
        );
        let stopped = budget(0);
        stopped.charge_metadata_workspace_bytes(1);
        assert!(stopped.check().is_err());
        assert!(
            crate::pdfium_visible_text::extract(&stopped, path, 7, &chars, &tags, &[]).is_err()
        );
    });
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(16))]
    /// 座標・文字サイズ・重なり・色が異なっても旧segmentの幾何/styleを保持する。
    #[test]
    fn ordinary_segments_match_native_segment_semantics(
        x in 0u16..100, y in 20u16..150, size in 5u16..30, overlap in proptest::bool::ANY,
    ) {
        let next_y = if overlap { y } else { y - 15 };
        let content = format!("BT /F {size} Tf 1 0 0 1 {x} {y} Tm (alpha) Tj 1 0 0 rg /F 18 Tf 1 0 0 1 {x} {next_y} Tm (beta) Tj ET");
        with_page(&content, |text| {
            assert_eq!(run(&budget(65536), 7, text).unwrap(), legacy(text));
        });
    }
}
