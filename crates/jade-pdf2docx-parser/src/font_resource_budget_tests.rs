//! Font定義・名前索引の予算外確保を固定する境界テスト。
use super::*;
use proptest::prelude::*;
use std::fmt::Write as _;

/// resource解決予算と独立にmetadata作業容量だけを制限する。
fn budget(capacity: usize) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity as u64,
        ..ParseLimits::default()
    })
}

/// 容量超過が保留されていることを確認する。
fn assert_limit(budget: &DecodeBudget) {
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// 定義の確保を除外し、局所Font mapだけの要求容量を検査する。
fn local_fonts(target: &DecodeBudget, dictionaries: &[&str]) -> PageFontResources {
    let named: Vec<_> = dictionaries
        .iter()
        .enumerate()
        .map(|(index, dictionary)| (format!("F{index}"), *dictionary))
        .collect();
    local_fonts_named(target, &named)
}

/// 重複した局所名も先に確保した定義として渡す。
fn local_fonts_named(target: &DecodeBudget, dictionaries: &[(String, &str)]) -> PageFontResources {
    let objects = crate::page_attributes::Objects::new();
    let mut cache = crate::font_metrics_cache::Cache::new(&objects, target, &[]);
    font_resources_from_definitions(
        target,
        &[],
        dictionaries
            .iter()
            .map(|(name, dictionary)| PageFontResourceDefinition {
                resource_name: name.clone(),
                object_number: None,
                inline_dictionary: Some(dictionary),
            })
            .collect(),
        &font_names::Names::default(),
        &font_names::Names::default(),
        &font_cmaps::Index::default(),
        &StreamOffsets::default(),
        &mut cache,
    )
}

/// 幅表を持たないFontも、局所mapのslotとキーを無制限には保存しない。
#[test]
fn local_maps_reject_zero_capacity() {
    let target = budget(0);
    let fonts = local_fonts(&target, &["<< /BaseFont /Helvetica >>"]);
    assert!(fonts.metrics.is_empty());
    assert!(fonts.base_fonts.is_empty());
    assert_limit(&target);
}

/// 名前・値の保存容量を拒否した場合に作成途中のmapを返さない。
#[test]
fn local_maps_discard_partial_result() {
    let target = budget(1);
    let fonts = local_fonts(
        &target,
        &["<< /BaseFont /Helvetica /Encoding /Identity-H >>"],
    );
    assert!(fonts.base_fonts.is_empty());
    assert!(fonts.encodings.is_empty());
    assert!(fonts.metrics.is_empty());
    assert_limit(&target);
}

/// 直接FontでもArc本体のresource容量を予約してから作成する。
#[test]
fn local_maps_direct_metrics_reject_zero_resource_capacity() {
    let target = DecodeBudget::new(&ParseLimits {
        max_total_resource_workspace_bytes: 0,
        ..Default::default()
    });
    let fonts = local_fonts(&target, &["<< /BaseFont /Helvetica >>"]);
    assert!(fonts.metrics.is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "resource workspace bytes",
            ..
        })
    ));
}

/// 定義slotとresource名のどちらも容量0のまま保存できない。
#[test]
fn definitions_reject_zero_capacity() {
    let budget = budget(0);
    let definitions = font_resource_definitions_from_dictionary(
        &budget,
        &crate::page_attributes::Objects::new(),
        "<< /Font << /F1 << /Type /Font /BaseFont /Helvetica >> >> >>",
    );
    assert!(definitions.is_empty());
    assert_limit(&budget);
}

/// 正確なslot・復号済み名容量の直前は部分定義を返さない。
#[test]
fn definitions_reserve_slots_and_names() {
    let capacity = std::mem::size_of::<PageFontResourceDefinition>() + 2;
    let budget = budget(capacity - 1);
    let definitions = font_resource_definitions_from_dictionary(
        &budget,
        &crate::page_attributes::Objects::new(),
        "<< /Font << /F#31 << /Type /Font /BaseFont /Helvetica >> >> >>",
    );
    assert!(definitions.is_empty());
    assert_limit(&budget);
}

/// 有効Fontだけで構成されるobject索引もmetadata予算に含める。
#[test]
fn object_names_reject_zero_capacity() {
    let objects = crate::page_attributes::Objects::from([(
        7,
        (0, b"<< /Type /Font /BaseFont /Helvetica >>".as_slice()),
    )]);
    let budget = budget(0);
    let names = font_object_names(&budget, &objects, b"BaseFont");
    assert!(names.get(&7).is_none());
    assert_limit(&budget);
}

/// 長いinline辞書も元入力を借用し、保存する名とslotの容量だけを使う。
#[test]
fn definitions_borrow_inline_dictionary_at_exact_capacity() {
    let resources = format!(
        "<< /Font << /F#ff << /Type /Font /Extra ({}) >> >> >>",
        "x".repeat(8192)
    );
    let capacity = std::mem::size_of::<PageFontResourceDefinition>() + 3;
    let budget = budget(capacity);
    let objects = crate::page_attributes::Objects::new();
    let definitions = font_resource_definitions_from_dictionary(&budget, &objects, &resources);
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].resource_name, "Fÿ");
    let dictionary = definitions[0].inline_dictionary.unwrap();
    let offset = resources.find("<< /Type").unwrap();
    assert_eq!(dictionary.as_ptr(), resources[offset..].as_ptr());
    budget.check().unwrap();
}

/// 解放した定義の再作成も容量へ加え、境界の一byte手前では再開させない。
#[test]
fn definitions_count_recreation_and_reject_consumed_stop() {
    let objects = crate::page_attributes::Objects::new();
    let resources = "<< /Font << /F1 <<>> >> >>";
    let one = std::mem::size_of::<PageFontResourceDefinition>() + 2;
    let target = budget(2 * one - 1);
    assert_eq!(
        font_resource_definitions_from_dictionary(&target, &objects, resources).len(),
        1
    );
    assert!(font_resource_definitions_from_dictionary(&target, &objects, resources).is_empty());
    assert_limit(&target);
    assert!(font_resource_definitions_from_dictionary(&target, &objects, resources).is_empty());
    let target = budget(2 * one);
    for _ in 0..2 {
        assert_eq!(
            font_resource_definitions_from_dictionary(&target, &objects, resources).len(),
            1
        );
    }
    target.check().unwrap();
}

/// 無効名・無効値も候補slotへ数え、有効な隣の定義を残す。
#[test]
fn definitions_count_rejected_candidates() {
    let resources = "<< /Font << /F#+F <<>> /number 42 /good <<>> >> >>";
    let capacity = 3 * std::mem::size_of::<PageFontResourceDefinition>() + 4;
    let target = budget(capacity);
    let definitions = font_resource_definitions_from_dictionary(
        &target,
        &crate::page_attributes::Objects::new(),
        resources,
    );
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].resource_name, "good");
    target.check().unwrap();
}

/// 間接Font辞書の解決後も本文コピーを作らずobject番号を保持する。
#[test]
fn definitions_resolve_indirect_font_without_dictionary_copy() {
    let objects = crate::page_attributes::Objects::from([
        (7, (2, b"<< /Type /Font /BaseFont /Helvetica >>".as_slice())),
        (8, (0, b"7 2 R".as_slice())),
    ]);
    let target = budget(2 * std::mem::size_of::<PageFontResourceDefinition>() + 2);
    let definitions = font_resource_definitions_from_dictionary(
        &target,
        &objects,
        "<< /Font << /F1 8 0 R /bad 7 0 R >> >>",
    );
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].object_number, Some(7));
    assert_eq!(definitions[0].inline_dictionary, None);
    target.check().unwrap();
}

/// Encoding名のUTF-8 byte容量と全object slotを正確に予約する。
#[test]
fn object_names_count_utf8_and_non_font_slots() {
    let objects = crate::page_attributes::Objects::from([
        (9, (0, b"<< /Type /Font /Encoding /#c3#a9 >>".as_slice())),
        (7, (0, b"<< /Type /Page /Encoding /Ignored >>".as_slice())),
        (
            3,
            (0, b"<< /Type /Font /Encoding /Identity#2dH >>".as_slice()),
        ),
    ]);
    let one = 3 * std::mem::size_of::<(u32, u16, String)>() + 2 + 10;
    let target = budget(one - 1);
    assert!(
        font_object_names(&target, &objects, b"Encoding")
            .get(&3)
            .is_none()
    );
    assert_limit(&target);
    let target = budget(one);
    let names = font_object_names(&target, &objects, b"Encoding");
    assert_eq!(names.get(&9).map(String::as_str), Some("é"));
    assert_eq!(
        names.into_iter().map(|(n, _, _)| n).collect::<Vec<_>>(),
        [3, 9]
    );
    target.check().unwrap();
}

/// 不正escapeは無確保、不正UTF-8は破棄した保存候補の容量も累積する。
#[test]
fn object_names_count_discarded_utf8_payload() {
    let objects = crate::page_attributes::Objects::from([
        (1, (0, b"<< /Type /Font /BaseFont /#ff >>".as_slice())),
        (2, (0, b"<< /Type /Font /BaseFont /#gg >>".as_slice())),
        (3, (0, b"<< /Type /Font /BaseFont /Good >>".as_slice())),
    ]);
    let one = 3 * std::mem::size_of::<(u32, u16, String)>() + 1 + 4;
    let target = budget(one);
    let names = font_object_names(&target, &objects, b"BaseFont");
    assert_eq!(names.get(&1), None);
    assert_eq!(names.get(&2), None);
    assert_eq!(names.get(&3).map(String::as_str), Some("Good"));
    target.check().unwrap();
    let target = budget(one - 1);
    assert!(
        font_object_names(&target, &objects, b"BaseFont")
            .get(&3)
            .is_none()
    );
    assert_limit(&target);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// 任意byte名の独立UTF-8容量モデルに境界・再作成・本文借用が一致する。
    #[test]
    fn definitions_match_capacity_model(names in prop::collection::vec(prop::collection::vec(any::<u8>(), 1..32), 1..12)) {
        let mut resources = String::from("<< /Font << ");
        for name in &names {
            resources.push('/');
            for byte in name { write!(&mut resources, "#{byte:02x}").unwrap(); }
            resources.push_str(" << /Type /Font >> ");
        }
        resources.push_str(">> >>");
        let one = names.len() * std::mem::size_of::<PageFontResourceDefinition>() + names.iter().flatten().map(|&n| if n < 128 { 1 } else { 2 }).sum::<usize>();
        let objects = crate::page_attributes::Objects::new();
        let target = budget(one - 1);
        prop_assert!(font_resource_definitions_from_dictionary(&target, &objects, &resources).is_empty());
        assert_limit(&target);
        let target = budget(one);
        let definitions = font_resource_definitions_from_dictionary(&target, &objects, &resources);
        prop_assert_eq!(definitions.len(), names.len());
        for (definition, name) in definitions.iter().zip(&names) {
            let expected: String = name.iter().copied().map(char::from).collect();
            prop_assert_eq!(&definition.resource_name, &expected);
            prop_assert!(definition.inline_dictionary.is_some());
        }
        target.check().unwrap();
        let target = budget(2 * one - 1);
        prop_assert_eq!(font_resource_definitions_from_dictionary(&target, &objects, &resources).len(), names.len());
        prop_assert!(font_resource_definitions_from_dictionary(&target, &objects, &resources).is_empty());
        assert_limit(&target);
        let target = budget(2 * one);
        for _ in 0..2 { prop_assert_eq!(font_resource_definitions_from_dictionary(&target, &objects, &resources).len(), names.len()); }
        target.check().unwrap();
    }

    /// 番号順の索引と再作成容量が独立した件数・文字列byteモデルに一致する。
    #[test]
    fn object_names_match_capacity_model(values in prop::collection::vec("[A-Za-z0-9]{1,40}", 1..16)) {
        let bodies = values.iter().map(|name| format!("<< /Type /Font /BaseFont /{name} >>")).collect::<Vec<_>>();
        let objects = bodies.iter().enumerate().rev().map(|(index, body)| ((index + 1) as u32, (0, body.as_bytes()))).collect::<crate::page_attributes::Objects>();
        let one = values.len() * std::mem::size_of::<(u32, u16, String)>() + values.iter().map(String::len).sum::<usize>();
        let target = budget(one - 1);
        prop_assert!(font_object_names(&target, &objects, b"BaseFont").get(&1).is_none());
        assert_limit(&target);
        let target = budget(one);
        let names = font_object_names(&target, &objects, b"BaseFont");
        for (index, value) in values.iter().enumerate() { prop_assert_eq!(names.get(&((index + 1) as u32)), Some(value)); }
        target.check().unwrap();
        let target = budget(2 * one - 1);
        prop_assert!(font_object_names(&target, &objects, b"BaseFont").get(&1).is_some());
        prop_assert!(font_object_names(&target, &objects, b"BaseFont").get(&1).is_none());
        assert_limit(&target);
        prop_assert!(font_object_names(&target, &objects, b"BaseFont").get(&1).is_none());
        let target = budget(2 * one);
        for _ in 0..2 { prop_assert!(font_object_names(&target, &objects, b"BaseFont").get(&1).is_some()); }
        target.check().unwrap();
    }
}

/// slot配列の全拡張容量を、件数だけから独立に計算する。
fn local_slot_capacity<T>(count: usize) -> usize {
    let mut slots = 0;
    let mut total = 0;
    for length in 0..count {
        if length >= slots / 2 {
            slots = (slots * 2).max(4);
            total += slots * std::mem::size_of::<Option<(String, T)>>();
        }
    }
    total
}

/// 直接Fontの幅表Arc本体にもinclusiveな境界を適用する。
#[test]
fn local_maps_direct_metrics_exact_resource_boundary() {
    let capacity = (std::mem::size_of::<crate::font_widths::Metrics>()
        + 2 * std::mem::size_of::<usize>()) as u64;
    for limit in [capacity - 1, capacity] {
        let target = DecodeBudget::new(&ParseLimits {
            max_total_resource_workspace_bytes: limit,
            ..Default::default()
        });
        let fonts = local_fonts(&target, &["<<>>"]);
        assert_eq!(fonts.metrics.is_empty(), limit < capacity);
        if limit == capacity {
            target.check().unwrap();
        } else {
            assert!(
                matches!(target.check(), Err(ParseError::ResourceLimit { resource: "resource workspace bytes", actual, .. }) if actual == capacity)
            );
        }
    }
}

/// 解放後のmap再作成もslotとキーの要求容量へ累積する。
#[test]
fn local_maps_recreation_counts_capacity_and_stays_stopped() {
    let one = local_slot_capacity::<std::sync::Arc<crate::font_widths::Metrics>>(1) + 2;
    for limit in [2 * one - 1, 2 * one] {
        let target = budget(limit);
        assert!(!local_fonts(&target, &["<<>>"]).metrics.is_empty());
        let second = local_fonts(&target, &["<<>>"]);
        assert_eq!(second.metrics.is_empty(), limit < 2 * one);
        if limit == 2 * one {
            target.check().unwrap();
        } else {
            assert_limit(&target);
            assert!(local_fonts(&target, &["<<>>"]).metrics.is_empty());
        }
    }
}

/// 同名Fontは各fieldだけ後出し優先にし、欠落fieldの既存値を消さない。
#[test]
fn local_maps_duplicate_names_preserve_independent_fields() {
    let target = budget(100_000);
    let fonts = local_fonts_named(
        &target,
        &[
            ("Fé".into(), "<< /BaseFont /First /Encoding /Identity-H >>"),
            ("Fé".into(), "<< /BaseFont /Last >>"),
            ("Fé".into(), "<< /Encoding /Identity-V >>"),
        ],
    );
    assert_eq!(fonts.base_fonts.get("Fé").map(String::as_str), Some("Last"));
    assert_eq!(
        fonts.encodings.get("Fé").map(String::as_str),
        Some("Identity-V")
    );
    assert!(fonts.metrics.get("Fé").is_some());
    target.check().unwrap();
}

/// 後続mapの値容量を拒否したときも、完成していた前のFontを外へ返さない。
#[test]
fn local_maps_late_failure_discards_completed_entries() {
    let one = local_slot_capacity::<std::sync::Arc<crate::font_widths::Metrics>>(1)
        + 2
        + local_slot_capacity::<String>(1)
        + 2
        + 1;
    let target = budget(one);
    let fonts = local_fonts(&target, &["<< /BaseFont /A >>", "<< /BaseFont /Long >>"]);
    assert!(fonts.metrics.is_empty());
    assert!(fonts.base_fonts.is_empty());
    assert_limit(&target);
}

proptest! {
    /// 独立したslot/UTF-8 byteモデルと局所mapのinclusive境界が一致する。
    #[test]
    fn local_maps_match_utf8_capacity_model(names in prop::collection::vec("[a-zé]{1,12}", 1..25)) {
        let named: Vec<_> = names.iter().enumerate().map(|(i, name)| (format!("F{i}{name}"), "<< /BaseFont /#c3#a9 /Encoding /Identity-H >>")).collect();
        let keys: usize = named.iter().map(|(key, _)| key.len()).sum();
        let count = named.len();
        let capacity = local_slot_capacity::<std::sync::Arc<crate::font_widths::Metrics>>(count)
            + 2 * local_slot_capacity::<String>(count) + 3 * keys + count * (2 + 10);
        for limit in [capacity - 1, capacity] {
            let target = budget(limit);
            let fonts = local_fonts_named(&target, &named);
            if limit == capacity {
                for (key, _) in &named {
                    prop_assert_eq!(fonts.base_fonts.get(key.as_str()).map(String::as_str), Some("é"));
                    prop_assert_eq!(fonts.encodings.get(key.as_str()).map(String::as_str), Some("Identity-H"));
                    prop_assert!(fonts.metrics.get(key.as_str()).is_some());
                }
                target.check().unwrap();
            } else {
                prop_assert!(fonts.base_fonts.is_empty());
                prop_assert!(fonts.encodings.is_empty());
                prop_assert!(fonts.metrics.is_empty());
                assert_limit(&target);
            }
        }
    }
}
