//! font名の推測と区別し、公開bytes APIで明示されたsource weightを保持する。
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;
#[path = "support/source_font_weight.rs"]
mod owned;
/// 位置付き本文を読み、重さの保存と文字内容を独立に検査する。
fn weights(entries: &str, scalar: Option<&str>, cid: bool) -> Vec<Option<u16>> {
    let data = owned::source(entries, "Vendor-Regular", scalar, cid);
    let parsed = parse_pdf_bytes("owned-font-weight.pdf", &data).unwrap();
    assert_eq!(
        parsed
            .text_elements
            .iter()
            .map(|t| t.text.as_str())
            .collect::<String>(),
        "Weight"
    );
    parsed.text_elements.iter().map(|t| t.font_weight).collect()
}
/// WidthsのないFontも、FontDescriptorの明示weightを失わない。
#[test]
fn direct_descriptor_weights_survive_without_widths() {
    for value in [1, 320, 400, 500, 700, 828, 1000] {
        assert_eq!(
            weights(&format!("/FontWeight {value}"), None, false),
            vec![Some(value)]
        );
    }
}
/// 数値参照とType0の単一descendant descriptorでも同じ属性を保持する。
#[test]
fn indirect_values_and_cid_descendants_preserve_weight() {
    for cid in [false, true] {
        assert_eq!(
            weights("/FontWeight 9 0 R", Some("700"), cid),
            vec![Some(700)]
        );
        assert_eq!(weights("/FontWeight 500", None, cid), vec![Some(500)]);
    }
}
/// 不正値・曖昧な重複・入れ子の同名値から重さを作らない。
#[test]
fn invalid_ambiguous_and_missing_weights_are_unconfirmed() {
    for value in [
        "0", "-1", "1001", "700.5", "/Bold", "[700]", "null", "9 0 R",
    ] {
        assert_eq!(
            weights(&format!("/FontWeight {value}"), None, false),
            vec![None]
        );
    }
    for entries in [
        "",
        "/Note << /FontWeight 700 >>",
        "/FontWeight 400 /FontWeight 700",
    ] {
        assert_eq!(weights(entries, None, false), vec![None]);
    }
}
proptest! {
 #![proptest_config(ProptestConfig { cases:24, failure_persistence:Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct("tests/production_source_font_weights.proptest-regressions"))), ..ProptestConfig::default() })]
 /// 元数値の範囲内で、参照の有無やsimple/CIDを変えてもweightを同一に返す。
 #[test]
 fn declared_weight_survives_resource_spelling(value in 1u16..1001, indirect in any::<bool>(), cid in any::<bool>()) {
  let scalar=value.to_string();let entries=if indirect{"/FontWeight 9 0 R".to_owned()}else{format!("/FontWeight {value}")};
  prop_assert_eq!(weights(&entries,indirect.then_some(scalar.as_str()),cid),vec![Some(value)]);
 }
}
