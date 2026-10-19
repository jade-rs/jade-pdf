//! RootなしFont Resourcesのnull継承と空辞書の上書きを固定する。
use super::*;
use proptest::prelude::*;

/// 予算対象外のfixture索引を作り、互換継承だけを検査する。
fn read<'a>(
    budget: &DecodeBudget,
    source: &'a str,
    page: &'a str,
) -> Vec<PageFontResourceDefinition<'a>> {
    let objects = crate::metadata::objects(source.as_bytes(), None)
        .map(|object| (object.number, (object.generation, object.body)))
        .collect();
    inherited_page_font_resource_definitions(budget, &objects, page)
}

/// 直接nullは欠落と同じく親のFontを継承する。
#[test]
fn direct_null_inherits_parent_font() {
    let source = "2 0 obj << /Resources << /Font << /ParentFont << >> >> >> >> endobj";
    let target = DecodeBudget::default();
    let result = read(&target, source, "<< /Resources null /Parent 2 0 R >>");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].resource_name, "ParentFont");
    assert!(target.check().is_ok());
}

/// 非ゼロ世代とコメントを含む間接nullの参照chainも親のFontを継承する。
#[test]
fn indirect_null_inherits_parent_font() {
    let source = "2 0 obj << /Resources << /Font << /ParentFont << >> >> >> >> endobj\n8 7 obj 9 3 R endobj\n9 3 obj % null value\n null % tail\n endobj";
    let target = DecodeBudget::default();
    let result = read(&target, source, "<< /Resources 8 7 R /Parent 2 0 R >>");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].resource_name, "ParentFont");
    assert!(target.check().is_ok());
}

/// nullの親を越えて最寄りの非null辞書を採用し、祖先の辞書は混ぜない。
#[test]
fn nearest_non_null_resources_win() {
    let source = "2 0 obj << /Resources null /Parent 3 0 R >> endobj\n3 0 obj << /Resources << /Font << /Nearest << >> >> >> /Parent 4 0 R >> endobj\n4 0 obj << /Resources << /Font << /Far << >> >> >> >> endobj";
    let result = read(&DecodeBudget::default(), source, "<< /Parent 2 0 R >>");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].resource_name, "Nearest");
}

/// 空辞書は直接・間接とも親のFontを上書きする。
#[test]
fn empty_dictionary_stops_inheritance() {
    let source =
        "2 0 obj << /Resources << /Font << /ParentFont << >> >> >> >> endobj\n8 7 obj << >> endobj";
    for resources in ["<< >>", "8 7 R", "<< /Font << >> >>"] {
        let target = DecodeBudget::default();
        let page = format!("<< /Resources {resources} /Parent 2 0 R >>");
        assert!(read(&target, source, &page).is_empty());
        assert!(target.check().is_ok());
    }
}

/// 参照失敗・不正型はnullと区別し、親のFontで失敗を隠さない。
#[test]
fn invalid_resources_do_not_inherit_parent() {
    let source = "2 0 obj << /Resources << /Font << /ParentFont << >> >> >> >> endobj\n8 7 obj 9 0 R endobj\n9 0 obj 8 7 R endobj";
    for resources in ["8 0 R", "8 7 R", "99 0 R", "42", "(null)", "/null", "[]"] {
        let target = DecodeBudget::default();
        let page = format!("<< /Resources {resources} /Parent 2 0 R >>");
        assert!(read(&target, source, &page).is_empty(), "{resources}");
        assert!(target.check().is_ok());
    }
}

/// null解決の容量不足は親のFontを返さず、エラー取得後にも停止を維持する。
#[test]
fn null_resolution_budget_failure_is_sticky() {
    let source =
        "2 0 obj << /Resources << /Font << /ParentFont << >> >> >> >> endobj\n8 7 obj null endobj";
    let target = DecodeBudget::new(&ParseLimits {
        max_total_resource_workspace_bytes: 0,
        ..Default::default()
    });
    assert!(read(&target, source, "<< /Resources 8 7 R /Parent 2 0 R >>").is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "resource workspace bytes",
            ..
        })
    ));
    assert!(
        read(
            &target,
            source,
            "<< /Resources << /Font << /Direct << >> >> >> >>"
        )
        .is_empty()
    );
}

/// null継承でも親参照の深度検査を訪問slotの確保より先に行う。
#[test]
fn null_inheritance_obeys_parent_depth_limit() {
    let source = "2 0 obj << /Resources << /Font << /ParentFont << >> >> >> >> endobj";
    let target = DecodeBudget::new(&ParseLimits {
        max_container_nesting: 0,
        max_total_metadata_workspace_bytes: 0,
        ..Default::default()
    });
    assert!(read(&target, source, "<< /Resources null /Parent 2 0 R >>").is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "page inheritance depth",
            limit: 0,
            actual: 1
        })
    ));
}

/// nullを持つ親の循環はFontを捏造せず有限回で停止する。
#[test]
fn null_parent_cycle_terminates() {
    let source = "2 0 obj << /Resources null /Parent 3 0 R >> endobj\n3 0 obj << /Resources null /Parent 2 0 R >> endobj";
    let target = DecodeBudget::default();
    assert!(read(&target, source, "<< /Resources null /Parent 2 0 R >>").is_empty());
    assert!(target.check().is_ok());
}

/// 同じnull参照chainを一度解決するcacheの最小要求容量を得る。
fn null_cache_capacity(source: &str) -> u64 {
    let objects = crate::metadata::objects(source.as_bytes(), None)
        .map(|object| (object.number, (object.generation, object.body)))
        .collect();
    let mut capacity = 0;
    for _ in 0..16 {
        let target = DecodeBudget::new(&ParseLimits {
            max_total_resource_workspace_bytes: capacity,
            ..Default::default()
        });
        let mut resolver = crate::resource_resolver::ResourceResolver::new(&objects, &target);
        let result = resolver.resolve(b"100 7 R");
        match target.check() {
            Ok(()) => {
                assert!(crate::page_attributes::is_null(result.unwrap().0));
                return capacity;
            }
            Err(ParseError::ResourceLimit {
                resource: "resource workspace bytes",
                actual,
                ..
            }) => {
                assert!(actual > capacity);
                capacity = actual;
            }
            result => panic!("unexpected {result:?}"),
        }
    }
    panic!("null cacheの容量境界へ到達しない");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 欠落/null/空辞書/Font辞書の独立モデルを直接・間接Resourcesで比較する。
    #[test]
    fn null_inheritance_matches_nearest_dictionary_model(
        nodes in prop::collection::vec((0u8..4, any::<bool>()), 1..16)
    ) {
        let mut source = String::new();
        let mut page = String::new();
        let mut expected = None;
        let mut found = false;
        for (i, &(kind, indirect)) in nodes.iter().enumerate() {
            let number = i + 1;
            let value = match kind {
                0 => None,
                1 => Some("null".to_owned()),
                2 => Some("<< >>".to_owned()),
                _ => Some(format!("<< /Font << /Node{i} << >> >> >>")),
            };
            if !found && kind >= 2 {
                expected = (kind == 3).then(|| format!("Node{i}"));
                found = true;
            }
            let resources = if let Some(value) = value {
                if indirect {
                    source.push_str(&format!("{} 7 obj {value} endobj\n", 100 + i));
                    format!("/Resources {} 7 R", 100 + i)
                } else {
                    format!("/Resources {value}")
                }
            } else {
                String::new()
            };
            let body = format!("<< {resources} /Parent {} 3 R >>", number + 1);
            if i == 0 { page = body; }
            else { source.push_str(&format!("{number} 3 obj {body} endobj\n")); }
        }
        source.push_str(&format!("{} 3 obj << /Resources << /Font << /Fallback << >> >> >> >> endobj", nodes.len() + 1));
        if !found { expected = Some("Fallback".to_owned()); }
        let target = DecodeBudget::default();
        let result = read(&target, &source, &page);
        prop_assert_eq!(result.len(), usize::from(expected.is_some()));
        if let Some(expected) = expected {
            prop_assert_eq!(&result[0].resource_name, &expected);
        }
        prop_assert!(target.check().is_ok());
    }

    /// 複数の親が同じnull参照を持ってもresolver容量はchain一回分で収まる。
    #[test]
    fn shared_null_alias_uses_one_cache(parent_count in 1u32..26, alias_count in 1u32..7) {
        let mut source = String::new();
        for number in 100..100 + alias_count {
            let value = if number + 1 == 100 + alias_count { "null".to_owned() }
                else { format!("{} 7 R", number + 1) };
            source.push_str(&format!("{number} 7 obj {value} endobj\n"));
        }
        for number in 2..=parent_count + 1 {
            let body = if number == parent_count + 1 { "/Resources << >>".to_owned() }
                else { format!("/Resources 100 7 R /Parent {} 0 R", number + 1) };
            source.push_str(&format!("{number} 0 obj << {body} >> endobj\n"));
        }
        let capacity = null_cache_capacity(&source);
        for limit in [capacity - 1, capacity] {
            let target = DecodeBudget::new(&ParseLimits {
                max_total_resource_workspace_bytes: limit,
                ..Default::default()
            });
            prop_assert!(read(&target, &source, "<< /Resources 100 7 R /Parent 2 0 R >>").is_empty());
            prop_assert_eq!(target.check().is_ok(), limit == capacity);
        }
    }
}
