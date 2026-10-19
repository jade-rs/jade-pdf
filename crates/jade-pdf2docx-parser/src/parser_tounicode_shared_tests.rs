//! Font 別名・ページ・互換 resource map の完成 CMap 共有を検証する。
use super::*;

use super::page_attribute_fixture as fixture;

/// 元入力の stream 境界を検査して共有可能な解析予算を作る。
fn checked_budget(bytes: &[u8], limits: &ParseLimits) -> DecodeBudget {
    let budget = DecodeBudget::new(limits);
    let streams = crate::structure::validate_structure(
        Path::new("shared-cmap.pdf"),
        bytes,
        9,
        limits,
        &budget,
    )
    .unwrap();
    budget.cache_streams(bytes, &streams);
    budget
}

/// 異なる Font と抽出経路が同じ stream の完成 map を再解析せず共有する。
#[test]
fn checked_font_objects_and_extractors_share_one_stream() {
    let bytes = String::from_utf8(document(true))
        .unwrap()
        .replace("/ToUnicode 13", "/ToUnicode 11")
        .into_bytes();
    let budget = checked_budget(
        &bytes,
        &ParseLimits {
            max_total_tounicode_entries: 1,
            ..Default::default()
        },
    );
    let pages = parse_page_font_resources(&budget, &bytes);
    assert!(budget.check().is_ok());
    assert!(pages[0].cmaps["F"].shares_storage_with(&pages[1].cmaps["Other"]));
    let maps = parse_font_resource_tounicode_cmaps(&budget, &bytes);
    let global = parse_tounicode_cmap(&budget, &bytes);
    let again = parse_tounicode_cmap(&budget, &bytes);
    assert!(budget.check().is_ok());
    assert!(pages[0].cmaps["F"].shares_storage_with(&maps["Other"]));
    assert!(global.shares_storage_with(&pages[0].cmaps["F"]));
    assert!(global.shares_storage_with(&again));
}

/// 直接 Font 辞書の別名・別ページも参照先 stream の完成 map を共有する。
#[test]
fn checked_inline_fonts_share_one_stream() {
    let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Custom /ToUnicode 10 0 R >>";
    let mut objects = fixture::pages(&format!(
        "/MediaBox [0 0 100 100] /Resources << /Font << /F {font} /Alias {font} >> >>"
    ));
    objects.push(fixture::stream("1 beginbfchar\n<01> <0041>\nendbfchar"));
    let bytes = fixture::pdf(&objects);
    let budget = checked_budget(
        &bytes,
        &ParseLimits {
            max_total_tounicode_entries: 1,
            ..Default::default()
        },
    );
    let pages = parse_page_font_resources(&budget, &bytes);
    assert!(pages[0].cmaps["F"].shares_storage_with(&pages[0].cmaps["Alias"]));
    assert!(pages[0].cmaps["F"].shares_storage_with(&pages[1].cmaps["F"]));
    assert!(pages[0].cmaps["F"].shares_storage_with(&parse_tounicode_cmap(&budget, &bytes)));
    assert!(budget.check().is_ok());
}

/// 文書全体の集約も二回目は payload と保存領域を増やさない。
#[test]
fn checked_global_aggregate_is_shared_without_reparsing() {
    let bytes = document(true);
    let budget = checked_budget(
        &bytes,
        &ParseLimits {
            max_total_tounicode_entries: 2,
            ..Default::default()
        },
    );
    let first = parse_tounicode_cmap(&budget, &bytes);
    assert_eq!(first.bmp_scalar_mappings(), vec![(1, 66)]);
    let second = parse_tounicode_cmap(&budget, &bytes);
    assert!(budget.check().is_ok());
    assert!(first.shares_storage_with(&second));
}

/// 別 slice と再検査した元 buffer は以前の文書の cache を採用しない。
#[test]
fn checked_cache_is_source_scoped_and_revalidation_clears_it() {
    let mut bytes = document(false);
    let budget = checked_budget(&bytes, &ParseLimits::default());
    let first = parse_tounicode_cmap(&budget, &bytes);
    let other = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("<0042>", "<0043>")
        .into_bytes();
    let second = parse_tounicode_cmap(&budget, &other);
    assert_eq!(second.bmp_scalar_mappings(), vec![(1, 67)]);
    assert_eq!(
        parse_tounicode_cmap(&budget, &bytes).bmp_scalar_mappings(),
        vec![(1, 66)]
    );
    bytes.copy_from_slice(&other);
    let streams = crate::structure::validate_structure(
        Path::new("rechecked.pdf"),
        &bytes,
        9,
        &ParseLimits::default(),
        &budget,
    )
    .unwrap();
    budget.cache_streams(&bytes, &streams);
    let third = parse_tounicode_cmap(&budget, &bytes);
    assert_eq!(third.bmp_scalar_mappings(), vec![(1, 67)]);
    assert!(!first.shares_storage_with(&third));
    assert!(budget.check().is_ok());
}

/// cache hit も停止後は結果を返さず、エラー取得後も再開しない。
#[test]
fn checked_cache_does_not_bypass_stopped_budget() {
    let bytes = document(false);
    let budget = checked_budget(&bytes, &ParseLimits::default());
    assert!(!parse_tounicode_cmap(&budget, &bytes).is_empty());
    budget.invalid_tounicode("test failure");
    assert!(budget.check().is_err());
    assert!(parse_tounicode_cmap(&budget, &bytes).is_empty());
    assert!(crate::text::parse_tounicode_stream(&budget, &bytes, 11).is_none());
    assert!(budget.check().is_ok());
}

/// mapping のない stream も入力予算を反復消費しない。
#[test]
fn checked_empty_stream_is_cached() {
    let bytes = document(false);
    let budget = checked_budget(&bytes, &ParseLimits::default());
    let (_, payload) = budget
        .stream_index(&bytes)
        .unwrap()
        .for_object(&bytes, 7)
        .unwrap();
    let limited = checked_budget(
        &bytes,
        &ParseLimits {
            max_total_tounicode_input_bytes: payload.len() as u64,
            ..Default::default()
        },
    );
    for _ in 0..8 {
        assert!(
            crate::text::parse_tounicode_stream(&limited, &bytes, 7)
                .unwrap()
                .is_empty()
        );
    }
    assert!(limited.check().is_ok());
}

/// CMap を持たない検査済み文書は選択走査を繰り返さない。
#[test]
fn checked_document_without_cmap_is_cached() {
    let bytes = fixture::pdf(&fixture::pages("/MediaBox [0 0 100 100]"));
    let budget = checked_budget(
        &bytes,
        &ParseLimits {
            max_total_tounicode_input_bytes: bytes.len() as u64,
            ..Default::default()
        },
    );
    for _ in 0..8 {
        assert!(parse_tounicode_cmap(&budget, &bytes).is_empty());
    }
    assert!(budget.check().is_ok());
}

/// 圧縮 CMap は Font と全体集約をまたいでも一度分の展開予算で処理する。
#[test]
fn checked_compressed_stream_is_decoded_once() {
    use std::io::Write;
    let body = b"1 beginbfchar\n<01> <0041>\nendbfchar";
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(body).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut bytes = format!("%PDF-1.7\n1 0 obj << /Type /Font /Subtype /Type1 /ToUnicode 2 0 R >> endobj\n2 0 obj << /Filter /FlateDecode /Length {} >>\nstream\n", compressed.len()).into_bytes();
    bytes.extend_from_slice(&compressed);
    bytes.extend_from_slice(b"\nendstream\nendobj\n%%EOF\n");
    let budget = checked_budget(
        &bytes,
        &ParseLimits {
            max_total_decoded_bytes: body.len() as u64,
            max_total_tounicode_entries: 1,
            ..Default::default()
        },
    );
    let offsets = stream_object_offsets(&budget, &bytes);
    let first = font_cmap_for_object_number(&budget, &bytes, 2, &offsets).unwrap();
    let global = parse_tounicode_cmap(&budget, &bytes);
    let again = font_cmap_for_object_number(&budget, &bytes, 2, &offsets).unwrap();
    assert!(first.shares_storage_with(&global));
    assert!(first.shares_storage_with(&again));
    assert!(budget.check().is_ok());
}

/// 文書全体の互換 Filter 方針で作った cache は Font の未知 Filter 拒否を迂回しない。
#[test]
fn checked_global_cache_preserves_font_filter_policy() {
    let mut objects =
        fixture::pages("/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> >>");
    objects.push("<< /Type /Font /Subtype /Type1 /ToUnicode 11 0 R >>".into());
    objects.push(
        fixture::stream("1 beginbfchar\n<01> <0041>\nendbfchar").replacen(
            "<<",
            "<< /Filter /Unknown",
            1,
        ),
    );
    let bytes = fixture::pdf(&objects);
    let budget = checked_budget(&bytes, &ParseLimits::default());
    assert_eq!(
        parse_tounicode_cmap(&budget, &bytes).bmp_scalar_mappings(),
        vec![(1, 65)]
    );
    assert!(
        parse_page_font_resources(&budget, &bytes)[0]
            .cmaps
            .is_empty()
    );
    assert!(budget.check().is_ok());
}

/// 複数 stream の集約用 String も payload 予算を確保前に消費する。
#[test]
fn checked_aggregate_copy_obeys_payload_budget() {
    let bytes = document(true);
    let budget = checked_budget(
        &bytes,
        &ParseLimits {
            max_total_tounicode_bytes: 6,
            ..Default::default()
        },
    );
    parse_tounicode_cmap(&budget, &bytes);
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "ToUnicode mapping bytes",
            limit: 6,
            actual: 9
        })
    ));
    assert!(budget.cached_global_cmap(&bytes).is_none());
}

/// 二つ目のstreamで停止した集約は先行mapを返さずcacheにも公開しない。
#[test]
fn checked_aggregate_stops_before_returning_pending_map() {
    let bytes = document(true);
    let budget = checked_budget(
        &bytes,
        &ParseLimits {
            max_total_tounicode_entries: 1,
            ..Default::default()
        },
    );
    assert!(parse_tounicode_cmap(&budget, &bytes).is_empty());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "ToUnicode entries",
            limit: 1,
            actual: 2,
        })
    ));
    assert!(budget.cached_global_cmap(&bytes).is_none());
    assert!(parse_tounicode_cmap(&budget, &bytes).is_empty());
}

/// Font 経由でも未圧縮 CMap の本文を入力から借用する。
#[test]
fn font_cmap_payload_borrows_input() {
    let bytes = document(false);
    let budget = DecodeBudget::default();
    let offsets = stream_object_offsets(&budget, &bytes);
    let payload = cmap_payload_for_object_number(&budget, &bytes, 11, &offsets).unwrap();
    let (_, expected) = extract_stream_for_object(&budget, &bytes, 11).unwrap();
    assert_eq!(payload.as_ptr(), expected.as_ptr());
    assert_eq!(payload.as_ref(), expected);
}

/// 停止した解析はエラー取得後も未圧縮 CMap の選択を再開しない。
#[test]
fn stopped_font_cmap_payload_is_unavailable() {
    let bytes = document(false);
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_tounicode_input_bytes: 0,
        ..Default::default()
    });
    let offsets = stream_object_offsets(&budget, &bytes);
    assert!(budget.charge_tounicode_input_bytes(1).is_none());
    assert!(cmap_payload_for_object_number(&budget, &bytes, 11, &offsets).is_none());
    assert!(budget.check().is_err());
    assert!(cmap_payload_for_object_number(&budget, &bytes, 11, &offsets).is_none());
    assert!(budget.check().is_ok());
}

/// 正式な二ページと、異なる局所名から参照する同一 Font/CMap を作る。
fn document(other_cmap: bool) -> Vec<u8> {
    let mut objects = fixture::pages(
        "/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R /Alias 10 0 R >> >>",
    );
    objects[3] = format!(
        "<< /Type /Page /Parent 2 0 R /Contents 6 0 R /Resources << /Font << /Other {} 0 R >> >> >>",
        if other_cmap { 12 } else { 10 }
    );
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Custom /ToUnicode 11 0 R >>".into());
    objects.push(fixture::stream("1 beginbfchar\n<01> <0041>\nendbfchar"));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Other /ToUnicode 13 0 R >>".into());
    objects.push(fixture::stream("1 beginbfchar\n<01> <0042>\nendbfchar"));
    fixture::pdf(&objects)
}

/// 同一 Font の別名と異なるページ名は map node と payload を共有する。
#[test]
fn page_aliases_share_complete_cmap_storage() {
    let budget = DecodeBudget::default();
    let pages = parse_page_font_resources(&budget, &document(false));
    assert_eq!(pages.len(), 2);
    assert!(pages[0].cmaps["F"].shares_storage_with(&pages[0].cmaps["Alias"]));
    assert!(pages[0].cmaps["F"].shares_storage_with(&pages[1].cmaps["Other"]));
    assert!(budget.check().is_ok());
}

/// 互換 resource map への別名登録 clone は深い複製をしない。
#[test]
fn compatibility_aliases_share_complete_cmap_storage() {
    let budget = DecodeBudget::default();
    let maps = parse_font_resource_tounicode_cmaps(&budget, &document(false));
    assert!(maps["F"].shares_storage_with(&maps["Alias"]));
    assert!(maps["F"].shares_storage_with(&maps["Other"]));
    assert!(budget.check().is_ok());
}

/// 別 object と別文書の同じ resource 名は誤った Unicode map を再利用しない。
#[test]
fn distinct_objects_and_documents_keep_separate_cmaps() {
    let budget = DecodeBudget::default();
    let first = parse_page_font_resources(&budget, &document(true));
    let second = parse_page_font_resources(&DecodeBudget::default(), &document(false));
    assert!(!first[0].cmaps["F"].shares_storage_with(&first[1].cmaps["Other"]));
    assert!(!first[0].cmaps["F"].shares_storage_with(&second[0].cmaps["F"]));
    assert_eq!(first[0].cmaps["F"].bmp_scalar_mappings(), vec![(1, 65)]);
    assert_eq!(first[1].cmaps["Other"].bmp_scalar_mappings(), vec![(1, 66)]);
    assert!(budget.check().is_ok());
}
