//! 同じ字体区分の行検出が、座標の絶対値や物理単位に依存しないことを検証する。
use super::{canonicalize_runs, Bounds, ExtractedRun};
use proptest::prelude::*;

/// 描画や外部字体に依存しない、一文字と元の座標・SDK診断の資料。
fn run(text: &str, x: f64, y: f64, size: f64, point: f64) -> ExtractedRun {
    let bounds = Bounds {
        x0: x * point,
        y0: (y - 2.) * point,
        x1: (x + 6.) * point,
        y1: (y + 8.) * point,
    };
    ExtractedRun {
        pending_spaces: Vec::new(),
        advance: Default::default(),
        text: text.into(),
        source_text_region: false,
        source_text_anchor: false,
        source_regions: Vec::new(),
        sdk_unicode_mapping_errors: usize::from(text == "B"),
        bounds,
        size: Some(size * point),
        spans: vec![(bounds.x0, bounds.x1)],
        origin: (x * point, y * point),
        angle: 0.,
        vertical: false,
        layout: bounds,
        tagged_cell: None,
        outside_mark: None,
    }
}

/// 補助文字のUTF-8バイト幅によらず、生成空白・文字セル・原文領域を同じ添字で更新する。
#[test]
fn deferred_spacing_preserves_source_region_and_character_cell_ownership() {
    let mut value = run("A", 0., 10., 12., 1.);
    value.text = "𐐀 B C".into();
    value.spans = vec![(0., 1.), (1., 1.), (2., 3.), (3., 3.), (4., 5.)];
    value.pending_spaces = vec![1];
    value.sdk_unicode_mapping_errors = 1;
    let bounds = value.bounds;
    value.source_regions = vec![crate::render::SourceTextRegion {
        start: 2,
        end: 5,
        bounds,
        anchor: false,
    }];
    let result = super::spacing::finish_order(vec![value]);
    assert_eq!(result[0].text, "𐐀B C");
    assert_eq!(
        result[0].spans,
        vec![(0., 1.), (2., 3.), (3., 3.), (4., 5.)]
    );
    assert_eq!(
        (
            result[0].source_regions[0].start,
            result[0].source_regions[0].end
        ),
        (1, 4)
    );
    assert_eq!(result[0].source_regions[0].bounds, bounds);
    assert_eq!(result[0].sdk_unicode_mapping_errors, 1);
}

/// NIST原本と同じ微小な字号・ベースライン差でも、同じ行を断片化しない。
#[test]
fn nearly_equal_sizes_share_the_same_baseline_grid() {
    let original = vec![
        run("A", 10., 504.19, 12., 1.),
        run("B", 20., 504.099, 12.0452776, 1.),
        run("C", 30., 504.19, 12., 1.),
    ];
    let cells: Vec<_> = original
        .iter()
        .flat_map(|r| r.spans.iter().copied())
        .collect();
    let result = canonicalize_runs(original, 1.);
    assert_eq!(
        result.len(),
        1,
        "Same-line fragments used incompatible baseline grids"
    );
    assert_eq!(result[0].text.split_whitespace().collect::<String>(), "ABC");
    assert_eq!(result[0].sdk_unicode_mapping_errors, 1);
    for cell in cells {
        assert!(result[0].spans.contains(&cell));
    }
}

/// 隣接する別行と、別の字号区分を結合しない。
#[test]
fn distinct_baselines_and_size_groups_remain_separate() {
    for (y, size) in [(506., 12.0452776), (504.19, 12.06)] {
        let result = canonicalize_runs(
            vec![run("A", 10., 504.19, 12., 1.), run("B", 20., y, size, 1.)],
            1.,
        );
        assert_eq!(result.len(), 2);
    }
}

/// 実字号の許容差が格子境界を僅かに跨いでも、従来許していた同じ行を保持する。
#[test]
fn actual_size_tolerance_survives_grid_boundaries() {
    let result = canonicalize_runs(
        vec![
            run("A", 10., 1.1999, 12.049, 1.),
            run("B", 20., 2.4047, 12.049, 1.),
        ],
        1.,
    );
    assert_eq!(result.len(), 1);
    let result = canonicalize_runs(
        vec![
            run("A", 10., 1.1999, 12.049, 1.),
            run("B", 20., 2.405, 12.049, 1.),
        ],
        1.,
    );
    assert_eq!(result.len(), 2);
    let result = canonicalize_runs(
        vec![
            run("A", 10., 504.19, 12., 1.),
            run("B", 200., 504.099, 12.045, 1.),
        ],
        1.,
    );
    assert_eq!(
        result.len(),
        2,
        "A large horizontal gutter still separates columns"
    );
}

proptest! {
    /// 同じ行の平行移動・物理単位・同一区分内の字号差で文字数と所属を変えない。
    #[test]
    fn baseline_membership_is_translation_and_unit_covariant(y in -1_000_000f64..1_000_000., delta in 0.0001f64..0.049, exponent in -10i32..11) {
        let point=2f64.powi(exponent);
        let result=canonicalize_runs(vec![run("A",10.,y,12.,point),run("B",20.,y-0.09,12.+delta,point),run("C",30.,y,12.,point)],point);
        prop_assert_eq!(result.len(),1);
        prop_assert_eq!(result[0].text.split_whitespace().collect::<String>(),"ABC");
        prop_assert_eq!(result[0].sdk_unicode_mapping_errors,1);
    }
}
