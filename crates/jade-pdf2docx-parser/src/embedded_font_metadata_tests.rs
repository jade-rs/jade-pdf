//! 埋め込みFontの名前・候補・出力配列の予算外保存を再現する。
use super::*;

/// installable embeddingを持ち、実装とは独立した最小sfnt fixtureを作る。
fn allowed_font() -> Vec<u8> {
    let mut bytes = vec![0; 38];
    bytes[..4].copy_from_slice(&[0, 1, 0, 0]);
    bytes[4..6].copy_from_slice(&1u16.to_be_bytes());
    bytes[12..16].copy_from_slice(b"OS/2");
    bytes[20..24].copy_from_slice(&28u32.to_be_bytes());
    bytes[24..28].copy_from_slice(&10u32.to_be_bytes());
    bytes
}

/// 生Font streamと最外Descriptorを一つだけ持つRootなしfixtureを作る。
fn fixture() -> Vec<u8> {
    let font = allowed_font();
    let mut bytes = format!("%PDF-1.7\n1 0 obj << /Type /FontDescriptor /FontName /TestFont /FontFile2 2 0 R >> endobj\n2 0 obj << /Length {} >>\nstream\n", font.len()).into_bytes();
    bytes.extend_from_slice(&font);
    bytes.extend_from_slice(b"\nendstream\nendobj\n%%EOF\n");
    bytes
}

/// 埋め込みFontの保存より前に必要な構造索引の容量境界だけを探す。
fn index_capacity(bytes: &[u8]) -> u64 {
    let mut capacity = 0;
    for _ in 0..16 {
        let target = DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: capacity,
            ..Default::default()
        });
        let parser_source = parser_source_with_compressed_objects(&target, bytes);
        if let Some(source) = crate::metadata_input::text(&target, &parser_source) {
            let offsets = stream_object_offsets(&target, bytes);
            let _ = font_descriptor_tounicode_mappings(&target, bytes, &source, &offsets);
        }
        match target.check() {
            Ok(()) => return capacity,
            Err(ParseError::ResourceLimit {
                resource: "metadata workspace bytes",
                actual,
                ..
            }) if actual > capacity => capacity = actual,
            other => panic!("unexpected index result: {other:?}"),
        }
    }
    panic!("index capacity did not converge");
}

/// 指定のmetadata容量では完成Fontを公開せず、型付き容量エラーを残す。
fn assert_rejected(extra: u64) {
    let bytes = fixture();
    let target = DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: index_capacity(&bytes) + extra,
        ..Default::default()
    });
    assert!(extract_embedded_truetype_fonts(&target, &bytes).is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes",
            ..
        })
    ));
}

/// 構造索引分しか残っていなければFont名の保存を開始しない。
#[test]
fn embedded_font_name_requires_metadata_capacity() {
    assert_rejected(0);
}

/// 名前だけ保存できても候補配列のslotを予算外で確保しない。
#[test]
fn embedded_font_candidates_require_metadata_capacity() {
    assert_rejected("TestFont".len() as u64);
}

/// 候補と名前の容量ちょうどでは最終出力配列を保存できない。
#[test]
fn embedded_font_results_require_metadata_capacity() {
    let slots = 4 * std::mem::size_of::<(String, Vec<u8>, ToUnicodeCMap)>();
    assert_rejected(("TestFont".len() + slots) as u64);
}
