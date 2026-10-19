//! metadata 索引・検証・抽出配列の予算超過を公開 API で返す回帰。
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes_with_limits};
#[path = "support/page_attribute_pdf.rs"]
mod page_attribute_pdf;

/// 二ページと共有 Font を持つ正式な PDF を作る。
fn shared_font_pdf() -> Vec<u8> {
    let mut objects =
        page_attribute_pdf::pages("/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> >>");
    objects[4] = page_attribute_pdf::stream("BT /F 10 Tf 10 10 Td (A) Tj ET");
    objects[5] = page_attribute_pdf::stream("BT /F 10 Tf 10 10 Td (B) Tj ET");
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    page_attribute_pdf::pdf(&objects)
}

/// 予算0で page tree/抽出の索引構築を拒否する。
#[test]
fn zero_metadata_budget_rejects_valid_pdf() {
    let bytes = page_attribute_pdf::pdf(&page_attribute_pdf::pages("/MediaBox [0 0 500 500]"));
    assert!(
        matches!(parse_pdf_bytes_with_limits("metadata-budget.pdf", &bytes, &ParseLimits {
        max_total_metadata_workspace_bytes: 0, ..Default::default()
    }), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit: 0, actual }) if actual > 0)
    );
}

/// 未対応描画もContents境界・別名・ページの初出順とstrict品質判定を保存する。
#[test]
fn unsupported_diagnostics_preserve_identity_at_capacity_boundary() {
    use jade_pdf2docx_parser::{
        ParseOptions, QualityIssue, UnsupportedXObjectReason, parse_pdf_bytes_with_options,
    };
    let mut objects = page_attribute_pdf::pages(
        "/MediaBox [0 0 100 100] /Resources << /XObject << /F 7 0 R /Alias 7 0 R >> >>",
    );
    objects[2] = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 10 0 R] >>".into();
    objects[4] = page_attribute_pdf::stream("/#46");
    objects[5] = page_attribute_pdf::stream("/Alias Do /F Do");
    objects[6] = "<< /Type /XObject /Subtype /PS /Length 0 >>\nstream\n\nendstream".into();
    objects.push(page_attribute_pdf::stream("Do /Alias Do BI /F Do EI"));
    let bytes = page_attribute_pdf::pdf(&objects);
    let expected = vec![
        QualityIssue::UnsupportedXObject {
            page: 0,
            object_number: 7,
            reason: UnsupportedXObjectReason::Subtype,
        },
        QualityIssue::UnsupportedInlineImage { page: 0 },
        QualityIssue::UnsupportedXObject {
            page: 1,
            object_number: 7,
            reason: UnsupportedXObjectReason::Subtype,
        },
    ];
    let capacity = minimum_metadata_capacity(&bytes);
    for _ in 0..2 {
        let limits = ParseLimits {
            max_total_metadata_workspace_bytes: capacity,
            ..Default::default()
        };
        let document =
            parse_pdf_bytes_with_limits("unsupported-budget.pdf", &bytes, &limits).unwrap();
        assert_eq!(document.quality.issues, expected);
        assert!(
            matches!(parse_pdf_bytes_with_limits("unsupported-budget.pdf", &bytes, &ParseLimits { max_total_metadata_workspace_bytes: capacity - 1, ..Default::default() }), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit, actual }) if limit == capacity - 1 && actual == capacity)
        );
        assert!(matches!(
            parse_pdf_bytes_with_options(
                "unsupported-budget.pdf",
                &bytes,
                &ParseOptions {
                    limits,
                    strict: true,
                    ..Default::default()
                }
            ),
            Err(ParseError::QualityRejected { .. })
        ));
    }
}

/// 共有画像の置換と文字なし・画像外文字ページのOCR診断を、累積容量境界でも保持する。
#[test]
fn image_and_ocr_diagnostics_preserve_identity_at_capacity_boundary() {
    use jade_pdf2docx_parser::{
        ImageReplacementReason, ParseOptions, QualityIssue, parse_pdf_bytes_with_options,
    };
    let mut objects = page_attribute_pdf::pages(
        "/MediaBox [0 0 100 100] /Resources << /XObject << /Im 7 0 R >> /Font << /F 10 0 R >> >>",
    );
    objects[4] = page_attribute_pdf::stream(
        "q 10 0 0 10 0 0 cm /Im Do /Im Do Q BT /F 10 Tf 10 30 Td (A) Tj ET",
    );
    objects[5] = page_attribute_pdf::stream("q 10 0 0 10 0 0 cm /Im Do /Im Do Q");
    objects[6] = "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream".into();
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    let bytes = page_attribute_pdf::pdf(&objects);
    let expected = vec![
        QualityIssue::ImageReplaced {
            object_number: Some(7),
            reason: ImageReplacementReason::UnsupportedFilter,
        },
        QualityIssue::OcrRequired { page: 0 },
        QualityIssue::OcrRequired { page: 1 },
    ];
    let capacity = minimum_metadata_capacity(&bytes);
    for _ in 0..2 {
        let limits = ParseLimits {
            max_total_metadata_workspace_bytes: capacity,
            ..Default::default()
        };
        let document =
            parse_pdf_bytes_with_limits("image-diagnostic-budget.pdf", &bytes, &limits).unwrap();
        assert_eq!(document.quality.issues, expected);
        assert_eq!(document.image_elements.len(), 4);
        assert_eq!(document.text_elements[0].text, "A");
        assert!(
            matches!(parse_pdf_bytes_with_limits("image-diagnostic-budget.pdf", &bytes, &ParseLimits { max_total_metadata_workspace_bytes: capacity - 1, ..Default::default() }), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit, actual }) if limit == capacity - 1 && actual == capacity)
        );
        assert!(matches!(
            parse_pdf_bytes_with_options(
                "image-diagnostic-budget.pdf",
                &bytes,
                &ParseOptions {
                    limits,
                    strict: true,
                    ..Default::default()
                }
            ),
            Err(ParseError::QualityRejected { .. })
        ));
    }
}

/// page tree・ページ/Form 抽出で再作成した索引を累積し、解析間で予算を分離する。
#[test]
fn cumulative_metadata_capacity_is_inclusive_at_public_api() {
    let bytes = shared_font_pdf();
    let capacity = minimum_metadata_capacity(&bytes);
    assert!(capacity > 0);
    for _ in 0..2 {
        assert!(
            matches!(parse_pdf_bytes_with_limits("metadata-budget.pdf", &bytes, &ParseLimits {
            max_total_metadata_workspace_bytes: capacity - 1, ..Default::default()
        }), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit, actual }) if limit == capacity - 1 && actual == capacity)
        );
        let parsed = parse_pdf_bytes_with_limits(
            "metadata-budget.pdf",
            &bytes,
            &ParseLimits {
                max_total_metadata_workspace_bytes: capacity,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(parsed.pages.len(), 2);
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .map(|text| text.text.as_str())
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
        assert_eq!(parsed.text_elements.len(), 2);
    }
}

/// 同一Fontのページ/Form描画を診断し、累積容量境界でも初出順と解析間分離を保つ。
#[test]
fn font_diagnostics_preserve_page_identity_at_capacity_boundary() {
    use jade_pdf2docx_parser::{QualityIssue, UnsupportedFontReason};
    let mut objects = page_attribute_pdf::pages(
        "/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    objects[4] = page_attribute_pdf::stream("BT /F 12 Tf (A) Tj ET /Fm Do /Fm Do");
    objects[5] = page_attribute_pdf::stream("BT /F 12 Tf (B) Tj ET /Fm Do");
    let body = "BT /F 12 Tf (C) Tj ET";
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Length {} >>\nstream\n{body}\nendstream",
        body.len()
    );
    objects.push("<< /Type /Font /Subtype /Type3 >>".into());
    let bytes = page_attribute_pdf::pdf(&objects);
    let baseline =
        parse_pdf_bytes_with_limits("font-diagnostics.pdf", &bytes, &ParseLimits::default())
            .unwrap();
    assert_eq!(
        baseline
            .quality
            .issues
            .iter()
            .filter(|issue| matches!(issue, QualityIssue::UnsupportedFont { .. }))
            .cloned()
            .collect::<Vec<_>>(),
        [
            QualityIssue::UnsupportedFont {
                page: 0,
                object_number: Some(10),
                reason: UnsupportedFontReason::Type3
            },
            QualityIssue::UnsupportedFont {
                page: 1,
                object_number: Some(10),
                reason: UnsupportedFontReason::Type3
            },
        ]
    );
    let capacity = minimum_metadata_capacity(&bytes);
    for _ in 0..2 {
        assert!(
            matches!(parse_pdf_bytes_with_limits("font-diagnostics.pdf", &bytes, &ParseLimits {
            max_total_metadata_workspace_bytes: capacity - 1, ..Default::default()
        }), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit, actual }) if limit == capacity - 1 && actual == capacity)
        );
        let parsed = parse_pdf_bytes_with_limits(
            "font-diagnostics.pdf",
            &bytes,
            &ParseLimits {
                max_total_metadata_workspace_bytes: capacity,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(parsed.quality, baseline.quality);
        assert_eq!(parsed.text_elements, baseline.text_elements);
    }
}

/// stream間をまたぐDoと別ページの共有Formを、累積容量境界でも保持する。
#[test]
fn joined_page_contents_and_shared_form_are_preserved_at_capacity_boundary() {
    let mut objects = page_attribute_pdf::pages(
        "/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >>",
    );
    for page in &mut objects[2..4] {
        *page = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 6 0 R] >>".into();
    }
    objects[4] = page_attribute_pdf::stream("/Fm");
    objects[5] = page_attribute_pdf::stream("Do");
    let content = "BT /F 12 Tf 10 20 Td (FORM) Tj ET";
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    let bytes = page_attribute_pdf::pdf(&objects);
    let baseline =
        parse_pdf_bytes_with_limits("form-storage.pdf", &bytes, &ParseLimits::default()).unwrap();
    assert_eq!(
        baseline
            .text_elements
            .iter()
            .map(|text| (text.page, text.text.as_str()))
            .collect::<Vec<_>>(),
        [(0, "FORM"), (1, "FORM")]
    );
    let capacity = minimum_metadata_capacity(&bytes);
    for _ in 0..2 {
        assert!(matches!(
            parse_pdf_bytes_with_limits("form-storage.pdf", &bytes, &ParseLimits {
                max_total_metadata_workspace_bytes: capacity - 1, ..Default::default()
            }),
            Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit, actual })
                if limit == capacity - 1 && actual == capacity
        ));
        let document = parse_pdf_bytes_with_limits(
            "form-storage.pdf",
            &bytes,
            &ParseLimits {
                max_total_metadata_workspace_bytes: capacity,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(document.pages.len(), 2);
        assert_eq!(
            document
                .text_elements
                .iter()
                .map(|text| (text.page, text.text.as_str()))
                .collect::<Vec<_>>(),
            [(0, "FORM"), (1, "FORM")]
        );
    }
}

/// 入れ子Formとq/Qの座標・再呼出しを累積容量境界でも保持する。
#[test]
fn nested_form_matrices_are_preserved_at_capacity_boundary() {
    let mut objects = page_attribute_pdf::pages(
        "/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R /Child 11 0 R >> >>",
    );
    objects[4] = page_attribute_pdf::stream("q 1 0 0 1 5 6 cm /Fm Do Q /Fm Do");
    objects[5] = page_attribute_pdf::stream("");
    let body = "q 1 0 0 1 20 30 cm BT /F 12 Tf 10 20 Td (FIRST) Tj ET Q BT /F 12 Tf 10 20 Td (SECOND) Tj ET /Child Do";
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [-100 -100 500 500] /Length {} >>\nstream\n{body}\nendstream",
        body.len()
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    let body = "BT /F 12 Tf 40 50 Td (CHILD) Tj ET";
    objects.push(format!("<< /Type /XObject /Subtype /Form /BBox [-100 -100 500 500] /Length {} >>\nstream\n{body}\nendstream", body.len()));
    let bytes = page_attribute_pdf::pdf(&objects);
    let baseline =
        parse_pdf_bytes_with_limits("form-state.pdf", &bytes, &ParseLimits::default()).unwrap();
    let expected = [
        ("FIRST", 35., 56.),
        ("SECOND", 15., 26.),
        ("CHILD", 45., 56.),
        ("FIRST", 30., 50.),
        ("SECOND", 10., 20.),
        ("CHILD", 40., 50.),
    ];
    assert_eq!(
        baseline
            .text_elements
            .iter()
            .map(|text| (text.text.as_str(), text.bounds.x0, text.bounds.y0))
            .collect::<Vec<_>>(),
        expected
    );
    let capacity = minimum_metadata_capacity(&bytes);
    for _ in 0..2 {
        assert!(
            matches!(parse_pdf_bytes_with_limits("form-state.pdf", &bytes, &ParseLimits {
            max_total_metadata_workspace_bytes: capacity - 1, ..Default::default()
        }), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit, actual })
            if limit == capacity - 1 && actual == capacity)
        );
        let parsed = parse_pdf_bytes_with_limits(
            "form-state.pdf",
            &bytes,
            &ParseLimits {
                max_total_metadata_workspace_bytes: capacity,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(parsed.pages.len(), 2);
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .map(|text| (text.text.as_str(), text.bounds.x0, text.bounds.y0))
                .collect::<Vec<_>>(),
            expected
        );
    }
}

/// crop・斜めの親Form・子Form・文字の切り抜きを再描画/別ページでも容量境界で保つ。
#[test]
fn nested_clip_geometry_is_preserved_at_capacity_boundary() {
    let mut objects = page_attribute_pdf::pages(
        "/MediaBox [0 0 100 100] /CropBox [0 0 80 80] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R /Child 11 0 R >> >>",
    );
    objects[4] = page_attribute_pdf::stream("/Fm Do /Fm Do");
    objects[5] = page_attribute_pdf::stream("/Fm Do");
    let body = "/Child Do";
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 50 50] /Matrix [1 0.5 0 1 20 20] /Length {} >>\nstream\n{body}\nendstream",
        body.len()
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    let body = "BT /F 12 Tf 10 10 Td (VISIBLE) Tj ET BT /F 12 Tf 100 100 Td (HIDDEN) Tj ET";
    objects.push(format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 50 50] /Length {} >>\nstream\n{body}\nendstream",
        body.len()
    ));
    let bytes = page_attribute_pdf::pdf(&objects);
    let baseline =
        parse_pdf_bytes_with_limits("form-clip.pdf", &bytes, &ParseLimits::default()).unwrap();
    assert_eq!(
        baseline
            .text_elements
            .iter()
            .map(|text| (text.page, text.text.as_str()))
            .collect::<Vec<_>>(),
        [(0, "VISIBLE"), (0, "VISIBLE"), (1, "VISIBLE")]
    );
    assert!(
        baseline
            .text_elements
            .iter()
            .all(|text| text.bounds.x0 == 30.
                && text.bounds.y0 == 35.
                && text.bounds.x1 <= 70.
                && text.bounds.y1 <= 80.)
    );
    let capacity = minimum_metadata_capacity(&bytes);
    for _ in 0..2 {
        assert!(matches!(
            parse_pdf_bytes_with_limits("form-clip.pdf", &bytes, &ParseLimits {
                max_total_metadata_workspace_bytes: capacity - 1, ..Default::default()
            }),
            Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit, actual })
                if limit == capacity - 1 && actual == capacity
        ));
        let parsed = parse_pdf_bytes_with_limits(
            "form-clip.pdf",
            &bytes,
            &ParseLimits {
                max_total_metadata_workspace_bytes: capacity,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(parsed.pages, baseline.pages);
        assert_eq!(parsed.text_elements, baseline.text_elements);
        assert_eq!(parsed.quality, baseline.quality);
    }
}

/// 公開 API が成功する最小の累積要求容量を、超過量を使って求める。
fn minimum_metadata_capacity(bytes: &[u8]) -> u64 {
    let mut capacity = 0;
    for _ in 0..1024 {
        match parse_pdf_bytes_with_limits(
            "graph-budget.pdf",
            bytes,
            &ParseLimits {
                max_total_metadata_workspace_bytes: capacity,
                ..Default::default()
            },
        ) {
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                actual,
                ..
            }) => {
                assert!(actual > capacity);
                capacity = actual;
            }
            Ok(parsed) => {
                assert_eq!(parsed.pages.len(), 2);
                return capacity;
            }
            result => panic!("unexpected {result:?}"),
        }
    }
    panic!("metadata の累積境界へ到達しない");
}

/// 正式 Root がなくても抽出配列の累積境界と本文・ページを保持する。
#[test]
fn legacy_page_extraction_capacity_is_inclusive_at_public_api() {
    let bytes = String::from_utf8(shared_font_pdf())
        .unwrap()
        .replace("/Root 1 0 R", "")
        .into_bytes();
    let capacity = minimum_metadata_capacity(&bytes);
    for _ in 0..2 {
        assert!(
            matches!(parse_pdf_bytes_with_limits("legacy-budget.pdf", &bytes, &ParseLimits {
            max_total_metadata_workspace_bytes: capacity - 1, ..Default::default()
        }), Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes", limit, actual
        }) if limit == capacity - 1 && actual == capacity)
        );
        let document = parse_pdf_bytes_with_limits(
            "legacy-budget.pdf",
            &bytes,
            &ParseLimits {
                max_total_metadata_workspace_bytes: capacity,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(document.pages.len(), 2);
        assert_eq!(
            document
                .text_elements
                .iter()
                .map(|text| text.text.as_str())
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
    }
}

/// Contents 配列の初出順・別ページの共有 stream を、コメントと予算境界を含めて保持する。
#[test]
fn contents_array_order_is_preserved_at_public_budget_boundary() {
    let mut objects =
        page_attribute_pdf::pages("/MediaBox [0 0 500 500] /Resources << /Font << /F 10 0 R >> >>");
    objects[2] =
        "<< /Type /Page /Parent 2 0 R /Contents [6 % object\n 0 % generation\n R 5 0 R 6 0 R] >>"
            .into();
    objects[3] = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 6 0 R] >>".into();
    objects[4] = page_attribute_pdf::stream("BT /F 10 Tf 10 10 Td (A) Tj ET");
    objects[5] = page_attribute_pdf::stream("BT /F 10 Tf 10 50 Td (B) Tj ET");
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into());
    let bytes = page_attribute_pdf::pdf(&objects);
    let capacity = minimum_metadata_capacity(&bytes);
    assert!(
        matches!(parse_pdf_bytes_with_limits("contents-budget.pdf", &bytes, &ParseLimits {
        max_total_metadata_workspace_bytes: capacity - 1, ..Default::default()
    }), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit, actual
    }) if limit == capacity - 1 && actual == capacity)
    );
    let document = parse_pdf_bytes_with_limits(
        "contents-budget.pdf",
        &bytes,
        &ParseLimits {
            max_total_metadata_workspace_bytes: capacity,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(document.pages.len(), 2);
    assert_eq!(
        document
            .text_elements
            .iter()
            .map(|text| (text.page, text.text.as_str()))
            .collect::<Vec<_>>(),
        [(0, "B"), (0, "A"), (1, "A"), (1, "B")]
    );
}

/// 同じ object 件数でも、未描画 Form の検証用集合・列・属性配列は追加予算を使う。
#[test]
fn unpainted_form_graph_requires_additional_metadata_workspace() {
    let mut objects = page_attribute_pdf::pages("/MediaBox [0 0 500 500] /Resources << >>");
    objects[4] = page_attribute_pdf::stream("");
    objects[5] = page_attribute_pdf::stream("");
    let mut entries = String::new();
    for index in 0..48 {
        let number = objects.len() + 1;
        entries.push_str(&format!(" /Form{index} {number} 0 R"));
        objects.push("<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << >> /Length 0 >>\nstream\n\nendstream".into());
    }
    let unused = page_attribute_pdf::pdf(&objects);
    let baseline = minimum_metadata_capacity(&unused);
    objects[1] = format!(
        "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] /MediaBox [0 0 500 500] /Resources << /XObject << {entries} >> >> >>"
    );
    let used = page_attribute_pdf::pdf(&objects);
    assert!(
        matches!(parse_pdf_bytes_with_limits("graph-budget.pdf", &used, &ParseLimits {
        max_total_metadata_workspace_bytes: baseline, ..Default::default()
    }), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual > baseline)
    );
    let capacity = minimum_metadata_capacity(&used);
    assert!(capacity > baseline);
    assert!(
        matches!(parse_pdf_bytes_with_limits("graph-budget.pdf", &used, &ParseLimits {
        max_total_metadata_workspace_bytes: capacity - 1, ..Default::default()
    }), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == capacity)
    );
    for _ in 0..2 {
        let parsed = parse_pdf_bytes_with_limits(
            "graph-budget.pdf",
            &used,
            &ParseLimits {
                max_total_metadata_workspace_bytes: capacity,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(parsed.pages.len(), 2);
        assert!(parsed.text_elements.is_empty());
    }
}

/// XObject の間接辞書・escaped 名と画像配置を、正式/Root なしの予算境界で保持する。
#[test]
fn xobject_image_placements_survive_public_metadata_boundary() {
    let mut objects = page_attribute_pdf::pages("/MediaBox [0 0 100 100] /Resources 8 0 R");
    objects[8] = "<< /#49m 7 0 R >>".into();
    for legacy in [false, true] {
        let bytes = if legacy {
            objects[2] = "<< /Type /Page /Parent 2 0 R /Contents 5 0 R /MediaBox [0 0 100 100] /Resources 8 0 R >>".into();
            objects[3] = "<< /Type /Page /Parent 2 0 R /Contents 6 0 R /MediaBox [0 0 100 100] /Resources 8 0 R >>".into();
            String::from_utf8(page_attribute_pdf::pdf(&objects))
                .unwrap()
                .replace("/Root 1 0 R", "")
                .into_bytes()
        } else {
            page_attribute_pdf::pdf(&objects)
        };
        let baseline =
            parse_pdf_bytes_with_limits("xobject-budget.pdf", &bytes, &ParseLimits::default())
                .unwrap();
        let capacity = minimum_metadata_capacity(&bytes);
        assert!(
            matches!(parse_pdf_bytes_with_limits("xobject-budget.pdf", &bytes, &ParseLimits {
            max_total_metadata_workspace_bytes: capacity - 1, ..Default::default()
        }), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit, actual })
            if limit == capacity - 1 && actual == capacity)
        );
        let document = parse_pdf_bytes_with_limits(
            "xobject-budget.pdf",
            &bytes,
            &ParseLimits {
                max_total_metadata_workspace_bytes: capacity,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            document.image_elements, baseline.image_elements,
            "legacy={legacy}"
        );
        assert_eq!(document.image_elements.len(), 2, "legacy={legacy}");
        for (page, image) in document.image_elements.iter().enumerate() {
            assert_eq!(image.page, page);
            assert_eq!(
                image.bounds.x1 - image.bounds.x0,
                if page == 0 { 10. } else { 20. }
            );
            assert!(!image.png_bytes.is_empty());
        }
    }
}

/// 共有/別画像の配列拡張後も、容量境界で配置・PNG・ページ診断を保持する。
#[test]
fn image_storage_growth_preserves_geometry_and_png_at_public_boundary() {
    use jade_pdf2docx_parser::{ImageReplacementReason, QualityIssue};
    let mut objects = page_attribute_pdf::pages(
        "/MediaBox [0 0 100 100] /Resources << /XObject << /Im 7 0 R /Alt 10 0 R >> >>",
    );
    for (page, count) in [(0, 9), (1, 5)] {
        let content = (0..count)
            .map(|index| {
                let name = if index % 2 == 0 { "Im" } else { "Alt" };
                format!("q 10 0 0 20 {} 0 cm /{name} Do Q\n", index * 3)
            })
            .collect::<String>();
        objects[4 + page] = page_attribute_pdf::stream(&content);
    }
    objects[6] = "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream".into();
    objects.push("<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream".into());
    let bytes = page_attribute_pdf::pdf(&objects);
    let complete =
        parse_pdf_bytes_with_limits("image-storage.pdf", &bytes, &ParseLimits::default()).unwrap();
    assert_eq!(complete.image_elements.len(), 14);
    assert_eq!(
        complete
            .image_elements
            .iter()
            .filter(|image| image.page == 0)
            .count(),
        9
    );
    assert_eq!(
        complete
            .image_elements
            .iter()
            .filter(|image| image.page == 1)
            .count(),
        5
    );
    assert!(
        complete
            .image_elements
            .iter()
            .all(|image| image.width_px == 13
                && image.height_px == 27
                && image.png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
    );
    assert_eq!(
        complete.quality.issues,
        [
            QualityIssue::ImageReplaced {
                object_number: Some(7),
                reason: ImageReplacementReason::UnsupportedFilter
            },
            QualityIssue::ImageReplaced {
                object_number: Some(10),
                reason: ImageReplacementReason::UnsupportedFilter
            },
            QualityIssue::OcrRequired { page: 0 },
            QualityIssue::OcrRequired { page: 1 },
        ]
    );
    let capacity = minimum_metadata_capacity(&bytes);
    for _ in 0..2 {
        let parsed = parse_pdf_bytes_with_limits(
            "image-storage.pdf",
            &bytes,
            &ParseLimits {
                max_total_metadata_workspace_bytes: capacity,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(parsed.image_elements, complete.image_elements);
        assert_eq!(parsed.quality, complete.quality);
        assert!(
            matches!(parse_pdf_bytes_with_limits("image-storage.pdf", &bytes, &ParseLimits { max_total_metadata_workspace_bytes: capacity - 1, ..Default::default() }), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == capacity)
        );
    }
}

/// ContentsをまたぐCTM復元と別ページの共有画像を、公開APIの容量境界で保持する。
#[test]
fn image_draw_state_and_shared_objects_survive_public_capacity_boundary() {
    let mut objects = page_attribute_pdf::pages(
        "/MediaBox [0 0 100 100] /Resources << /XObject << /Im 7 0 R /Alt 10 0 R >> >>",
    );
    objects[2] = "<< /Type /Page /Parent 2 0 R /Contents [5 0 R 11 0 R] >>".into();
    objects[4] = page_attribute_pdf::stream("2 0 0 3 0 0 cm q 4 0 0 5 7 11 cm /#49m% comment\n");
    objects[5] = page_attribute_pdf::stream("10 0 0 10 0 0 cm /Im Do");
    objects.push(objects[6].clone());
    objects.push(page_attribute_pdf::stream("Do Q /Alt Do"));
    let bytes = page_attribute_pdf::pdf(&objects);
    let baseline =
        parse_pdf_bytes_with_limits("image-draws.pdf", &bytes, &ParseLimits::default()).unwrap();
    assert_eq!(baseline.image_elements.len(), 3);
    let dimensions = baseline
        .image_elements
        .iter()
        .map(|image| (image.page, image.width_px, image.height_px))
        .collect::<Vec<_>>();
    assert_eq!(dimensions, [(0, 11, 20), (1, 13, 13), (0, 3, 4)]);
    assert!(
        baseline
            .image_elements
            .iter()
            .all(|image| image.png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
    );
    let capacity = minimum_metadata_capacity(&bytes);
    let parsed = parse_pdf_bytes_with_limits(
        "image-draws.pdf",
        &bytes,
        &ParseLimits {
            max_total_metadata_workspace_bytes: capacity,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(parsed.image_elements, baseline.image_elements);
    assert_eq!(parsed.quality, baseline.quality);
    assert!(
        matches!(parse_pdf_bytes_with_limits("image-draws.pdf", &bytes, &ParseLimits { max_total_metadata_workspace_bytes: capacity - 1, ..Default::default() }), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == capacity)
    );
}
