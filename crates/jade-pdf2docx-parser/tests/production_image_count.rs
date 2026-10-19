//! 画像数だけで画素を失わず、描画対象と明示的な資源上限を守る公開API契約。
#[path = "support/image_count_pdf.rs"]
mod fixture;
use fixture::image_count_pdf;
use jade_pdf2docx_parser::{
    ParseError, ParseLimits, ParseOptions, QualityIssue, parse_pdf_bytes_with_options,
};
use proptest::prelude::*;
use std::io::Read;

/// 小さいfilter 0 PNGから実際の画素を取得する。
fn pixels(png: &[u8]) -> Vec<u8> {
    let mut cursor = 8;
    let mut idat = Vec::new();
    while cursor + 12 <= png.len() {
        let length = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
        if &png[cursor + 4..cursor + 8] == b"IDAT" {
            idat.extend_from_slice(&png[cursor + 8..cursor + 8 + length]);
        }
        cursor += length + 12;
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(idat.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    assert_eq!(raw[0], 0);
    raw[1..].to_vec()
}

/// 各SMaskを単独画像へ抽出せず、可視画像の透明度として保持する。
#[test]
fn auxiliary_masks_are_not_extracted_separately() {
    for visible in [11, 20] {
        let pdf = image_count_pdf(visible, 0, true);
        let parsed = parse_pdf_bytes_with_options(
            "mask-count.pdf",
            &pdf,
            &ParseOptions {
                strict: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            parsed.quality.issues,
            [QualityIssue::OcrRequired { page: 0 }]
        );
        assert_eq!(parsed.image_elements.len(), visible);
        for image in parsed.image_elements {
            assert_eq!(pixels(&image.png_bytes), [10, 20, 30, 0, 40, 50, 60, 77]);
        }
        assert!(parsed.text_preview.contains("retained text"));
    }
}

/// resourceに登録されても描画されない画像を抽出しない。
#[test]
fn unpainted_resources_are_not_extracted() {
    let pdf = image_count_pdf(20, 21, false);
    let parsed = parse_pdf_bytes_with_options(
        "unused-count.pdf",
        &pdf,
        &ParseOptions {
            strict: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        parsed.quality.issues,
        [QualityIssue::OcrRequired { page: 0 }]
    );
    assert_eq!(parsed.image_elements.len(), 20);
    for image in parsed.image_elements {
        assert_eq!(pixels(&image.png_bytes), [10, 20, 30, 40, 50, 60]);
    }
}

/// 可視画像が21枚でも補助maskの有無にかかわらず全画素を保持する。
#[test]
fn twenty_one_visible_images_keep_pixels() {
    for masks in [false, true] {
        let pdf = image_count_pdf(21, 0, masks);
        let parsed =
            parse_pdf_bytes_with_options("visible-count.pdf", &pdf, &ParseOptions::default())
                .unwrap();
        assert_eq!(parsed.image_elements.len(), 21);
        assert_eq!(
            parsed.quality.issues,
            [QualityIssue::OcrRequired { page: 0 }]
        );
        for image in &parsed.image_elements {
            let expected = if masks {
                vec![10, 20, 30, 0, 40, 50, 60, 77]
            } else {
                vec![10, 20, 30, 40, 50, 60]
            };
            assert_eq!(pixels(&image.png_bytes), expected);
        }
        match parse_pdf_bytes_with_options(
            "visible-count.pdf",
            &pdf,
            &ParseOptions {
                strict: true,
                ..Default::default()
            },
        ) {
            Err(ParseError::QualityRejected { report, .. }) => assert_eq!(report, parsed.quality),
            other => panic!("expected OCR-only rejection: {other:?}"),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 補助画像数と可視画像数が変化しても全描画画像の画素を保持する。
    #[test]
    fn image_count_matches_visible_object_model(visible in 1usize..=22, unpainted in 0usize..=23, masks in any::<bool>()) {
        let pdf = image_count_pdf(visible, unpainted, masks);
        let parsed = parse_pdf_bytes_with_options("model-count.pdf", &pdf, &ParseOptions::default()).unwrap();
        prop_assert_eq!(parsed.image_elements.len(), visible);
        prop_assert!(parsed.text_preview.contains("retained text"));
        prop_assert_eq!(&parsed.quality.issues, &vec![QualityIssue::OcrRequired { page: 0 }]);
        for image in &parsed.image_elements {
            let expected = if masks { vec![10, 20, 30, 0, 40, 50, 60, 77] } else { vec![10, 20, 30, 40, 50, 60] };
            prop_assert_eq!(pixels(&image.png_bytes), expected);
        }
    }
}

/// 21枚以上でも画像保存上限の超過はstrictの有無によらず資源エラーになる。
#[test]
fn many_images_still_enforce_storage_limit() {
    let pdf = image_count_pdf(21, 0, true);
    for strict in [false, true] {
        let options = ParseOptions {
            strict,
            limits: ParseLimits {
                max_total_image_storage_bytes: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(matches!(
            parse_pdf_bytes_with_options("bounded.pdf", &pdf, &options),
            Err(ParseError::ResourceLimit {
                resource: "image storage bytes",
                ..
            })
        ));
    }
}
