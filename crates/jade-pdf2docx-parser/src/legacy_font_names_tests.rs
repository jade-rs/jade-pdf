use super::*;
use crate::{ParseError, ParseLimits, page_attributes::Objects};
use proptest::prelude::*;

/// 一つのUnicode name ID6だけを持つmetadata fixture。描画用Fontではない。
fn font() -> Vec<u8> {
    let text = "OwnedCJK";
    let mut name = Vec::new();
    for value in [0u16, 1, 18, 3, 1, 0x409, 6, 16, 0] {
        name.extend_from_slice(&value.to_be_bytes());
    }
    for unit in text.encode_utf16() {
        name.extend_from_slice(&unit.to_be_bytes());
    }
    let mut bytes = vec![0; 28];
    bytes[..4].copy_from_slice(&[0, 1, 0, 0]);
    bytes[4..6].copy_from_slice(&1u16.to_be_bytes());
    bytes[12..16].copy_from_slice(b"name");
    bytes[20..24].copy_from_slice(&28u32.to_be_bytes());
    bytes[24..28].copy_from_slice(&(name.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&name);
    bytes
}

/// 各byteで切った入力から、途中の名前を公開しない。
#[test]
fn truncated_name_and_directory_do_not_yield_partial_names() {
    let bytes = font();
    for end in 0..bytes.len() {
        assert!(postscript_name(&bytes[..end]).is_none(), "{end}");
    }
    let result = postscript_name(&bytes).unwrap();
    assert_eq!(&result.bytes[..result.len], b"OwnedCJK");
}

/// Format・offset・文字数・文字種・UTF-16の破損を飽和や置換で受け入れない。
#[test]
fn invalid_name_records_are_rejected_without_replacement() {
    for (at, replacement) in [
        (28, vec![0, 2]),
        (32, vec![0xff, 0xff]),
        (42, vec![0, 15]),
        (46, vec![0, b'/']),
        (46, vec![0xd8, 0]),
        (20, vec![0xff, 0xff, 0xff, 0xff]),
        (24, vec![0xff, 0xff, 0xff, 0xff]),
    ] {
        let mut bytes = font();
        bytes[at..at + replacement.len()].copy_from_slice(&replacement);
        assert!(postscript_name(&bytes).is_none(), "offset {at}");
    }
}

/// 出力名の15byteを確保前に検査し、解放後も累積予算を返却しない。
#[test]
fn recovered_display_name_respects_exact_cumulative_metadata_capacity() {
    for capacity in [14, 15, 29, 30] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: capacity,
            ..Default::default()
        });
        let objects = Objects::from([(7, (0, b"<< /Length 0 >>\nstream\nendstream".as_slice()))]);
        let mut resolver = ResourceResolver::new(&objects, &budget);
        let descriptor = b"<< /FontName /ABCDEF+#ff /FontFile2 7 0 R >>";
        for pass in 1..=2 {
            let result = descriptor_payload(&budget, descriptor, &mut resolver, |_| Some(font()));
            assert_eq!(result.is_some(), pass * 15 <= capacity);
            if let Some((name, _)) = result {
                assert_eq!(name, "ABCDEF+OwnedCJK");
            }
        }
        let error = budget.check();
        assert_eq!(error.is_ok(), capacity == 30);
        if capacity < 30 {
            assert!(
                matches!(
                    error,
                    Err(ParseError::ResourceLimit {
                        resource: "metadata workspace bytes",
                        ..
                    })
                ),
                "capacity {capacity}: {error:?}"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意の壊れたsfntでもpanicせず、採用名は有界な許可ASCIIだけになる。
    #[test]
    fn arbitrary_program_metadata_is_bounded(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        if let Some(name) = postscript_name(&bytes) {
            prop_assert!((1..=63).contains(&name.len));
            let allowed = name.bytes[..name.len].iter().all(|b| (33..=126).contains(b) && !b"[](){}<>/%".contains(b));
            prop_assert!(allowed);
        }
    }
}
