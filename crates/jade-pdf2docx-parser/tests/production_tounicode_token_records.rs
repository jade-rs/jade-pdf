//! CMapの意味を、改行や一行内のentry数に依存させない。
use jade_pdf2docx_parser::parse_pdf_bytes;
use proptest::prelude::*;
#[path = "support/inline_cmap_source.rs"]
mod owned;
/// 公開bytes APIの文字列を、制御文字を除かずそのまま照合する。
fn text(cmap: &str, encoded: &str) -> String {
    let pdf = owned::mapped(cmap, encoded);
    parse_pdf_bytes("owned-inline-cmap.pdf", &pdf)
        .unwrap()
        .text_elements
        .iter()
        .map(|t| t.text.as_str())
        .collect()
}
/// 期待値を変えず、元の一行CID CMapからNULを混ぜずに同じ六文字を復元する。
#[test]
fn single_line_cmap_decodes_weight_without_nul_prefixes() {
    assert_eq!(text(owned::CMAP, "<005700650069006700680074>"), "Weight");
}
/// 一行の複数pairと複数行のarrayを、同じtoken列として読む。
#[test]
fn multiple_pairs_and_multiline_destination_arrays_are_decoded() {
    for cmap in [
        "1 begincodespacerange <0000> <FFFF> endcodespacerange 3 beginbfchar <0001> <0041> <0002> <0042> <0003> <0043> endbfchar",
        "1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n1 beginbfrange\n<0001> <0003> [<0041>\n<0042> <0043>]\nendbfrange",
    ] {
        assert_eq!(text(cmap, "<000100020003>"), "ABC");
    }
}
/// コメント・literal string・PDF nameのoperator風文字列でstateやmappingを変えない。
#[test]
fn quoted_and_commented_keywords_do_not_create_fake_mappings() {
    let source = "1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n1 beginbfchar\n<0057> <0057>\nendbfchar\n% beginbfchar\n(<0057> <0058>) pop\n% endbfchar\n/beginbfchar\n(<0057> <0058>) pop\n/endbfchar";
    assert_eq!(text(source, "<0057>"), "W");
}
/// 同じ保存前の所有bytesを独立SDKで読み、一行CMap自体の期待文字を確認する。
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn independent_native_reader_confirms_original_single_line_fixture() {
    use pdfium_render::prelude::*;
    let bytes = owned::source("/FontWeight 700", "Vendor-Regular", None, true);
    let native = if let Some(path) = std::env::var_os("PDFIUM_LIB_PATH") {
        Pdfium::new(Pdfium::bind_to_library(std::path::PathBuf::from(path)).unwrap())
    } else {
        pdfium_auto::bind_pdfium_silent().unwrap()
    };
    let doc = native.load_pdf_from_byte_slice(&bytes, None).unwrap();
    let page = doc.pages().get(0).unwrap();
    let text = page.text().unwrap();
    assert_eq!(text.all().trim(), "Weight");
}
proptest! {
 #![proptest_config(ProptestConfig { cases:24, failure_persistence:Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct("tests/production_tounicode_token_records.proptest-regressions"))), ..ProptestConfig::default() })]
 /// 同じ意味のCMapで、operand間のPDF空白を変えても本文を変えない。
 #[test]
 fn source_text_is_invariant_under_token_whitespace(style in 0usize..5) {
  let separators=[" ","\t","\n","\r\n","\x0c"];
  let cmap=owned::CMAP.split_whitespace().collect::<Vec<_>>().join(separators[style]);
  prop_assert_eq!(text(&cmap,"<005700650069006700680074>"),"Weight");
 }
}
