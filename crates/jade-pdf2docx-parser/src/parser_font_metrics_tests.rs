//! Font 幅表のページ間共有と、object・文書間の分離を検証する。
use super::{DecodeBudget, PageFontResources, parse_page_font_resources};
use std::sync::Arc;

use super::page_attribute_fixture as fixture;

/// 二ページと同一 CID Font object を持つ正式な PDF を作る。
fn document(width: u32, second_resources: Option<&str>) -> Vec<u8> {
    let mut objects = fixture::pages(
        "/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R /Alias 10 0 R >> >>",
    );
    if let Some(resources) = second_resources {
        objects[3] =
            format!("<< /Type /Page /Parent 2 0 R /Contents 6 0 R /Resources {resources} >>");
    }
    objects.push(
        "<< /Type /Font /Subtype /Type0 /Encoding /Identity-H /DescendantFonts [11 0 R] >>".into(),
    );
    objects.push(format!(
        "<< /Type /Font /Subtype /CIDFontType2 /W [65 [{width}]] >>"
    ));
    objects.push(
        "<< /Type /Font /Subtype /Type0 /Encoding /Identity-H /DescendantFonts [13 0 R] >>".into(),
    );
    objects.push("<< /Type /Font /Subtype /CIDFontType2 /W [65 [900]] >>".into());
    fixture::pdf(&objects)
}

/// 独立した decode budget でページ Font resource を解決する。
fn resources(bytes: &[u8]) -> Vec<PageFontResources> {
    let budget = DecodeBudget::new(&crate::ParseLimits::default());
    parse_page_font_resources(&budget, bytes)
}

/// 同じ Font の別名は幅表の読み直しと allocation を発生させない。
#[test]
fn font_aliases_share_metrics() {
    let pages = resources(&document(600, None));
    assert_eq!(pages.len(), 2);
    assert!(Arc::ptr_eq(
        &pages[0].metrics["F"],
        &pages[0].metrics["Alias"]
    ));
}

/// 異なる局所 Resources からも同じ間接 Font の幅表を共有する。
#[test]
fn pages_with_distinct_resources_share_font_metrics() {
    let pages = resources(&document(600, Some("<< /Font << /Other 10 0 R >> >>")));
    assert_eq!(pages.len(), 2);
    assert!(Arc::ptr_eq(
        &pages[0].metrics["F"],
        &pages[1].metrics["Other"]
    ));
}

/// 同じ resource 名でも別 Font object の幅を混ぜない。
#[test]
fn same_resource_name_does_not_share_distinct_fonts() {
    let pages = resources(&document(600, Some("<< /Font << /F 12 0 R >> >>")));
    assert!(!Arc::ptr_eq(&pages[0].metrics["F"], &pages[1].metrics["F"]));
    assert_eq!(
        pages[0].metrics["F"]
            .layout(&DecodeBudget::default(), b"<0041>", 10., 0., 0.)
            .unwrap()
            .advance,
        6.
    );
    assert_eq!(
        pages[1].metrics["F"]
            .layout(&DecodeBudget::default(), b"<0041>", 10., 0., 0.)
            .unwrap()
            .advance,
        9.
    );
}

/// 別文書の同じ object number に以前の幅表を再利用しない。
#[test]
fn separate_documents_do_not_share_metrics() {
    let first = resources(&document(600, None));
    let second = resources(&document(900, None));
    assert!(!Arc::ptr_eq(
        &first[0].metrics["F"],
        &second[0].metrics["F"]
    ));
    assert_eq!(
        first[0].metrics["F"]
            .layout(&DecodeBudget::default(), b"<0041>", 10., 0., 0.)
            .unwrap()
            .advance,
        6.
    );
    assert_eq!(
        second[0].metrics["F"]
            .layout(&DecodeBudget::default(), b"<0041>", 10., 0., 0.)
            .unwrap()
            .advance,
        9.
    );
}

/// 同名の直接 Font 辞書を indirect object の cache と混同しない。
#[test]
fn inline_fonts_do_not_reuse_indirect_metrics() {
    let pages = resources(&document(
        600,
        Some("<< /Font << /F << /Type /Font /Subtype /Type1 /BaseFont /Courier >> >> >>"),
    ));
    assert!(!Arc::ptr_eq(&pages[0].metrics["F"], &pages[1].metrics["F"]));
    assert_eq!(
        pages[1].metrics["F"]
            .layout(&DecodeBudget::default(), b"(A)", 10., 0., 0.)
            .unwrap()
            .advance,
        6.
    );
}

/// 古い generation の参照は、同じ number の有効な幅表を採用しない。
#[test]
fn stale_font_generation_is_not_a_cache_hit() {
    let pages = resources(&document(600, Some("<< /Font << /F 10 1 R >> >>")));
    assert!(pages[0].metrics.contains_key("F"));
    assert!(!pages[1].metrics.contains_key("F"));
}

/// CID 表に加え、simple Font の幅表も同じ allocation を共有する。
#[test]
fn simple_font_widths_share_metrics_between_pages() {
    let mut objects =
        fixture::pages("/MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> >>");
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Custom /FirstChar 65 /LastChar 65 /Widths [700] >>".into());
    let pages = resources(&fixture::pdf(&objects));
    assert!(Arc::ptr_eq(&pages[0].metrics["F"], &pages[1].metrics["F"]));
    assert_eq!(
        pages[1].metrics["F"]
            .layout(&DecodeBudget::default(), b"(A)", 10., 0., 0.)
            .unwrap()
            .advance,
        7.
    );
}
