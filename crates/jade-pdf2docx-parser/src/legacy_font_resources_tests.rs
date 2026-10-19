//! 互換Font継承の訪問容量と借用参照の正確性を固定する。
use super::*;
use proptest::prelude::*;

/// 訪問集合のmetadata容量だけを限定する共有予算を作る。
fn budget(limit: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: limit,
        ..Default::default()
    })
}

/// 予算検査対象外のfixture索引を作り、継承の容量だけを観測する。
fn definitions<'a>(
    budget: &DecodeBudget,
    source: &'a str,
    page: &'a str,
) -> Vec<PageFontResourceDefinition<'a>> {
    let objects = crate::metadata::objects(source.as_bytes(), None)
        .map(|object| (object.number, (object.generation, object.body)))
        .collect();
    inherited_page_font_resource_definitions(budget, &objects, page)
}

/// Fontが空でも親訪問slotの不足を隠さず、inclusiveでのみ継承を完了する。
#[test]
fn inherited_font_resources_require_visited_capacity() {
    let source = "1 0 obj << /Resources << >> >> endobj";
    let total = (4 * size_of::<Option<u32>>()) as u64;
    for limit in [total - 1, total] {
        let target = budget(limit);
        assert!(definitions(&target, source, "<< /Parent 1 0 R >>").is_empty());
        if limit == total {
            assert!(target.check().is_ok());
        } else {
            assert!(matches!(target.check(), Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes", actual, ..
        }) if actual == total));
        }
    }
}

/// 別ページで再作成する親集合も累積し、エラー取得後には再開しない。
#[test]
fn inherited_font_resources_recreation_is_cumulative() {
    let total = (4 * size_of::<Option<u32>>()) as u64;
    let target = budget(total * 2 - 1);
    let source = "1 0 obj << /Resources << >> >> endobj";
    let page = "<< /Parent 1 0 R >>";
    assert!(definitions(&target, source, page).is_empty());
    assert!(target.check().is_ok());
    assert!(definitions(&target, source, page).is_empty());
    assert!(
        matches!(target.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == total * 2)
    );
    assert!(
        definitions(
            &target,
            source,
            "<< /Resources << /Font << /F1 << /BaseFont /Helvetica >> >> >> >>"
        )
        .is_empty()
    );
}

/// 親の番号が一致しても異なる世代のFont定義を継承しない。
#[test]
fn inherited_font_resources_respect_parent_generation() {
    let source =
        "1 2 obj << /Resources << /Font << /Wrong << /BaseFont /Helvetica >> >> >> >> endobj";
    assert!(definitions(&DecodeBudget::default(), source, "<< /Parent 1 0 R >>").is_empty());
    assert_eq!(
        definitions(&DecodeBudget::default(), source, "<< /Parent 1 2 R >>")[0].resource_name,
        "Wrong"
    );
}

/// 同番号の最後の有効本文から継承し、先行世代のFontを混入させない。
#[test]
fn inherited_font_resources_use_latest_object() {
    let source = "1 0 obj << /Resources << /Font << /Old << >> >> >> >> endobj\n1 1 obj << /Resources << /Font << /New << >> >> >> >> endobj";
    let result = definitions(&DecodeBudget::default(), source, "<< /Parent 1 1 R >>");
    assert_eq!(result[0].resource_name, "New");
}

/// Resources参照でも世代を照合し、異なる世代の辞書を使わない。
#[test]
fn inherited_font_resources_respect_resource_generation() {
    let source = "1 2 obj << /Font << /Wrong << >> >> >> endobj";
    assert!(definitions(&DecodeBudget::default(), source, "<< /Resources 1 0 R >>").is_empty());
}

/// Resources辞書へ至る間接参照のchainを既存resolverで解決できる。
#[test]
fn inherited_font_resources_follow_resource_alias() {
    let source = "1 0 obj << /Font << /F1 << >> >> >> endobj\n2 0 obj 1 0 R endobj";
    let result = definitions(&DecodeBudget::default(), source, "<< /Resources 2 0 R >>");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].resource_name, "F1");
}

/// inline Fontの参照は完全な辞書名とPDF空白・コメントを認識する。
#[test]
fn inline_font_reference_uses_pdf_dictionary_grammar() {
    let objects = crate::page_attributes::Objects::from([
        (7, (2, b"<< >>".as_slice())),
        (8, (0, b"<< >>".as_slice())),
        (9, (0, b"<< >>".as_slice())),
    ]);
    assert_eq!(
        font_cmaps::reference(&objects, b"<< /ToUnicode 7 % number\n2\0R >>"),
        Some(7)
    );
    assert_eq!(
        font_cmaps::reference(&objects, b"<< /To#55nicode 8 0 R >>"),
        Some(8)
    );
    for dictionary in [
        "<< /ToUnicodeSuffix 9 0 R >>",
        "<< /Name ( /ToUnicode 9 0 R ) >>",
        "<< % /ToUnicode 9 0 R\n >>",
        "<< /Other << /ToUnicode 9 0 R >> >>",
        "<< /ToUnicode 9 invalid R >>",
        "<< /ToUnicode 9 -1 R >>",
        "<< /ToUnicode 9 65536 R >>",
        "<< /ToUnicode 9 0 /R >>",
    ] {
        assert!(
            font_cmaps::reference(&objects, dictionary.as_bytes()).is_none(),
            "{dictionary}"
        );
    }
}

/// ページFont抽出が成功する最小容量を、公開された超過量から求める。
fn page_font_capacity(source: &[u8]) -> u64 {
    let mut capacity = 0;
    for _ in 0..128 {
        let target = budget(capacity);
        let result = parse_page_font_resources(&target, source);
        match target.check() {
            Ok(()) => {
                assert_eq!(result.len(), 2);
                return capacity;
            }
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                actual,
                ..
            }) => {
                assert!(actual > capacity);
                capacity = actual;
            }
            result => panic!("unexpected {result:?}"),
        }
    }
    panic!("Font抽出の容量境界へ到達しない");
}

/// 後続ページのFont親継承で不足した場合、先行ページの空mapも返さない。
#[test]
fn page_font_inheritance_failure_discards_all_pages() {
    let source = b"1 0 obj << /Type /Page /Resources << >> >> endobj\n2 0 obj << /Type /Page /Parent 3 0 R >> endobj\n3 0 obj << /Type /Pages /Resources << >> >> endobj";
    let capacity = page_font_capacity(source);
    let target = budget(capacity - 1);
    assert!(parse_page_font_resources(&target, source).is_empty());
    assert!(matches!(target.check(), Err(ParseError::ResourceLimit {
        resource: "metadata workspace bytes", actual, ..
    }) if actual == capacity));
}

/// inline/間接Fontが同じCMap streamを参照する互換入力を作る。
fn font_cmap_pdf(inline: bool, generation: u16) -> Vec<u8> {
    let dictionary = format!(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 4 {generation} R >>"
    );
    let font = if inline {
        dictionary.clone()
    } else {
        "3 0 R".to_owned()
    };
    let cmap = "1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<41> <6587>\nendbfchar\n";
    format!("1 0 obj << /Type /Page /Resources << /Font << /F {font} >> >> >> endobj\n3 0 obj {dictionary} endobj\n4 0 obj << /Length {} >> stream\n{cmap}\nendstream endobj\n", cmap.len()).into_bytes()
}

/// inline FontのToUnicodeは索引の世代と一致する場合だけ採用する。
#[test]
fn inline_tounicode_generation_matches_index() {
    for generation in [0, 1] {
        let source = font_cmap_pdf(true, generation);
        let target = DecodeBudget::default();
        let pages = parse_page_font_resources(&target, &source);
        assert!(target.check().is_ok());
        assert_eq!(pages[0].cmaps.get("F").is_some(), generation == 0);
    }
}

/// 間接FontのToUnicodeも索引の世代が異なれば採用しない。
#[test]
fn indirect_tounicode_generation_matches_index() {
    for generation in [0, 1] {
        let source = font_cmap_pdf(false, generation);
        let target = DecodeBudget::default();
        let pages = parse_page_font_resources(&target, &source);
        assert!(target.check().is_ok());
        assert_eq!(pages[0].cmaps.get("F").is_some(), generation == 0);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 親chain長と再継承に応じて全拡張slotの要求容量を累積する。
    #[test]
    fn inherited_font_resources_capacity_model(count in 1u32..30, repeat in 1usize..4) {
        let mut source = String::new();
        for number in 1..=count {
            let body = if number == count { "/Resources << >>".to_owned() }
                else { format!("/Parent {} 0 R", number + 1) };
            source.push_str(&format!("{number} 0 obj << {body} >> endobj\n"));
        }
        let mut capacity = 4usize; let mut slots = capacity;
        while capacity / 2 < count as usize { capacity *= 2; slots += capacity; }
        let one = (slots * size_of::<Option<u32>>()) as u64;
        for limit in [one * repeat as u64 - 1, one * repeat as u64] {
            let target = budget(limit);
            for attempt in 1..=repeat {
                prop_assert!(definitions(&target, &source, "<< /Parent 1 0 R >>").is_empty());
                let succeeds = attempt < repeat || limit == one * repeat as u64;
                prop_assert_eq!(target.check().is_ok(), succeeds);
            }
        }
    }
}
