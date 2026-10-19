//! resource名の借用比較を既存byte対応キーと独立に照合する。
use super::*;
use proptest::prelude::*;

/// escape表記・hexの大小・非UTF-8 byteを同じ名前として照合する。
#[test]
fn escaped_and_binary_resource_names_match() {
    for (left, right) in [
        (&b"F#31"[..], &b"F1"[..]),
        (b"#fF", b"\xff"),
        (b"#00", b"\0"),
        (b"", b""),
    ] {
        assert!(same_resource_name(left, right));
        assert!(valid_resource_name(left));
        assert!(valid_resource_name(right));
    }
    assert!(!same_resource_name(b"F1", b"F10"));
    assert!(!same_resource_name(b"#FF", b"\xc3\xbf"));
}

/// 不正escapeは両辺で同じでも一致させず、末尾まで検査する。
#[test]
fn malformed_resource_names_never_match() {
    for bad in [b"#".as_slice(), b"A#0", b"A#GG", b"A#0x", b"A#ff#"] {
        assert!(!valid_resource_name(bad));
        assert!(!same_resource_name(bad, bad));
        assert!(!same_resource_name(bad, b"A"));
        assert!(!same_resource_name(b"A", bad));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意byte名の比較・検証が従来の復号キーと一致する。
    #[test]
    fn comparison_matches_decoded_key_model(left in prop::collection::vec(any::<u8>(), 0..160), right in prop::collection::vec(any::<u8>(), 0..160)) {
        let decoded_left = crate::metadata::resource_name(&left);
        let decoded_right = crate::metadata::resource_name(&right);
        prop_assert_eq!(valid_resource_name(&left), decoded_left.is_some());
        prop_assert_eq!(same_resource_name(&left, &right), decoded_left.is_some() && decoded_left == decoded_right);
        prop_assert_eq!(same_resource_name(&left, &left), decoded_left.is_some());
    }
}
