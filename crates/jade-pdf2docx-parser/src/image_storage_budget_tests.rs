//! 画像保存・配置・最終結果配列の累積要求容量と部分結果の回帰。
use super::*;
use proptest::prelude::*;
use std::mem::size_of;

/// metadataだけを制限する予算を作る。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    })
}

/// 描画のない互換入力を返す。
fn empty_pdf() -> &'static [u8] {
    b"%PDF-1.7\n%%EOF"
}

/// 未対応画像streamを指定数用意する。
fn streams(count: usize) -> Vec<crate::structure::StreamSpan<'static>> {
    (0..count)
        .map(|index| crate::structure::StreamSpan {
            dictionary:
                b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode >>",
            payload: b"x",
            object_offset: None,
            object_number: Some(index as u32 + 100),
        })
        .collect()
}

/// 画像配置を指定回数描くページを作る。
fn fixture(count: usize) -> Vec<u8> {
    let content = "1 0 0 1 0 0 cm /Im Do ".repeat(count);
    format!("%PDF-1.7\n1 0 obj << /Type /Page /MediaBox [0 0 100 100] /Resources << /XObject << /Im 100 0 R >> >> /Contents 2 0 R >> endobj\n2 0 obj << /Length {} >>\nstream\n{content}\nendstream\nendobj\n100 0 obj << /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream\nendobj\n%%EOF", content.len()).into_bytes()
}

/// 配置配列以外の参照・本文抽出が要求する容量を求める。
fn preparation(bytes: &[u8]) -> u64 {
    minimum(|target| {
        let source = parser_source_with_compressed_objects(target, bytes);
        let _ = page_xobject_refs_from_source(target, &source);
        let contents = extract_page_content_streams_with_page(target, bytes);
        let _ = image_draws::visit_in_source(target, &source, &contents, |_, _, _, _| Some(()));
    })
}

/// 共有予算の要求容量を、停止を新しい予算へ引き継がず求める。
fn minimum(mut run: impl FnMut(&DecodeBudget)) -> u64 {
    let mut capacity = 0;
    for _ in 0..128 {
        let target = budget(capacity);
        run(&target);
        match target.check() {
            Ok(()) => return capacity,
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                actual,
                ..
            }) if actual > capacity => capacity = actual,
            other => panic!("unexpected capacity {other:?}"),
        }
    }
    panic!("capacity did not converge");
}

/// 画像本体配列以外の空入力処理の容量を求める。
fn image_preparation() -> u64 {
    minimum(|target| {
        let _ = extract_image_placements_from_pdf_bytes(target, empty_pdf());
        let _ = unpainted_xobject_image_object_numbers(target, empty_pdf());
    })
}

/// 初回4slotと拡張時の全slotの累積容量を独立に計算する。
fn slots(count: usize) -> usize {
    let mut total = 0;
    let mut capacity = 0;
    while capacity < count {
        capacity = (capacity * 2).max(4);
        total += capacity;
    }
    total
}

/// 既存の診断容量を再利用して画像配列だけを検査する。
fn extract(target: &DecodeBudget, count: usize) -> Vec<ParsedImageElement> {
    let mut report = QualityReport {
        issues: Vec::with_capacity(count),
    };
    extract_image_elements_from_pdf_bytes(target, empty_pdf(), &streams(count), &mut report)
}

/// 配置の初回slotを容量外に確保しない。
#[test]
fn placement_initial_slots_require_capacity() {
    let bytes = fixture(1);
    let target = budget(preparation(&bytes));
    assert!(extract_image_placements_from_pdf_bytes(&target, &bytes).is_empty());
    assert!(target.check().is_err());
}

/// 配置の拡張容量不足で先頭四件を公開しない。
#[test]
fn placement_growth_discards_partial_results() {
    let bytes = fixture(5);
    let target = budget(preparation(&bytes) + (4 * size_of::<ImagePlacement>()) as u64);
    assert!(extract_image_placements_from_pdf_bytes(&target, &bytes).is_empty());
    assert!(target.check().is_err());
}

/// 画像stream保存の初回slotを容量外に確保しない。
#[test]
fn image_stream_initial_slots_require_capacity() {
    let target = budget(image_preparation());
    assert!(extract(&target, 1).is_empty());
    assert!(target.check().is_err());
}

/// 五件目の画像streamで全拡張容量を予約する。
#[test]
fn image_stream_growth_discards_partial_results() {
    let target = budget(image_preparation() + (4 * size_of::<ImageStreamEntry>()) as u64);
    assert!(extract(&target, 5).is_empty());
    assert!(target.check().is_err());
}

/// 最終画像の初回slotにも別容量が必要。
#[test]
fn image_result_initial_slots_require_capacity() {
    let target = budget(image_preparation() + (4 * size_of::<ImageStreamEntry>()) as u64);
    assert!(extract(&target, 1).is_empty());
    assert!(target.check().is_err());
}

/// 最終画像の五件目で先頭四件を公開しない。
#[test]
fn image_result_growth_discards_partial_results() {
    let capacity = 12 * size_of::<ImageStreamEntry>() + 4 * size_of::<ParsedImageElement>();
    let target = budget(image_preparation() + capacity as u64);
    assert!(extract(&target, 5).is_empty());
    assert!(target.check().is_err());
}

/// 解放後の同じ抽出も共有予算を再消費し、消費済み停止を再開しない。
#[test]
fn recreated_arrays_are_cumulative_and_keep_consumed_stop() {
    let cost = image_preparation()
        + (4 * (size_of::<ImageStreamEntry>() + size_of::<ParsedImageElement>())) as u64;
    let target = budget(cost * 2);
    for _ in 0..2 {
        assert_eq!(extract(&target, 1).len(), 1);
        target.check().unwrap();
    }
    assert!(extract(&target, 1).is_empty());
    assert!(target.check().is_err());
    assert!(extract(&target, 1).is_empty());
    assert!(target.is_stopped());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 保存・最終配列の独立容量モデルと、画像識別・寸法・PNGの境界を照合する。
    #[test]
    fn image_array_capacity_matches_model(count in 1usize..18) {
        let capacity = image_preparation() + (slots(count) * (size_of::<ImageStreamEntry>() + size_of::<ParsedImageElement>())) as u64;
        let target = budget(capacity);
        let images = extract(&target, count);
        prop_assert_eq!(images.len(), count);
        target.check().unwrap();
        for image in images {
            prop_assert_eq!(image.page, 0);
            prop_assert_eq!((image.width_px, image.height_px), (1, 1));
            prop_assert!(image.png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
        let target = budget(capacity - 1);
        prop_assert!(extract(&target, count).is_empty());
        prop_assert!(matches!(target.check(), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == capacity), "全要求容量を報告する");
    }
}

/// 配置の独立容量モデルの境界で順序とCTMを保持する。
#[test]
fn placement_inclusive_capacity_preserves_geometry() {
    for count in [1, 4, 5, 9, 17] {
        let bytes = fixture(count);
        let capacity = preparation(&bytes) + (slots(count) * size_of::<ImagePlacement>()) as u64;
        let target = budget(capacity);
        let placements = extract_image_placements_from_pdf_bytes(&target, &bytes);
        assert_eq!(placements.len(), count);
        target.check().unwrap();
        assert!(
            placements
                .iter()
                .all(|placement| placement.object_number == 100
                    && placement.page == 0
                    && placement.matrix == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
        );
        let target = budget(capacity - 1);
        assert!(extract_image_placements_from_pdf_bytes(&target, &bytes).is_empty());
        assert!(
            matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == capacity)
        );
    }
}

/// 配置の解放後も累積予約し、停止エラー消費後の空入力でも再開しない。
#[test]
fn recreated_placements_are_cumulative_and_keep_consumed_stop() {
    let bytes = fixture(5);
    let cost = preparation(&bytes) + (slots(5) * size_of::<ImagePlacement>()) as u64;
    let target = budget(cost * 2);
    for _ in 0..2 {
        assert_eq!(
            extract_image_placements_from_pdf_bytes(&target, &bytes).len(),
            5
        );
        target.check().unwrap();
    }
    assert!(extract_image_placements_from_pdf_bytes(&target, &bytes).is_empty());
    assert!(target.check().is_err());
    assert!(extract_image_placements_from_pdf_bytes(&target, empty_pdf()).is_empty());
    assert!(target.is_stopped());
}

/// stream保存の途中で容量不足になっても、保存済み置換診断を保持する。
#[test]
fn failed_storage_preserves_completed_diagnostics() {
    let capacity = image_preparation() + (4 * size_of::<ImageStreamEntry>()) as u64;
    let target = budget(capacity);
    let existing = QualityIssue::UnsupportedInlineImage { page: 8 };
    let mut report = QualityReport {
        issues: vec![existing.clone()],
    };
    report.issues.reserve_exact(7);
    assert!(
        extract_image_elements_from_pdf_bytes(&target, empty_pdf(), &streams(5), &mut report)
            .is_empty()
    );
    assert!(target.check().is_err());
    assert_eq!(report.issues[0], existing);
    assert_eq!(report.issues.len(), 6);
    for (index, issue) in report.issues[1..].iter().enumerate() {
        assert_eq!(
            *issue,
            QualityIssue::ImageReplaced {
                object_number: Some(100 + index as u32),
                reason: ImageReplacementReason::UnsupportedFilter
            }
        );
    }
    let completed = report.clone();
    assert!(
        extract_image_elements_from_pdf_bytes(&target, empty_pdf(), &streams(5), &mut report)
            .is_empty()
    );
    assert_eq!(report, completed);
}
