//! 互換ページの親世代・最新本文・共有索引容量を固定する。
use super::*;
use proptest::prelude::*;

/// Rootなしfixtureのviewportを共有予算で読み、停止していないことを検査する。
fn read(source: &str) -> Vec<ParsedPageViewport> {
    let budget = DecodeBudget::default();
    let result = parse_page_viewports(&budget, source.as_bytes());
    budget.check().unwrap();
    result
}

/// 存在しない世代からMediaBoxを復活させない。
#[test]
fn legacy_media_box_rejects_parent_generation_mismatch() {
    let source = "1 0 obj << /Type /Page /Parent 2 0 R >> endobj\n2 7 obj << /MediaBox [0 0 100 200] >> endobj";
    assert!(read(source).is_empty());
}

/// 世代不一致のCropBoxとRotateは直接MediaBoxのページへ適用しない。
#[test]
fn legacy_crop_rotation_reject_parent_generation_mismatch() {
    let source = "1 0 obj << /Type /Page /MediaBox [0 0 100 200] /Parent 2 0 R >> endobj\n2 1 obj << /CropBox [10 20 90 170] /Rotate 90 >> endobj";
    let result = read(source);
    assert_eq!(
        (
            result[0].page.width,
            result[0].page.height,
            result[0].page.rotation
        ),
        (100.0, 200.0, 0)
    );
    assert!(!result[0].clip_to_page);
}

/// 同番号更新では最後の世代の矩形・原点・回転を三属性とも選ぶ。
#[test]
fn legacy_geometry_uses_latest_parent_body() {
    let source = "2 0 obj << /MediaBox [0 0 10 20] /CropBox [0 0 5 5] /Rotate 90 >> endobj\n1 0 obj << /Type /Page /Parent 2 3 R >> endobj\n2 3 obj << /MediaBox [0 0 100 200] /CropBox [10 20 90 170] /Rotate 180 >> endobj";
    let result = read(source);
    assert_eq!(
        (
            result[0].page.width,
            result[0].page.height,
            result[0].page.rotation
        ),
        (80.0, 150.0, 180)
    );
    assert_eq!((result[0].origin_x, result[0].origin_y), (10.0, 20.0));
    assert!(result[0].clip_to_page);
}

/// Font継承の深さ超過は訪問slotを確保する前に診断する。
#[test]
fn legacy_font_parent_depth_rejects_before_lookup() {
    let objects =
        crate::page_attributes::Objects::from([(2, (0, b"<< /Resources << >> >>".as_slice()))]);
    let target = DecodeBudget::new(&ParseLimits {
        max_container_nesting: 0,
        max_total_metadata_workspace_bytes: 0,
        ..Default::default()
    });
    assert!(
        inherited_page_font_resource_definitions(&target, &objects, "<< /Parent 2 0 R >>")
            .is_empty()
    );
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "page inheritance depth",
            limit: 0,
            actual: 1,
            ..
        })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 互換幾何とFontの親chainは深さ境界を共有し、超過後は直接属性も返さない。
    #[test]
    fn legacy_parent_depth_is_inclusive_and_sticky(count in 1u32..25, font in any::<bool>()) {
        let mut source = "1 0 obj << /Type /Page /Parent 2 0 R >> endobj\n".to_owned();
        for number in 2..=count + 1 {
            let body = if number == count + 1 { "/MediaBox [0 0 100 200] /Resources << >>".to_owned() }
                else { format!("/Parent {} 0 R", number + 1) };
            source.push_str(&format!("{number} 0 obj << {body} >> endobj\n"));
        }
        let objects: crate::page_attributes::Objects<'_> = crate::metadata::objects(source.as_bytes(), None)
            .map(|object| (object.number, (object.generation, object.body))).collect();
        for limit in [count as usize - 1, count as usize] {
            let target = DecodeBudget::new(&ParseLimits { max_container_nesting: limit, ..Default::default() });
            if font {
                prop_assert!(inherited_page_font_resource_definitions(&target, &objects, "<< /Parent 2 0 R >>").is_empty());
            } else {
                let result = parse_page_viewports(&target, source.as_bytes());
                prop_assert_eq!(result.len(), usize::from(limit == count as usize));
            }
            let status = target.check();
            prop_assert_eq!(status.is_ok(), limit == count as usize);
            if limit < count as usize {
                prop_assert!(matches!(status, Err(ParseError::ResourceLimit { resource: "page inheritance depth", actual, .. }) if actual == count as u64), "親chain深度超過の型と実測値");
                prop_assert!(parse_inherited_page_box(&target, &objects, "<< /MediaBox [0 0 100 200] >>", "/MediaBox").is_none());
                prop_assert!(inherited_page_font_resource_definitions(&target, &objects, "<< /Resources << /Font << /F << >> >> >> >>").is_empty());
            }
        }
    }

    /// 長いchainの三属性は一つの索引と三つの訪問集合の全容量を予約する。
    #[test]
    fn legacy_geometry_index_and_visits_capacity_model(count in 1u32..20) {
        let mut source = "1 0 obj << /Type /Page /Parent 2 0 R >> endobj\n".to_owned();
        for number in 2..=count + 1 {
            let body = if number == count + 1 { "/MediaBox [0 0 100 200] /CropBox [10 20 90 170] /Rotate 90".to_owned() }
                else { format!("/Parent {} 0 R", number + 1) };
            source.push_str(&format!("{number} 0 obj << {body} >> endobj\n"));
        }
        let probe = DecodeBudget::new(&ParseLimits { max_total_metadata_workspace_bytes: 0, ..Default::default() });
        prop_assert!(crate::object_index::build(&probe, source.as_bytes()).is_none());
        let Err(ParseError::ResourceLimit { actual: index_bytes, .. }) = probe.check() else { panic!("索引容量が未検査"); };
        let mut capacity = 4usize;
        let mut slots = capacity;
        while capacity / 2 < count as usize { capacity *= 2; slots += capacity; }
        let base = source.len() + size_of::<&[u8]>() + size_of::<ParsedPageViewport>();
        let total = base as u64 + index_bytes + (3 * slots * size_of::<Option<u32>>()) as u64;
        for limit in [total - 1, total] {
            let target = DecodeBudget::new(&ParseLimits { max_total_metadata_workspace_bytes: limit, ..Default::default() });
            let result = parse_page_viewports(&target, source.as_bytes());
            prop_assert_eq!(result.len(), usize::from(limit == total));
            prop_assert_eq!(target.check().is_ok(), limit == total);
            if limit == total { prop_assert_eq!(result[0].page.rotation, 90); }
        }
    }

    /// 後勝ち本文と参照世代の一致を独立モデルとして照合する。
    #[test]
    fn legacy_geometry_matches_latest_generation_model(generation in 1u16..65535, matches in any::<bool>(), width in 20u16..1000, quarter in 0i32..4) {
        let referenced = if matches { generation } else { generation - 1 };
        let rotation = quarter * 90;
        let source = format!("2 0 obj << /MediaBox [0 0 5 5] /CropBox [0 0 4 4] /Rotate 270 >> endobj\n1 0 obj << /Type /Page /Parent 2 {referenced} R >> endobj\n2 {generation} obj << /MediaBox [0 0 {width} 200] /CropBox [1 2 {} 190] /Rotate {rotation} >> endobj", width - 1);
        let result = read(&source);
        prop_assert_eq!(result.len(), usize::from(matches));
        if matches {
            prop_assert_eq!(result[0].page.width, f32::from(width - 2));
            prop_assert_eq!(result[0].page.height, 188.0);
            prop_assert_eq!(result[0].page.rotation, rotation);
        }
    }
}
