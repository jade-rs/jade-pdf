//! 罫線の作業配列・結果容量と字句走査の回帰を固定する。
use super::*;
use proptest::prelude::*;

/// 指定したmetadata容量で罫線を抽出する。
fn extract(limit: u64, source: &[u8]) -> (DecodeBudget, Vec<ParsedGraphicElement>) {
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    });
    let result = graphics_with_budget(&budget, 0, source);
    (budget, result)
}

/// pathと結果の初回全slotをinclusiveに予約する。
#[test]
fn graphics_path_and_results_require_full_capacity() {
    let total = 4 * (size_of::<ParsedBounds>() + size_of::<ParsedGraphicElement>()) as u64;
    for limit in [total - 1, total] {
        let (budget, result) = extract(limit, b"0 0 m 20 0 l S");
        assert_eq!(result.len(), usize::from(limit == total));
        if limit == total {
            assert!(budget.check().is_ok());
        } else {
            assert!(
                matches!(budget.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == total)
            );
        }
    }
}

/// 塗り矩形の作業slotも結果とは別に予約する。
#[test]
fn graphics_rectangles_require_capacity() {
    let total = 4 * (size_of::<[f32; 4]>() + size_of::<ParsedGraphicElement>()) as u64;
    for limit in [total - 1, total] {
        let (budget, result) = extract(limit, b"0 0 20 1 re f");
        assert_eq!(result.len(), usize::from(limit == total));
        if limit == total {
            assert!(budget.check().is_ok());
        } else {
            assert!(
                matches!(budget.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == total)
            );
        }
    }
}

/// q保存は行列と線幅の全容量を確保前に数える。
#[test]
fn graphics_saved_state_requires_capacity() {
    for limit in [111, 112] {
        let (budget, result) = extract(limit, b"q Q");
        assert!(result.is_empty());
        assert_eq!(budget.check().is_ok(), limit == 112);
    }
}

/// 同じ入力の再抽出も累積し、停止後は結果を返さない。
#[test]
fn graphics_recreation_and_stopped_budget_discard_results() {
    let total = 4 * (size_of::<ParsedBounds>() + size_of::<ParsedGraphicElement>()) as u64;
    let (budget, first) = extract(total, b"0 0 m 20 0 l S");
    assert_eq!(first.len(), 1);
    assert!(graphics_with_budget(&budget, 0, b"0 0 m 20 0 l S").is_empty());
    assert!(budget.check().is_err());
    assert!(graphics_with_budget(&budget, 0, b"0 0 m 20 0 l S").is_empty());
}

/// コメント・文字列・配列・辞書の中の命令は描画しない。
#[test]
fn graphics_ignore_non_operator_contexts() {
    for source in [
        b"% 0 0 m 20 0 l S\n".as_slice(),
        b"( 0 0 m 20 0 l S ) Tj",
        b"[ 0 0 m 20 0 l S ] TJ",
        b"<< /X 0 0 m 20 0 l S >>",
    ] {
        assert!(graphics_with_budget(&DecodeBudget::default(), 0, source).is_empty());
    }
}

/// Qで復元する状態には線幅も含める。
#[test]
fn graphics_restore_stroke_width() {
    let (_, result) = extract(4096, b"2 w q 5 w Q 0 0 m 20 0 l S");
    assert_eq!(result[0].stroke_width, 2.0);
}

/// 区間端がi32上限へ丸められても隣接判定はoverflowしない。
#[test]
fn graphics_merge_handles_extreme_endpoints() {
    let line = ParsedGraphicElement {
        page: 0,
        bounds: ParsedBounds {
            x0: 0.0,
            y0: 0.0,
            x1: i32::MAX as f32,
            y1: 0.0,
        },
        stroke_width: 1.0,
    };
    assert_eq!(
        merge_axis_aligned_segments(vec![line.clone(), line]).len(),
        1
    );
}

/// 小さいPDFのq保存増幅を公開APIでも資源超過として返す。
#[test]
fn public_graphics_state_amplification_is_limited() {
    let source = format!("0 0 m 20 0 l S {}", "q ".repeat(8192));
    let pdf = content_bytes_tests::fixture(source.as_bytes(), b"", false);
    assert!(pdf.len() < 32 * 1024);
    let limits = ParseLimits {
        max_total_metadata_workspace_bytes: 64 * 1024,
        ..Default::default()
    };
    assert!(matches!(
        parse_pdf_bytes_with_limits("graphics.pdf", &pdf, &limits),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
    let budget = DecodeBudget::new(&limits);
    assert!(extract_graphics_from_pdf_bytes(&budget, &pdf).is_empty());
    assert!(budget.check().is_err());
}

/// PDF空白・巨大な数値operand列・binary文字列は一時配列を作らない。
#[test]
fn graphics_operands_and_binary_strings_are_borrowed() {
    let mut source = b"(\xff\x80) ".to_vec();
    source.extend_from_slice("1 ".repeat(8192).as_bytes());
    source.extend_from_slice(b"0\0 0\0 m 20\0 0\0 l S");
    let total = 4 * (size_of::<ParsedBounds>() + size_of::<ParsedGraphicElement>()) as u64;
    let (budget, result) = extract(total, &source);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].bounds.x0, 0.0);
    assert!(budget.check().is_ok());
}

/// 未対応inline画像のbinary命令を罫線として公開しない。
#[test]
fn graphics_stop_at_inline_image_data() {
    let (_, result) = extract(4096, b"BI /W 1 /H 1 ID 0 0 m 20 0 l S EI");
    assert!(result.is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 結果配列の倍増全容量を累積し、後続不足で先行結果も破棄する。
    #[test]
    fn graphics_growth_capacity_model(count in 1usize..25) {
        let source = "0 0 m 20 0 l S ".repeat(count);
        let mut slots = 4usize;
        let mut capacity = 4usize;
        while capacity < count { capacity *= 2; slots += capacity; }
        let total = (4 * size_of::<ParsedBounds>() + slots * size_of::<ParsedGraphicElement>()) as u64;
        for limit in [total - 1, total] {
            let (budget, result) = extract(limit, source.as_bytes());
            prop_assert_eq!(result.len(), if limit == total { count } else { 0 });
            prop_assert_eq!(budget.check().is_ok(), limit == total);
        }
    }

    /// pathとq保存の倍増後全容量を、解放後の再作成も含めて数える。
    #[test]
    fn graphics_workspace_growth_capacity_model(count in 1usize..25, saved in any::<bool>()) {
        let source = if saved { "q ".repeat(count) }
            else { format!("0 0 m {} n", "20 0 l ".repeat(count)) };
        let mut slots = 4usize;
        let mut capacity = 4usize;
        while capacity < count { capacity *= 2; slots += capacity; }
        let total = (slots * if saved { 7 * size_of::<f32>() } else { size_of::<ParsedBounds>() }) as u64;
        for limit in [total - 1, total] {
            let (budget, result) = extract(limit, source.as_bytes());
            prop_assert!(result.is_empty());
            prop_assert_eq!(budget.check().is_ok(), limit == total);
            if limit == total {
                prop_assert!(graphics_with_budget(&budget, 0, source.as_bytes()).is_empty());
                prop_assert!(budget.check().is_err());
            }
        }
    }

    /// 整列結合は入力順に依存せず、再実行しても被覆と線幅を変えない。
    #[test]
    fn graphics_merge_matches_discrete_coverage(intervals in prop::collection::vec((0i32..32, 1i32..8, 1u8..5), 1..40)) {
        let lines: Vec<_> = intervals.iter().map(|&(start, length, width)| ParsedGraphicElement {
            page: 2,
            bounds: ParsedBounds { x0: start as f32, y0: 3.0, x1: (start + length) as f32, y1: 3.0 },
            stroke_width: f32::from(width),
        }).collect();
        let mut reversed = lines.clone(); reversed.reverse();
        let merged = merge_axis_aligned_segments(lines);
        prop_assert_eq!(&merged, &merge_axis_aligned_segments(reversed));
        prop_assert_eq!(&merged, &merge_axis_aligned_segments(merged.clone()));
        for &(start, length, width) in &intervals {
            prop_assert!(merged.iter().any(|line| line.bounds.x0 <= start as f32
                && line.bounds.x1 >= (start + length) as f32 && line.stroke_width >= f32::from(width)));
        }
        for pair in merged.windows(2) { prop_assert!(pair[0].bounds.x1 + 1.0 < pair[1].bounds.x0); }
        for line in merged {
            for cell in line.bounds.x0 as i32..line.bounds.x1 as i32 {
                // 1座標の間隔を連結する既存仕様を含む被覆を検査する。
                prop_assert!(intervals.iter().any(|&(start, length, _)| start <= cell + 1 && start + length >= cell));
            }
        }
    }
}
