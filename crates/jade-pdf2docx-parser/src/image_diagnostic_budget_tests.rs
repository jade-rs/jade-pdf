//! 画像置換とOCR候補がmetadata予算を迂回する回帰。
use super::*;
use std::{collections::BTreeSet, mem::size_of};

/// 未対応filterの共有画像を、指定ページ数から描画する入力を作る。
fn fixture(pages: usize) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let content = "1 0 0 1 0 0 cm /Im Do";
    for page in 0..pages {
        let number = page * 2 + 1;
        let next = number + 1;
        bytes.extend_from_slice(format!("{number} 0 obj << /Type /Page /MediaBox [0 0 100 100] /Resources << /XObject << /Im 100 0 R >> >> /Contents {next} 0 R >> endobj\n{next} 0 obj << /Length {} >>\nstream\n{content}\nendstream\nendobj\n", content.len()).as_bytes());
    }
    bytes.extend_from_slice(b"100 0 obj << /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream\nendobj\n%%EOF");
    bytes
}

/// 独立して用意した検査済み画像streamを返す。
fn streams() -> [crate::structure::StreamSpan<'static>; 1] {
    [crate::structure::StreamSpan {
        dictionary:
            b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>",
        payload: b"x",
        object_offset: None,
        object_number: Some(100),
    }]
}

/// metadataだけを制限する共有予算を作る。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    })
}

/// 診断以外の既存抽出準備が要求する累積容量を求める。
fn preparation(bytes: &[u8]) -> u64 {
    let mut capacity = 0;
    for _ in 0..128 {
        let target = budget(capacity);
        let _ = extract_image_placements_from_pdf_bytes(&target, bytes);
        let _ = unpainted_xobject_image_object_numbers(&target, bytes);
        match target.check() {
            Ok(()) => return capacity,
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                actual,
                ..
            }) if actual > capacity => capacity = actual,
            other => panic!("unexpected preparation {other:?}"),
        }
    }
    panic!("preparation did not converge");
}

/// 完全予算での診断が存在し、指定容量不足では画像結果を公開しないことを固定する。
fn rejected(pages: usize, extra: usize) -> QualityReport {
    let bytes = fixture(pages);
    let mut complete = QualityReport::default();
    let target = budget(u64::MAX);
    assert_eq!(
        extract_image_elements_from_pdf_bytes(&target, &bytes, &streams(), &mut complete).len(),
        pages
    );
    target.check().unwrap();
    assert_eq!(complete.issues.len(), pages + 1);
    let target = budget(preparation(&bytes) + extra as u64);
    let mut report = QualityReport::default();
    let images = extract_image_elements_from_pdf_bytes(&target, &bytes, &streams(), &mut report);
    assert!(images.is_empty(), "予算外の画像結果を公開しない");
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
    report
}

/// 描画済みobject集合の初回slotを予算外に確保しない。
#[test]
fn placed_object_set_requires_capacity() {
    assert!(rejected(1, 0).issues.is_empty());
}

/// 画像置換の出力slotを保存前に予約する。
#[test]
fn replacement_output_requires_capacity() {
    assert!(rejected(1, 4 * size_of::<Option<u32>>()).issues.is_empty());
}

/// 置換診断が保存済みでもOCR候補ページの保存には別容量が必要。
#[test]
fn painted_pages_require_capacity() {
    let report = rejected(
        1,
        4 * (size_of::<Option<u32>>()
            + size_of::<QualityIssue>()
            + size_of::<ImageStreamEntry>()
            + size_of::<ParsedImageElement>()),
    );
    assert_eq!(
        report.issues,
        [QualityIssue::ImageReplaced {
            object_number: Some(100),
            reason: ImageReplacementReason::UnsupportedFilter
        }]
    );
}

/// 置換と三ページ分の診断の後も、OCR出力の全拡張slotを予約する。
#[test]
fn ocr_output_growth_requires_capacity() {
    let report = rejected(
        4,
        4 * (size_of::<Option<u32>>()
            + size_of::<QualityIssue>()
            + size_of::<usize>()
            + size_of::<ImageStreamEntry>()
            + size_of::<ParsedImageElement>()),
    );
    assert_eq!(report.issues.len(), 4);
    assert_eq!(
        &report.issues[1..],
        &[
            QualityIssue::OcrRequired { page: 0 },
            QualityIssue::OcrRequired { page: 1 },
            QualityIssue::OcrRequired { page: 2 }
        ]
    );
}

/// 完成診断の出力容量を含め、境界ちょうどの予約を受け入れる。
#[test]
fn inclusive_capacity_preserves_shared_image_diagnostics() {
    for pages in [1, 2, 4, 5, 9] {
        let bytes = fixture(pages);
        let mut capacity = preparation(&bytes);
        let complete = loop {
            let target = budget(capacity);
            let mut report = QualityReport::default();
            let images =
                extract_image_elements_from_pdf_bytes(&target, &bytes, &streams(), &mut report);
            match target.check() {
                Ok(()) => {
                    assert_eq!(images.len(), pages);
                    break report;
                }
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    actual,
                    ..
                }) if actual > capacity => capacity = actual,
                other => panic!("unexpected image budget {other:?}"),
            }
        };
        let target = budget(capacity - 1);
        let mut report = QualityReport::default();
        assert!(
            extract_image_elements_from_pdf_bytes(&target, &bytes, &streams(), &mut report)
                .is_empty()
        );
        assert!(
            matches!(target.check(), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == capacity)
        );
        assert_eq!(
            complete.issues[0],
            QualityIssue::ImageReplaced {
                object_number: Some(100),
                reason: ImageReplacementReason::UnsupportedFilter
            }
        );
        assert_eq!(
            complete.issues[1..],
            (0..pages)
                .map(|page| QualityIssue::OcrRequired { page })
                .collect::<Vec<_>>()
        );
    }
}

/// 五ページ目のOCR候補は拡張する全slotの容量を必要とする。
#[test]
fn painted_page_growth_does_not_publish_partial_images() {
    let report = rejected(
        5,
        4 * (size_of::<Option<u32>>()
            + size_of::<QualityIssue>()
            + size_of::<usize>()
            + size_of::<ImageStreamEntry>())
            + 12 * size_of::<ParsedImageElement>(),
    );
    assert_eq!(report.issues.len(), 1);
}

/// 既存の診断slotは再利用し、同じ抽出をやり直す作業容量は累積する。
#[test]
fn report_slots_are_reused_but_preparation_is_cumulative() {
    let bytes = fixture(1);
    let cost = preparation(&bytes)
        + (4 * (size_of::<Option<u32>>()
            + size_of::<usize>()
            + size_of::<ImageStreamEntry>()
            + size_of::<ParsedImageElement>())) as u64;
    let target = budget(cost * 2);
    let mut report = QualityReport {
        issues: Vec::with_capacity(8),
    };
    for _ in 0..2 {
        assert_eq!(
            extract_image_elements_from_pdf_bytes(&target, &bytes, &streams(), &mut report).len(),
            1
        );
        target.check().unwrap();
    }
    let completed = report.clone();
    assert!(
        extract_image_elements_from_pdf_bytes(&target, &bytes, &streams(), &mut report).is_empty()
    );
    assert!(target.check().is_err());
    assert_eq!(report, completed);
}

/// 停止エラーを取り出した後も診断配列の空slotへ書き込まない。
#[test]
fn consumed_stop_preserves_existing_image_diagnostics() {
    let target = budget(0);
    assert!(target.metadata_vector::<u8>(1).is_none());
    assert!(target.check().is_err());
    let mut report = QualityReport {
        issues: vec![QualityIssue::UnsupportedInlineImage { page: 8 }],
    };
    let original = report.clone();
    assert!(
        extract_image_elements_from_pdf_bytes(&target, &fixture(1), &streams(), &mut report)
            .is_empty()
    );
    assert!(target.is_stopped());
    assert_eq!(report, original);
}

/// 各ページの参照先を指定し、画像objectを一度ずつ保存する入力を作る。
fn separate_images_fixture(numbers: &[u32]) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    for (page, image) in numbers.iter().enumerate() {
        let number = page * 2 + 1;
        let next = number + 1;
        bytes.extend_from_slice(format!("{number} 0 obj << /Type /Page /MediaBox [0 0 100 100] /Resources << /XObject << /Im {image} 0 R >> >> /Contents {next} 0 R >> endobj\n{next} 0 obj << /Length 6 >>\nstream\n/Im Do\nendstream\nendobj\n").as_bytes());
    }
    for number in numbers.iter().copied().collect::<BTreeSet<_>>() {
        bytes.extend_from_slice(format!("{number} 0 obj << /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream\nendobj\n").as_bytes());
    }
    bytes.extend_from_slice(b"%%EOF");
    bytes
}

/// 三つ目の異なるobjectでは集合の全拡張slotを予約する。
#[test]
fn placed_object_growth_is_reserved_before_any_diagnostics() {
    let bytes = separate_images_fixture(&[100, 101, 102]);
    let streams = (100..103)
        .map(|number| crate::structure::StreamSpan {
            object_number: Some(number),
            ..streams()[0]
        })
        .collect::<Vec<_>>();
    let mut report = QualityReport::default();
    let unlimited = budget(u64::MAX);
    assert_eq!(
        extract_image_elements_from_pdf_bytes(&unlimited, &bytes, &streams, &mut report).len(),
        3
    );
    unlimited.check().unwrap();
    let target = budget(preparation(&bytes) + (4 * size_of::<Option<u32>>()) as u64);
    report.issues.clear();
    assert!(
        extract_image_elements_from_pdf_bytes(&target, &bytes, &streams, &mut report).is_empty()
    );
    assert!(target.check().is_err());
    assert!(report.issues.is_empty());
}

/// 画像objectの順序とページ順が異なっても、OCR診断はページ順に一度だけ並べる。
#[test]
fn painted_pages_preserve_order_and_deduplicate_across_objects() {
    let bytes = separate_images_fixture(&[101, 100, 100]);
    let streams = (100..102)
        .map(|number| crate::structure::StreamSpan {
            object_number: Some(number),
            ..streams()[0]
        })
        .collect::<Vec<_>>();
    let target = budget(u64::MAX);
    let mut report = QualityReport::default();
    let images = extract_image_elements_from_pdf_bytes(&target, &bytes, &streams, &mut report);
    target.check().unwrap();
    assert_eq!(
        images.iter().map(|image| image.page).collect::<Vec<_>>(),
        [1, 2, 0]
    );
    assert_eq!(
        report.issues[2..],
        [
            QualityIssue::OcrRequired { page: 0 },
            QualityIssue::OcrRequired { page: 1 },
            QualityIssue::OcrRequired { page: 2 }
        ]
    );
}

/// 複数の未対応画像とdecode失敗の置換分岐でも出力slotを予約する。
#[test]
fn all_replacement_branches_reserve_output_slots() {
    let bytes = fixture(1);
    let source = &streams()[0];
    for (dictionary, count, reason) in [(source.dictionary, 21, ImageReplacementReason::UnsupportedFilter), (b"<< /Subtype /Image /Width 1 /Height 1 >>".as_slice(), 1, ImageReplacementReason::UnsupportedFilter), (b"<< /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode >>".as_slice(), 1, ImageReplacementReason::DecodeFailed)] {
        let streams = (0..count).map(|_| crate::structure::StreamSpan { dictionary, ..*source }).collect::<Vec<_>>();
        let mut report = QualityReport::default();
        let unlimited = budget(u64::MAX);
        let images = extract_image_elements_from_pdf_bytes(&unlimited, &bytes, &streams, &mut report);
        unlimited.check().unwrap();
        assert_eq!(images.len(), count);
        assert_eq!(report.issues[0], QualityIssue::ImageReplaced { object_number: Some(100), reason });
        let capacity = preparation(&bytes) + (4 * size_of::<Option<u32>>()) as u64;
        let target = budget(capacity);
        let mut report = QualityReport::default();
        assert!(extract_image_elements_from_pdf_bytes(&target, &bytes, &streams, &mut report).is_empty());
        assert!(matches!(target.check(), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == capacity + (4 * size_of::<QualityIssue>()) as u64));
        assert!(report.issues.is_empty());
    }
}
