//! 正式なnative正規化の後でも、確認済み原本の全文字所属maskをSDKの厳密な所属と照合する。
use super::*;

/// 原本を変更せず、同じコピー許可確認・復号・SDK読み込み後の両ページを比較する。
#[test]
#[ignore = "requires reviewed original and pinned Pdfium; set JADE_JAPANESE_TABLE_HOLDOUT"]
fn reviewed_japanese_source_clip_masks_match_exact_sdk_ownership() {
    let input = std::env::var("JADE_JAPANESE_TABLE_HOLDOUT").unwrap();
    let path = Path::new(&input);
    let bytes = std::fs::read(path).unwrap();
    let hash = hmac_sha256::Hash::hash(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        hash,
        "a74ef38911860b41868e5f1b7d8f870d2ff870ad2dcfd16c637a4e15abfd1e14"
    );
    let limits = ParseLimits::default();
    let normalized = encryption::normalize(path, &bytes, &limits, &DecodeBudget::new(&limits))
        .expect("existing copy-permitted native normalization");
    let outcome = {
        let native = library::bind(path).unwrap();
        let document = native.load_pdf_from_byte_slice(&normalized, None).unwrap();
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_eq!(document.pages().len(), 2);
            for (index, page) in document.pages().iter().enumerate() {
                let expected =
                    crate::pdfium_text::budget_tests::clip_index_tests::native_ownership_reference(
                        &page,
                    );
                let text = page.text().unwrap();
                let actual = crate::pdfium_clip::excluded(
                    &DecodeBudget::new(&limits),
                    page.objects(),
                    &text,
                )
                .unwrap();
                assert_eq!(
                    actual,
                    expected,
                    "original source page{} ownership differs",
                    index + 1
                );
                eprintln!(
                    "source page{}: {} SDK chars; mask{}; excluded{}; exact SDK ownership",
                    index + 1,
                    text.len(),
                    actual.len(),
                    actual.iter().filter(|v| **v).count()
                );
            }
        }))
    };
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
}
