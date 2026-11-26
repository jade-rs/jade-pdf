//! nativeの文字index予約と通常segment保存の容量・意味保持を確認する。
use super::*;
use crate::{
    ParseLimits,
    page_attribute_fixture::{pdf, stream},
};

#[path = "pdfium_clip_index_tests.rs"]
pub(crate) mod clip_index_tests;

/// 指定容量だけを持つ独立したmetadata予算を作る。
pub(super) fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    })
}

/// 正式なxref/Rootを持つPDFをnativeへ読み込み、借用ページ本文を検査する。
pub(super) fn with_page(content: &str, check: impl FnOnce(&PdfPageText<'_>)) {
    with_native_page(content, |page| check(&page.text().unwrap()));
}

/// ページobjectと文字indexを同じnative読み込みから検査する。
fn with_native_page(content: &str, check: impl FnOnce(&PdfPage<'_>)) {
    let bytes = owned_text_pdf(content);
    with_native_bytes(&bytes, check);
}

/// 所有の正式なPDFを同じSDK lifecycleで検査し、assertionのpanicをdrop後に伝える。
pub(super) fn with_native_bytes(bytes: &[u8], check: impl FnOnce(&PdfPage<'_>)) {
    // assertionのunwind中にnativeをdropすると、依存先の共有Mutexをpoisonする。
    let outcome = {
        let native = pdfium_auto::bind_pdfium_silent().unwrap();
        let document = native.load_pdf_from_byte_slice(bytes, None).unwrap();
        let page = document.pages().get(0).unwrap();
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&page)))
    };
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
}

/// 印刷hyphenの復元は新たなmetadataを要求せず、preview・通常・可視抽出で一致する。
#[test]
fn printed_hyphens_preserve_storage_and_all_native_text_paths() {
    with_page(
        "BT /F 12 Tf 20 160 Td (well-) Tj 0 -20 Td (known) Tj ET",
        |text| {
            let chars = characters(&budget(65536), text).unwrap();
            assert!(
                chars
                    .iter()
                    .any(|ch| ch.unicode_value() == 2 && ch.is_hyphen().unwrap())
            );
            let path = std::path::Path::new("owned-printed-hyphen.pdf");
            let mut preview = text.all();
            assert!(preview.contains('\u{2}'));
            let pointer = preview.as_ptr();
            let capacity = preview.capacity();
            crate::pdfium_printed_hyphens::page(&budget(0), path, &mut preview, &chars).unwrap();
            assert!(preview.contains("well-"));
            assert!(!preview.contains('\u{2}'));
            assert_eq!((preview.as_ptr(), preview.capacity()), (pointer, capacity));
            let mut unmatched = text.all();
            unmatched.push('\u{2}');
            let before = unmatched.clone();
            crate::pdfium_printed_hyphens::page(&budget(0), path, &mut unmatched, &chars).unwrap();
            assert_eq!(unmatched, before);
            let normal = run(&budget(65536), 0, text).unwrap();
            assert!(normal.iter().any(|element| element.text.contains("well-")));
            let tags = crate::pdfium_font_tags::Tagged::default();
            let (visible_preview, visible) =
                crate::pdfium_visible_text::extract(&budget(65536), path, 0, &chars, &tags, &[])
                    .unwrap();
            assert!(visible_preview.contains("well-"));
            assert!(visible.iter().any(|element| element.text == "-"));
            let stopped = budget(0);
            stopped.charge_metadata_workspace_bytes(1);
            let mut unchanged = text.all();
            let before = unchanged.clone();
            assert!(
                crate::pdfium_printed_hyphens::page(&stopped, path, &mut unchanged, &chars)
                    .is_err()
            );
            assert_eq!(unchanged, before);
        },
    );
}

/// 同じU+0002でもSDKが印刷hyphenと判定していないToUnicode文字は書き換えない。
#[test]
fn literal_control_code_is_not_guessed_as_a_hyphen() {
    let source = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F 5 0 R >> >> >>".into(),
        stream("BT /F 12 Tf 20 160 Td (A) Tj ET"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /ToUnicode 6 0 R >>".into(),
        stream("1 beginbfchar\n<41> <0002>\nendbfchar"),
    ]);
    with_native_bytes(&source, |page| {
        let text = page.text().unwrap();
        let chars = characters(&budget(65536), &text).unwrap();
        assert!(
            chars
                .iter()
                .any(|ch| ch.unicode_value() == 2 && !ch.is_hyphen().unwrap())
        );
        let mut raw = text.all();
        assert!(raw.contains('\u{2}'));
        let before = raw.clone();
        crate::pdfium_printed_hyphens::page(
            &budget(0),
            std::path::Path::new("owned-control.pdf"),
            &mut raw,
            &chars,
        )
        .unwrap();
        assert_eq!(raw, before);
    });
}

/// 同じページ内に二種類のU+0002があっても、印刷hyphenの位置だけを復元する。
#[test]
fn mixed_printed_hyphen_and_literal_control_keep_distinct_identity() {
    let source = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F 5 0 R >> >> >>".into(),
        stream("BT /F 12 Tf 20 160 Td (A well-) Tj 0 -20 Td (known) Tj ET"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding /ToUnicode 6 0 R >>".into(),
        stream("1 beginbfchar\n<41> <0002>\nendbfchar"),
    ]);
    with_native_bytes(&source, |page| {
        let text = page.text().unwrap();
        let chars = characters(&budget(65536), &text).unwrap();
        let mut raw = text.all();
        assert_eq!(raw.chars().filter(|ch| *ch == '\u{2}').count(), 2);
        assert!(raw.starts_with('\u{2}'));
        let storage = (raw.as_ptr(), raw.capacity());
        crate::pdfium_printed_hyphens::page(
            &budget(0),
            std::path::Path::new("owned-mixed-hyphen.pdf"),
            &mut raw,
            &chars,
        )
        .unwrap();
        assert_eq!(raw.chars().filter(|ch| *ch == '\u{2}').count(), 1);
        assert!(raw.starts_with('\u{2}'));
        assert!(raw.contains("well-"));
        assert_eq!((raw.as_ptr(), raw.capacity()), storage);
    });
}

/// sourceの空白とSDKの報告を区別するため、同じowned入力を両経路へ渡す。
pub(super) fn owned_text_pdf(content: &str) -> Vec<u8> {
    pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F 5 0 R >> >> >>".into(),
        stream(content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ])
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

/// SDK直接取得の本文・先頭文字属性を基準にし、端の改行だけを除く。
fn native_segment_reference(text: &PdfPageText<'_>) -> Vec<ParsedTextElement> {
    text.segments()
        .iter()
        .filter_map(|segment| {
            let raw = segment.text();
            let trimmed = raw.trim_matches(|ch| matches!(ch, '\r' | '\n'));
            if trimmed.is_empty() {
                return None;
            }
            let bounds = parsed_bounds_from_pdf_rect(segment.bounds());
            let mut result = ParsedTextElement {
                fill_stroke_width: None,
                font_weight: None,
                baseline_y: None,
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
                // referenceは独立したSDK segment.charsの先頭を使う。
                result.baseline_y = first
                    .angle_degrees()
                    .ok()
                    .filter(|a| a.is_finite() && a.rem_euclid(360.0) == 0.0)
                    .and_then(|_| first.origin_y().ok().map(|p| p.value))
                    .filter(|y| y.is_finite());
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
    super::extract(
        budget,
        std::path::Path::new("owned.pdf"),
        page,
        text,
        &chars,
    )
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
            let expected = native_segment_reference(text);
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
                super::extract(
                    &budget(0),
                    std::path::Path::new("owned.pdf"),
                    7,
                    text,
                    &shared
                ),
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

/// 本当に空のページを空結果とし、停止済み予算では空結果も返さない。
#[test]
fn ordinary_segments_preserve_empty_and_stopped_behavior() {
    with_page("", |text| {
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
    /// 文字原点とsizeを変えても印刷hyphenだけを復元し、SDKの矩形・style・String容量を変えない。
    #[test]
    fn printed_hyphen_restoration_keeps_geometry_and_capacity(x in 5u16..40, y in 80u16..170, size in 6u16..18) {
        let content = format!("BT /F {size} Tf {x} {y} Td (well-) Tj 0 -{} Td (known) Tj ET", size * 2);
        with_page(&content, |text| {
            let reference = native_segment_reference(text);
            let actual = run(&budget(65536), 7, text).unwrap();
            assert_eq!(actual.len(), reference.len());
            assert!(actual.iter().any(|element| element.text.contains("well-")));
            for (a, b) in actual.iter().zip(reference) {
                assert_eq!((a.bounds, a.baseline_y, a.font_size, &a.font_name, &a.fill_color), (b.bounds, b.baseline_y, b.font_size, &b.font_name, &b.fill_color));
            }
            let chars = characters(&budget(65536), text).unwrap();
            let mut preview = text.all();
            let storage = (preview.as_ptr(), preview.capacity());
            crate::pdfium_printed_hyphens::page(&budget(0), std::path::Path::new("owned-transformed-hyphen.pdf"), &mut preview, &chars).unwrap();
            assert!(preview.contains("well-"));
            assert!(!preview.contains('\u{2}'));
            assert_eq!((preview.as_ptr(), preview.capacity()), storage);
        });
    }

    /// 座標・文字サイズ・重なり・色が異なっても旧segmentの幾何/styleを保持する。
    #[test]
    fn ordinary_segments_match_native_segment_semantics(
        x in 0u16..100, y in 20u16..150, size in 5u16..30, overlap in proptest::bool::ANY,
    ) {
        let next_y = if overlap { y } else { y - 15 };
        let content = format!("BT /F {size} Tf 1 0 0 1 {x} {y} Tm (alpha) Tj 1 0 0 rg /F 18 Tf 1 0 0 1 {x} {next_y} Tm (beta) Tj ET");
        with_page(&content, |text| {
            assert_eq!(run(&budget(65536), 7, text).unwrap(), native_segment_reference(text));
        });
    }
}

/// native fallbackでも単語を繋げず、SDKが報告した端・単語間の別objectの空白を保持する。
#[test]
fn ordinary_segments_preserve_declared_word_spaces() {
    with_page("BT /F 12 Tf 10 80 Td ( alpha ) Tj ET", |text| {
        let output = run(&budget(65536), 7, text).unwrap();
        assert_eq!(
            output.iter().map(|e| e.text.as_str()).collect::<String>(),
            " alpha "
        );
    });
    with_page(
        "BT /F 12 Tf 10 80 Td (a) Tj /F 13 Tf ( ) Tj /F 12 Tf (b) Tj ET",
        |text| {
            assert_eq!(text.all(), "a b");
            let output = run(&budget(65536), 7, text).unwrap();
            assert_eq!(
                output.iter().map(|e| e.text.as_str()).collect::<String>(),
                "a b"
            );
        },
    );
    // SDKは空白だけの3文字を1文字へ畳む。raw抽出では観測された1文字を保持する。
    with_page("BT /F 12 Tf 10 80 Td (   ) Tj ET", |text| {
        assert_eq!(text.all(), " ");
        let output = run(&budget(65536), 7, text).unwrap();
        assert_eq!(
            output.iter().map(|e| e.text.as_str()).collect::<String>(),
            " "
        );
        assert!(output.iter().any(|e| e.text == " "));
    });
}

/// SDKが単独の3空白を1文字へ畳んでも、source側の同数の空白をnative合成で維持する。
#[test]
fn native_reconciliation_keeps_source_spaces_when_sdk_coalesces_them() {
    let bytes = owned_text_pdf("BT /F 12 Tf 10 80 Td (   ) Tj ET");
    with_page("BT /F 12 Tf 10 80 Td (   ) Tj ET", |text| {
        assert_eq!(text.all(), " ");
        assert_eq!(text.len(), 1);
    });
    let limits = ParseLimits::default();
    let parsed = crate::pdfium_backend::parse_pdf(
        std::path::Path::new("owned-spaces.pdf"),
        &bytes,
        &limits,
        &DecodeBudget::new(&limits),
    )
    .unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|e| e.text.as_str())
            .collect::<String>(),
        "   "
    );
}

/// baselineを報告しても元のink矩形は変えず、回転文字へ水平座標を推測しない。
#[test]
fn baseline_metadata_preserves_ink_and_rejects_rotated_glyphs() {
    with_page("BT /F 12 Tf 10 80 Td (-) Tj ET", |text| {
        let rect = text.segments().iter().next().unwrap().bounds();
        let output = run(&budget(65536), 7, text).unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].baseline_y, Some(80.0));
        assert_eq!(output[0].bounds, parsed_bounds_from_pdf_rect(rect));
        assert_ne!(output[0].bounds.y0, 80.0);
    });
    with_page("BT /F 12 Tf 0 1 -1 0 40 80 Tm (-) Tj ET", |text| {
        let output = run(&budget(65536), 7, text).unwrap();
        assert!(!output.is_empty());
        assert!(output.iter().all(|e| e.baseline_y.is_none()));
    });
}
