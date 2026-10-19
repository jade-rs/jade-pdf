//! 画像Do走査の字句・CTM・共有画像判定と作業容量の回帰。
use super::*;
use std::mem::size_of;

/// metadataだけを制限する予算を作る。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    })
}

/// 各ページに順序付きContentsと二つの共有XObjectを用意する。
fn fixture(pages: &[Vec<&[u8]>]) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    for (page, contents) in pages.iter().enumerate() {
        let number = page * 20 + 1;
        let refs = (0..contents.len())
            .map(|index| format!("{} 0 R ", number + index + 1))
            .collect::<String>();
        bytes.extend_from_slice(format!("{number} 0 obj << /Type /Page /MediaBox [0 0 100 100] /Resources << /XObject << /Im 100 0 R /Other 101 0 R >> >> /Contents [{refs}] >> endobj\n").as_bytes());
        for (index, content) in contents.iter().enumerate() {
            bytes.extend_from_slice(
                format!(
                    "{} 0 obj << /Length {} >>\nstream\n",
                    number + index + 1,
                    content.len()
                )
                .as_bytes(),
            );
            bytes.extend_from_slice(content);
            bytes.extend_from_slice(b"\nendstream\nendobj\n");
        }
    }
    for number in [100, 101] {
        bytes.extend_from_slice(format!("{number} 0 obj << /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream\nendobj\n").as_bytes());
    }
    bytes.extend_from_slice(b"%%EOF");
    bytes
}

/// 画像走査以外の参照・本文抽出の要求容量を求める。
fn preparation(bytes: &[u8]) -> u64 {
    let mut capacity = 0;
    for _ in 0..128 {
        let target = budget(capacity);
        let _ = parse_page_xobject_refs_with_page(&target, bytes);
        let _ = extract_page_content_streams_with_page(&target, bytes);
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

/// cmは現在のCTMへ連結し、単独の上書きにしない。
#[test]
fn image_ctm_concatenates_before_drawing() {
    let bytes = fixture(&[vec![b"2 0 0 3 100 200 cm 4 0 0 5 7 11 cm /Im Do"]]);
    let placements = extract_image_placements_from_pdf_bytes(&budget(u64::MAX), &bytes);
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0].matrix, [8.0, 0.0, 0.0, 15.0, 114.0, 233.0]);
}

/// Q後の描画にはq前のCTMを復元する。
#[test]
fn image_q_restores_parent_matrix() {
    let bytes = fixture(&[vec![
        b"2 0 0 3 4 5 cm q 7 0 0 11 13 17 cm /Im Do Q /Other Do",
    ]]);
    let placements = extract_image_placements_from_pdf_bytes(&budget(u64::MAX), &bytes);
    assert_eq!(placements.len(), 2);
    assert_eq!(placements[1].matrix, [2.0, 0.0, 0.0, 3.0, 4.0, 5.0]);
}

/// Contents境界をまたぐname/DoとCTM/q保存状態を同じページで保持する。
#[test]
fn image_operands_and_matrix_span_contents() {
    let bytes = fixture(&[vec![b"10 0 0 20 5 6 cm q /Im", b"Do Q /Other Do"]]);
    let placements = extract_image_placements_from_pdf_bytes(&budget(u64::MAX), &bytes);
    assert_eq!(placements.len(), 2);
    assert!(
        placements
            .iter()
            .all(|placement| placement.matrix == [10.0, 0.0, 0.0, 20.0, 5.0, 6.0])
    );
}

/// コメント・literal/hex文字列・配列・辞書中のDoを描画しない。
#[test]
fn image_lexer_ignores_non_operator_do() {
    for content in [
        b"% /Im Do\n".as_slice(),
        b"( /Im Do ) Tj",
        b"[ /Im Do ]",
        b"<< /Fake /Im Do >>",
        b"<2f496d20446f> Tj",
    ] {
        let bytes = fixture(&[vec![content]]);
        assert!(
            extract_image_placements_from_pdf_bytes(&budget(u64::MAX), &bytes).is_empty(),
            "{content:?}"
        );
    }
}

/// inline image以降はbinary内の偽Doと後続の不確定な境界を描画しない。
#[test]
fn inline_image_stops_only_its_page() {
    let bytes = fixture(&[
        vec![b"/Im Do BI /W 1 /H 1 ID /Other Do EI /Im Do"],
        vec![b"/Other Do"],
    ]);
    let placements = extract_image_placements_from_pdf_bytes(&budget(u64::MAX), &bytes);
    assert_eq!(
        placements
            .iter()
            .map(|placement| (placement.page, placement.object_number))
            .collect::<Vec<_>>(),
        [(0, 100), (1, 101)]
    );
}

/// 名前直後のコメントを空白として扱い、escapeを借用照合する。
#[test]
fn image_name_comments_and_escapes_are_lexed() {
    let bytes = fixture(&[vec![b"/#49m% name comment\nDo"]]);
    let placements = extract_image_placements_from_pdf_bytes(&budget(u64::MAX), &bytes);
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0].object_number, 100);
}

/// 別ページで描画された共有objectを一方のページの未描画名から除外しない。
#[test]
fn shared_images_painted_on_another_page_are_retained() {
    let bytes = fixture(&[vec![b"/Other Do"], vec![b"/Im Do"]]);
    let target = budget(u64::MAX);
    let unpainted = unpainted_xobject_image_object_numbers(&target, &bytes);
    target.check().unwrap();
    assert!(!unpainted.contains(&100));
    assert!(!unpainted.contains(&101));
}

/// 未描画判定の集合・ページ状態も参照/本文とは別に容量を予約する。
#[test]
fn unpainted_classification_requires_additional_capacity() {
    let bytes = fixture(&[vec![b"/Other Do"]]);
    let target = budget(preparation(&bytes));
    let unpainted = unpainted_xobject_image_object_numbers(&target, &bytes);
    assert!(unpainted.is_empty());
    assert!(target.check().is_err());
}

/// q/Q保存列にも配置出力とは別の容量が必要。
#[test]
fn image_graphics_stack_requires_additional_capacity() {
    let bytes = fixture(&[vec![b"q /Im Do"]]);
    let target = budget(preparation(&bytes) + (4 * size_of::<ImagePlacement>()) as u64);
    assert!(extract_image_placements_from_pdf_bytes(&target, &bytes).is_empty());
    assert!(target.check().is_err());
}

/// 未描画集合のinclusive境界・再作成累積・停止消費後の空結果を保持する。
#[test]
fn unpainted_capacity_and_recreation_preserve_classification() {
    let bytes = fixture(&[vec![b"/Other Do"]]);
    let mut capacity = 0;
    for _ in 0..128 {
        let target = budget(capacity);
        let _ = unpainted_xobject_image_object_numbers(&target, &bytes);
        match target.check() {
            Ok(()) => break,
            Err(ParseError::ResourceLimit { actual, .. }) if actual > capacity => capacity = actual,
            other => panic!("unexpected capacity {other:?}"),
        }
    }
    assert!(capacity > preparation(&bytes));
    for maximum in [capacity - 1, capacity, 2 * capacity - 1, 2 * capacity] {
        let target = budget(maximum);
        for pass in 1..=2 {
            let result = unpainted_xobject_image_object_numbers(&target, &bytes);
            assert_eq!(result.contains(&100), pass * capacity <= maximum);
            assert!(!result.contains(&101));
            assert_eq!(result.len(), usize::from(pass * capacity <= maximum));
        }
        assert_eq!(target.check().is_ok(), 2 * capacity <= maximum);
        if target.is_stopped() {
            assert!(unpainted_xobject_image_object_numbers(&target, &bytes).is_empty());
        }
    }
}

/// 完成配置の後のq拡張失敗でも配置のprefixを公開しない。
#[test]
fn late_graphics_stack_failure_discards_completed_placements() {
    let bytes = fixture(&[vec![b"/Im Do q q q q q /Other Do"]]);
    let mut capacity = 0;
    for _ in 0..128 {
        let target = budget(capacity);
        let placements = extract_image_placements_from_pdf_bytes(&target, &bytes);
        match target.check() {
            Ok(()) => {
                assert_eq!(placements.len(), 2);
                break;
            }
            Err(ParseError::ResourceLimit { actual, .. }) if actual > capacity => capacity = actual,
            other => panic!("unexpected capacity {other:?}"),
        }
    }
    let target = budget(capacity - 1);
    assert!(extract_image_placements_from_pdf_bytes(&target, &bytes).is_empty());
    assert!(target.check().is_err());
    assert!(extract_image_placements_from_pdf_bytes(&target, &bytes).is_empty());
}
