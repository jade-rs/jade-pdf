//! nativeページ保存の確保前容量検査と合成結果の意味保持を確認する。
use super::*;
use crate::ParseLimits;
use proptest::prelude::*;

/// 指定容量を持つ独立したmetadata予算を作る。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    })
}

/// 順序と回転前寸法を識別できるページ情報を作る。
fn page(number: usize, rotation: i32) -> ParsedPage {
    ParsedPage {
        page_number: number,
        width: 100. + number as f32,
        height: 200. + number as f32,
        rotation,
    }
}

/// 保存配列の全slotを上限ちょうどで予約し、1 byte不足を型付きエラーにする。
#[test]
fn storage_reserves_exact_slots_before_allocation() {
    let count = 3;
    let bytes = (count * std::mem::size_of::<ParsedPage>()) as u64;
    assert!(reserve(&budget(bytes), count).unwrap().capacity() >= count);
    assert!(matches!(
        reserve(&budget(bytes - 1), count),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            limit,
            actual,
        }) if limit == bytes - 1 && actual == bytes
    ));
}

/// 解放して再作成するページ保存配列も同じ累積容量へ数える。
#[test]
fn storage_counts_recreation() {
    let bytes = (2 * std::mem::size_of::<ParsedPage>()) as u64;
    let target = budget(bytes * 2 - 1);
    drop(reserve(&target, 2).unwrap());
    assert!(matches!(
        reserve(&target, 2),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            actual,
            ..
        }) if actual == bytes * 2
    ));
}

/// 空入力は容量0で動作するが、消費済み停止後は保存・合成を再開しない。
#[test]
fn empty_storage_preserves_consumed_stop() {
    assert!(reserve(&budget(0), 0).unwrap().is_empty());
    let stopped = budget(0);
    stopped.charge_metadata_workspace_bytes(1);
    assert!(stopped.check().is_err());
    assert!(reserve(&stopped, 0).is_err());
}

/// 合成は追加配列を作らず予約済みnative配列と容量を保持する。
#[test]
fn merge_reuses_storage_and_preserves_geometry() {
    let target = budget((4 * std::mem::size_of::<ParsedPage>()) as u64);
    let mut native = reserve(&target, 4).unwrap();
    native.extend([page(2, 90), page(0, 270), page(1, -90)]);
    let pointer = native.as_ptr();
    let capacity = native.capacity();
    let pure = [page(1, 180), page(0, 0), page(1, 0)];
    let result = merge(&target, &pure, native).unwrap();
    assert_eq!(result.as_ptr(), pointer);
    assert_eq!(result.capacity(), capacity);
    let mut missing = page(2, 90);
    std::mem::swap(&mut missing.width, &mut missing.height);
    assert_eq!(result, [missing, pure[1].clone(), pure[0].clone()]);
    target.check().unwrap();
}

/// 昇順のpure情報が疎でも、ordinal参照と二分探索で欠損を区別する。
#[test]
fn merge_preserves_sparse_sorted_page_numbers() {
    let pure = [page(0, 180), page(3, 180), page(usize::MAX, 180)];
    let native = vec![
        page(usize::MAX, 90),
        page(3, 90),
        page(0, 90),
        page(1, -270),
    ];
    let mut missing = page(1, -270);
    std::mem::swap(&mut missing.width, &mut missing.height);
    assert_eq!(
        merge(&budget(0), &pure, native).unwrap(),
        [pure[2].clone(), pure[1].clone(), pure[0].clone(), missing]
    );
}

/// 停止後の合成は空のpure/native情報にも成功を返さない。
#[test]
fn merge_preserves_consumed_stop() {
    assert!(merge(&budget(0), &[], Vec::new()).unwrap().is_empty());
    let stopped = budget(0);
    stopped.charge_metadata_workspace_bytes(1);
    assert!(stopped.check().is_err());
    assert!(merge(&stopped, &[], Vec::new()).is_err());
    assert!(merge(&stopped, &[], vec![page(0, 90)]).is_err());
}

/// file公開APIでnativeページ保存の超過を文字抽出より先に返す。
#[test]
fn file_api_reports_page_storage_before_character_storage() {
    use crate::page_attribute_fixture::{pdf, stream};
    use std::io::Write;
    let bytes = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>".into(),
        stream("BT /F 12 Tf 10 80 Td (abc) Tj ET"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ]);
    let limits = |capacity| ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    };
    let mut low = 0;
    let mut high = 65536;
    assert!(crate::parse_pdf_bytes_with_limits("native-pages.pdf", &bytes, &limits(high)).is_ok());
    while low < high {
        let middle = low + (high - low) / 2;
        match crate::parse_pdf_bytes_with_limits("native-pages.pdf", &bytes, &limits(middle)) {
            Ok(_) => high = middle,
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                ..
            }) => low = middle + 1,
            result => panic!("unexpected pure parser result: {result:?}"),
        }
    }
    let native_slots = std::mem::size_of::<ParsedPage>() as u64;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-native-pages-{}-{nonce}.pdf",
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    file.write_all(&bytes).unwrap();
    drop(file);
    let result = crate::parse_pdf_with_limits(&path, &limits(low));
    std::fs::remove_file(&path).unwrap();
    assert!(
        matches!(result, Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit, actual,
    }) if limit == low && actual == low + native_slots),
        "unexpected native result: {result:?}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 欠損・重複・順序違い・符号付き回転でも旧合成の初出優先とnative順序を保持する。
    #[test]
    fn merge_matches_legacy_page_semantics(
        pure_numbers in prop::collection::vec(0usize..20, 0..30),
        native_numbers in prop::collection::vec(0usize..20, 0..30),
        quarter_turn in -8i32..9,
    ) {
        let pure: Vec<_> = pure_numbers.into_iter().map(|n| page(n, 180)).collect();
        let target = budget((native_numbers.len() * std::mem::size_of::<ParsedPage>()) as u64);
        let mut native = reserve(&target, native_numbers.len()).unwrap();
        native.extend(native_numbers.into_iter().map(|n| page(n, quarter_turn * 90)));
        let expected: Vec<_> = native.iter().map(|n| {
            pure.iter().find(|p| p.page_number == n.page_number).cloned().unwrap_or_else(|| {
                let mut result = n.clone();
                if result.rotation.rem_euclid(180) == 90 {
                    std::mem::swap(&mut result.width, &mut result.height);
                }
                result
            })
        }).collect();
        prop_assert_eq!(merge(&target, &pure, native).unwrap(), expected);
        prop_assert!(target.check().is_ok());
    }
}
