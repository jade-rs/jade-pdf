//! 未対応 Font の実際の文字描画をページ・Form・状態継承で診断する回帰。
use jade_pdf2docx_parser::{
    ParseError, ParseOptions, parse_pdf_bytes, parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;
use page_attribute_pdf::{pages, pdf, stream};

/// PDF 1.7 の Type3 はモード7も描画することを実際の Pdfium pixel で確認する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pdf17_type3_rendering_is_confirmed_by_native_pixels() {
    use pdfium_render::prelude::*;
    let pdfium = pdfium_auto::bind_pdfium_silent().unwrap();
    for (mode, painted) in [(0, true), (3, false), (7, true)] {
        let bytes = pdf(&objects(&format!(
            "BT /F 12 Tf {mode} Tr 1 0 0 1 10 50 Tm (A) Tj ET"
        )));
        let document = pdfium.load_pdf_from_byte_slice(&bytes, None).unwrap();
        let page = document.pages().get(0).unwrap();
        let bitmap = page
            .render_with_config(&PdfRenderConfig::new().set_target_width(200))
            .unwrap();
        let image = bitmap.as_image().to_rgb8();
        assert_eq!(
            image.pixels().any(|p| p.0.iter().any(|&c| c < 128)),
            painted,
            "mode {mode}"
        );
    }
}

/// CharProc を持つ最小の Type3 Font 辞書。
fn type3() -> String {
    "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 600 600] /FontMatrix [.001 0 0 .001 0 0] /CharProcs << /A 11 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [600] /Resources << >> >>".into()
}

/// 二ページで同じ Font と正常な比較用 Font を利用できる PDF を作る。
fn objects(content: &str) -> Vec<String> {
    let mut objects = pages(
        "/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R /G 12 0 R >> /XObject << /Fm 13 0 R >> >>",
    );
    objects[4] = stream(content);
    objects[5] = stream("");
    objects.extend([
        type3(),
        stream("600 0 d0 0 0 400 600 re f"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        form("<< >>", ""),
    ]);
    objects
}

/// Font 診断だけを既存 API の安定識別子から取り出す。
fn warnings(objects: &[String]) -> Vec<String> {
    parse_pdf_bytes("font.pdf", &pdf(objects))
        .unwrap()
        .quality
        .issues
        .iter()
        .map(ToString::to_string)
        .filter(|s| s.contains("unsupported_font"))
        .collect()
}

/// 局所 Resources を持つ Form を作る。
fn form(resources: &str, content: &str) -> String {
    format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources {resources} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// Type3 と未知 Subtype の文字描画を各ページ・最終 object で一度記録する。
#[test]
fn unsupported_fonts_are_reported_per_page_and_object() {
    for (subtype, reason) in [
        ("Type3", "type3"),
        ("UnknownFont", "unsupported_subtype"),
        ("CIDFontType2", "unsupported_subtype"),
    ] {
        let mut objects = objects("BT /F 12 Tf (A) Tj (A) Tj ET");
        if subtype != "Type3" {
            objects[9] = format!("<< /Type /Font /Subtype /{subtype} >>");
        }
        objects[5] = stream("BT /F 12 Tf [(A) 10 (A)] TJ ET");
        let issues = warnings(&objects);
        assert_eq!(issues.len(), 2, "{subtype}: {issues:?}");
        for (page, issue) in issues.iter().enumerate() {
            assert!(
                issue.contains(&format!("page {}", page + 1))
                    && issue.contains("object 10")
                    && issue.contains(reason),
                "{issue}"
            );
        }
    }
}

/// resource にあるだけ、または Tf の選択だけでは文字描画の欠落を報告しない。
#[test]
fn unused_fonts_and_fake_commands_are_not_reported() {
    for content in [
        "",
        "BT /F 12 Tf ET",
        "BT /F 12 Tf () Tj <> Tj [] TJ ET",
        "% BT /F 12 Tf (A) Tj ET\n",
        "(BT /F 12 Tf (A) Tj ET)",
        "[BT /F 12 Tf (A) Tj ET]",
        "<< /Fake (BT /F 12 Tf (A) Tj ET) >>",
        "BI /W 1 /H 1 /BPC 8 ID BT /F 12 Tf (A) Tj ET EI",
    ] {
        assert!(warnings(&objects(content)).is_empty(), "{content}");
    }
}

/// 既存の非 Type3 本文抽出対象は Subtype だけで拒否しない。
#[test]
fn ordinary_font_subtypes_keep_existing_behavior() {
    for subtype in ["Type1", "MMType1", "TrueType", "Type0"] {
        let mut objects = objects("BT /F 12 Tf (A) Tj ET");
        objects[9] = format!("<< /Type /Font /Subtype /{subtype} /BaseFont /Helvetica >>");
        assert!(warnings(&objects).is_empty(), "{subtype}");
    }
}

/// Contents 配列の字句分割と text state を結合して診断する。
#[test]
fn split_contents_keep_selected_font() {
    let mut objects = objects("BT /F");
    objects[2] = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 14 0 R] >>".into();
    objects.push(stream("12 Tf (A) Tj ET"));
    assert_eq!(warnings(&objects).len(), 1);
}

/// q/Q は選択 Font を復元し、BT/ET は Font を初期化しない。
#[test]
fn saved_and_persistent_font_state_is_used() {
    for content in [
        "BT /F 12 Tf q /G 12 Tf (A) Tj Q (A) Tj ET",
        "BT /F 12 Tf ET BT (A) ' ET",
        "BT /F 12 Tf 0 0 (A) \" ET",
    ] {
        assert_eq!(warnings(&objects(content)).len(), 1, "{content}");
    }
    assert!(warnings(&objects("BT /F 12 Tf q /G 12 Tf (A) Tj Q ET")).is_empty());
}

/// 親の選択済み Font は Form の同名 resource に置換されず、Form の変更は復帰する。
#[test]
fn form_font_inheritance_shadow_and_restore_are_diagnosed() {
    let mut objects = objects("BT /F 12 Tf ET /Fm Do BT (A) Tj ET");
    objects[12] = form(
        "<< /Font << /F 12 0 R >> >>",
        "BT (A) Tj /F 12 Tf (A) Tj ET",
    );
    assert_eq!(warnings(&objects).len(), 1);
    objects[4] = stream("BT /G 12 Tf ET /Fm Do BT (A) Tj ET");
    objects[12] = form("<< /Font << /F 10 0 R >> >>", "BT /F 12 Tf (A) Tj ET");
    assert_eq!(warnings(&objects).len(), 1);
    objects[12] = form("<< /Font << /F 10 0 R >> >>", "BT /F 12 Tf ET");
    assert!(warnings(&objects).is_empty());
}

/// 間接 map・Font alias・Subtype と escaped 名を解決して最終 Font 番号を報告する。
#[test]
fn indirect_and_direct_font_identity_is_preserved() {
    let mut objects = objects("BT /F#31 12 Tf (A) Tj ET");
    objects[1] = "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] /MediaBox [0 0 100 100] /Resources << /Font 14 0 R >> >>".into();
    objects[9] = "15 0 R".into();
    objects.extend([
        "<< /F1 10 0 R >>".into(),
        "<< /Type /Font /Subtype 16 0 R >>".into(),
        "/Type3".into(),
    ]);
    let issues = warnings(&objects);
    assert_eq!(issues.len(), 1);
    assert!(issues[0].contains("object 15"), "{issues:?}");
    objects[13] = "<< /F1 << /Type /Font /Subtype /Type3 >> >>".into();
    assert!(warnings(&objects)[0].contains("object unknown"));
}

/// bytes strict は抽出文字があっても未対応 glyph 描画を拒否する。
#[test]
fn strict_rejects_unsupported_font() {
    let bytes = pdf(&objects("BT /F 12 Tf (A) Tj ET"));
    assert!(matches!(
        parse_pdf_bytes_with_options(
            "font.pdf",
            &bytes,
            &ParseOptions {
                strict: true,
                ..Default::default()
            }
        ),
        Err(ParseError::QualityRejected { .. })
    ));
}

/// 不可視モード3だけに使う未対応Fontはglyph描画の欠落を報告しない。
#[test]
fn invisible_font_use_does_not_reject_strict_conversion() {
    for content in [
        "3 Tr BT /F 12 Tf (A) Tj ET",
        "BT /F 12 Tf 3 Tr [(A) 20 (A)] TJ ET",
        "BT /F 12 Tf 3 Tr (A) ' ET",
        "BT /F 12 Tf 3 Tr 0 0 (A) \" ET",
    ] {
        let objects = objects(content);
        assert!(warnings(&objects).is_empty(), "{content}");
        assert!(
            parse_pdf_bytes_with_options(
                "invisible-font.pdf",
                &pdf(&objects),
                &ParseOptions {
                    strict: true,
                    ..Default::default()
                }
            )
            .is_ok(),
            "{content}"
        );
    }
}

/// BT・q/Q・Contentsを越える描画モードをFont診断にも適用する。
#[test]
fn font_diagnostics_follow_saved_and_persistent_rendering_mode() {
    for content in [
        "3 Tr q 0 Tr BT /G 12 Tf (PUBLIC) Tj ET Q BT /F 12 Tf (A) Tj ET",
        "BT /F 12 Tf 3 Tr (A) Tj ET BT (A) Tj ET",
        "3 Tr BT /F 12 Tf q 0 Tr /G 12 Tf (PUBLIC) Tj Q (A) Tj ET",
    ] {
        assert!(warnings(&objects(content)).is_empty(), "{content}");
    }
    let mut objects = objects("3 Tr BT /F 12 Tf ET");
    objects[2] = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 14 0 R] >>".into();
    objects.push(stream("BT (A) Tj ET"));
    assert!(warnings(&objects).is_empty());
    objects[5] = stream("BT /F 12 Tf (A) Tj ET");
    let issues = warnings(&objects);
    assert_eq!(issues.len(), 1);
    assert!(issues[0].contains("page 2"), "{issues:?}");
}

/// 不可視描画をseenへ登録せず、後続の可視・clip描画を診断する。
#[test]
fn hidden_font_use_does_not_mask_later_visible_or_clipping_use() {
    for mode in [0, 1, 2, 4, 5, 6, 7] {
        let content = format!("BT /F 12 Tf 3 Tr (A) Tj {mode} Tr (A) Tj ET");
        assert_eq!(warnings(&objects(&content)).len(), 1, "{content}");
    }
}

/// 偽Tr・不正operandは描画モードを更新せず、可視Fontを隠さない。
#[test]
fn fake_and_invalid_rendering_commands_do_not_hide_font_diagnostics() {
    for content in [
        "BT /F 12 Tf [3 Tr] << /Fake 3 Tr >> (3 Tr) Tj ET",
        "BT /F 12 Tf 3.5 Tr -1 Tr 8 Tr (A) Tj ET",
        "BT /F 12 Tf 3 0 Tr (A) Tj ET",
    ] {
        assert_eq!(warnings(&objects(content)).len(), 1, "{content}");
    }
    assert!(
        warnings(&objects(
            "3 Tr [0 Tr] << /Fake 0 Tr >> BT /F 12 Tf (A) Tj ET"
        ))
        .is_empty()
    );
}

/// Formへモードを継承し、局所変更を親へ漏らさずFont診断を行う。
#[test]
fn form_font_diagnostics_inherit_and_restore_rendering_mode() {
    let mut objects = objects("3 Tr BT /F 12 Tf ET /Fm Do BT (A) Tj ET");
    objects[12] = form("<< /Font << /F 10 0 R >> >>", "BT (A) Tj ET");
    assert!(warnings(&objects).is_empty());
    objects[12] = form("<< /Font << /F 10 0 R >> >>", "0 Tr BT (A) Tj ET");
    assert_eq!(warnings(&objects).len(), 1);
    objects[4] = stream("BT /G 12 Tf ET /Fm Do BT /F 12 Tf (A) Tj ET");
    objects[12] = form("<< /Font << /F 10 0 R >> >>", "3 Tr BT /F 12 Tf (A) Tj ET");
    let issues = warnings(&objects);
    assert_eq!(issues.len(), 1);
    assert!(issues[0].contains("object 10"), "{issues:?}");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// モード列の途中にglyphを使う描画が一つでもある場合だけ診断する。
    #[test]
    fn rendering_sequence_reports_glyph_use(modes in prop_oneof![Just(vec![3u8]), prop::collection::vec(0u8..8, 1..25)]) {
        let expected = modes.iter().any(|mode| *mode != 3);
        let mut content = String::from("BT /F 12 Tf ");
        for mode in modes { content.push_str(&format!("{mode} Tr (A) Tj ")); }
        content.push_str("ET");
        prop_assert_eq!(warnings(&objects(&content)).len(), usize::from(expected));
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, failure_persistence: None, ..ProptestConfig::default() })]
    /// Font 選択と空文字描画を繰り返しても診断の重複・未描画の誤判定を起こさない。
    #[test]
    fn repetitions_only_report_used_fonts(repeat in 1usize..12, used in any::<bool>()) {
        let text = if used { "(A)" } else { "()" };
        let content = format!("BT {} ET", format!("/F 12 Tf {text} Tj ").repeat(repeat));
        prop_assert_eq!(warnings(&objects(&content)).len(), usize::from(used));
    }
}

/// Type3のpaintと診断を実効versionごとに判定し、通常Fontのclipは本文へ出さない。
#[test]
fn type3_visibility_depends_on_effective_version_and_font() {
    for version in ["", "/Version /2.0"] {
        for mode in 0..8 {
            let mut objects = objects(&format!(
                "BT /F 12 Tf {mode} Tr 10 50 Td (A) Tj /G 12 Tf (CLIP) Tj 0 Tr (PUBLIC) Tj ET"
            ));
            objects[0] = format!("<< /Type /Catalog /Pages 2 0 R {version} >>");
            let parsed = parse_pdf_bytes("type3.pdf", &pdf(&objects)).unwrap();
            let painted = mode != 3 && (mode != 7 || version.is_empty());
            let text: String = parsed
                .text_elements
                .iter()
                .map(|e| e.text.as_str())
                .collect();
            assert_eq!(
                text.starts_with('A'),
                painted,
                "{version} mode {mode}: {text}"
            );
            assert_eq!(text.contains("CLIP"), !matches!(mode, 3 | 7));
            assert!(text.ends_with("PUBLIC"));
            assert_eq!(
                parsed
                    .quality
                    .issues
                    .iter()
                    .any(|i| i.to_string().contains("unsupported_font")),
                painted,
                "{version} mode {mode}"
            );
        }
    }
}

/// PDF2の不可視Type3モード7だけならstrictを通し、他の未対応Fontのclipは診断する。
#[test]
fn pdf20_type3_clip_only_use_is_not_a_quality_loss() {
    let mut objects = objects("BT /F 12 Tf 7 Tr 10 50 Td (A) Tj /G 12 Tf 0 Tr (PUBLIC) Tj ET");
    objects[0] = "<< /Type /Catalog /Pages 2 0 R /Version /2.0 >>".into();
    let parsed = parse_pdf_bytes_with_options(
        "type3.pdf",
        &pdf(&objects),
        &ParseOptions {
            strict: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(parsed.text_preview, "PUBLIC");
    objects[9] = "<< /Type /Font /Subtype /Unknown >>".into();
    assert!(
        warnings(&objects)
            .iter()
            .any(|i| i.contains("unsupported_subtype"))
    );
}

/// Form局所Type3と通常Fontの同名aliasを区別し、版による不可視診断を保持する。
#[test]
fn form_type3_visibility_uses_local_subtype_and_effective_version() {
    for version in ["", "/Version /2.0"] {
        let mut objects = objects(
            "BT /G 12 Tf 7 Tr 10 50 Td (CLIP) Tj ET /Fm Do BT /G 12 Tf 0 Tr 10 20 Td (PUBLIC) Tj ET",
        );
        objects[0] = format!("<< /Type /Catalog /Pages 2 0 R {version} >>");
        objects[12] = form(
            "<< /Font << /G 10 0 R >> >>",
            "BT /G 12 Tf 10 50 Td (A) Tj ET",
        );
        let parsed = parse_pdf_bytes("type3.pdf", &pdf(&objects)).unwrap();
        assert_eq!(
            parsed.text_elements.iter().any(|e| e.text == "A"),
            version.is_empty()
        );
        assert_eq!(warnings(&objects).len(), usize::from(version.is_empty()));
        assert!(!parsed.text_preview.contains("CLIP"));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 版・描画モード・文字描画演算子の組合せでもType3の可視性を維持する。
    #[test]
    fn type3_version_mode_property(pdf20 in any::<bool>(), mode in 0u8..8, operator in 0usize..4) {
        let shown = ["(A) Tj", "[(A)] TJ", "(A) '", "0 0 (A) \""][operator];
        let mut objects = objects(&format!("BT /F 12 Tf {mode} Tr 10 50 Td {shown} ET"));
        if pdf20 {objects[0] = "<< /Type /Catalog /Pages 2 0 R /Version /2.0 >>".into();}
        let parsed = parse_pdf_bytes("type3.pdf", &pdf(&objects)).unwrap();
        let painted = mode != 3 && (mode != 7 || !pdf20);
        prop_assert_eq!(parsed.text_elements.iter().any(|e| e.text == "A"), painted);
        prop_assert_eq!(warnings(&objects).len(), usize::from(painted));
    }
}

/// 同座標・同文字・同Font名でもnativeは通常FontのclipをType3に取り違えない。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_type3_identity_survives_direct_fonts_forms_and_name_collisions() {
    let dir =
        std::env::temp_dir().join(format!("jade-pdf2docx-type3-fonts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (index, direct) in [false, true].into_iter().enumerate() {
        let mut objects = objects(
            "BT /G 12 Tf 7 Tr 10 50 Td (A) Tj ET /Fm Do BT /G 12 Tf 0 Tr 10 20 Td (PUBLIC) Tj 0 -15 Td (A) Tj ET",
        );
        let font = type3().replace("/Subtype /Type3", "/Subtype /Type3 /BaseFont /Helvetica");
        objects[9] = font.clone();
        objects[11] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding << /Type /Encoding /Differences [65 /Z] >> >>".into();
        objects[12] = form(
            &format!(
                "<< /Font << /G {} >> >>",
                if direct { font.as_str() } else { "10 0 R" }
            ),
            "BT /G 12 Tf 7 Tr 10 50 Td (A) Tj ET",
        );
        let path = dir.join(format!("{index}.pdf"));
        std::fs::write(&path, pdf(&objects)).unwrap();
        let result = jade_pdf2docx_parser::parse_pdf(&path);
        std::fs::remove_file(&path).unwrap();
        let parsed = result.unwrap();
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .map(|e| e.text.matches('A').count())
                .sum::<usize>(),
            1,
            "{:?}",
            parsed.text_elements
        );
        assert!(parsed.text_preview.contains("PUBLIC"));
        assert!(
            parsed.text_preview.contains('Z'),
            "{:?}",
            parsed.text_preview
        );
        assert!(parsed.text_elements.iter().all(|e| {
            !e.font_name
                .as_deref()
                .unwrap_or_default()
                .contains("jade-pdf2docxType3_")
        }));
    }
    std::fs::remove_dir(&dir).unwrap();
}
