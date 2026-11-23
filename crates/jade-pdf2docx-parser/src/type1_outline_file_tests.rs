//! 所有FontFileの全体経路・区間境界・取消・末尾不変条件を検査する。
use super::*;
use crate::type1_outline::charstring::Command;
use crate::{ParseCancellation, ParseError, ParseLimits};
use proptest::prelude::*;

/// cleartextと二層の復号を一つのproduct関数でつなぎ、元の幅と三角形を得る。
#[test]
fn original_fontfile_reaches_metadata_and_all_outlines() {
    let (bytes, length1, length2, _) = crate::type1_embedding_tests::program();
    let font = read(&DecodeBudget::default(), &bytes, length1, length2).unwrap();
    assert_eq!(font.header.name, b"JadeOwnedType1");
    assert_eq!(font.header.info.fs_type, Some(0));
    assert_eq!(font.header.encoding.slots[65], Some(b"A".as_slice()));
    assert_eq!(font.glyphs.len(), 2);
    assert_eq!(font.glyphs[0].name, b".notdef");
    assert_eq!(font.glyphs[0].outline.advance, 600.);
    assert!(font.glyphs[0].outline.commands.is_empty());
    assert_eq!(font.glyphs[1].name, b"A");
    assert_eq!(font.glyphs[1].outline.advance, 600.);
    assert_eq!(
        font.glyphs[1].outline.commands,
        vec![
            Command::Move([0., 0.]),
            Command::Line([300., 700.]),
            Command::Line([600., 0.]),
            Command::Line([0., 0.]),
            Command::Close
        ]
    );
}

/// 範囲のoverflow・不足と破損した暗号を、成功した空fontへ置き換えない。
#[test]
fn invalid_segments_fail_before_allocation() {
    let (bytes, length1, length2, _) = crate::type1_embedding_tests::program();
    for (a, b) in [
        (usize::MAX, 1),
        (length1, usize::MAX),
        (bytes.len() + 1, 4),
        (length1, 3),
        (0, length2),
    ] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_embedded_font_bytes: 0,
            ..Default::default()
        });
        assert!(read(&budget, &bytes, a, b).is_none());
        assert!(budget.check().is_ok());
    }
    let mut damaged = bytes.clone();
    damaged[length1 + length2 - 1] ^= 128;
    assert!(read(&DecodeBudget::default(), &damaged, length1, length2).is_none());
}

/// 容量不足と取消で全体を停止し、成功したglyphだけを公開しない。
#[test]
fn font_budget_and_cancellation_are_atomic() {
    let (bytes, length1, length2, _) = crate::type1_embedding_tests::program();
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert!(read(&budget, &bytes, length1, length2).is_none());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "embedded font bytes",
            ..
        })
    ));
    let token = ParseCancellation::new();
    token.cancel();
    let budget = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
    assert!(read(&budget, &bytes, length1, length2).is_none());
    assert!(matches!(budget.check(), Err(ParseError::Cancelled)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    /// 宣言した暗号化区間の後へ任意byteを追加しても、元のglyphとEncodingは変化しない。
    #[test]
    fn trailing_bytes_do_not_change_declared_glyphs(trailer in prop::collection::vec(any::<u8>(),0..256)) {
        let (mut bytes,length1,length2,_)=crate::type1_embedding_tests::program();
        bytes.extend_from_slice(&trailer);
        let actual=read(&DecodeBudget::default(),&bytes,length1,length2).unwrap();
        prop_assert_eq!(actual.header.encoding.slots[65],Some(b"A".as_slice()));
        prop_assert_eq!(actual.glyphs.len(),2);prop_assert_eq!(actual.glyphs[1].outline.advance,600.);
        prop_assert_eq!(&actual.glyphs[1].outline.commands[1],&Command::Line([300.,700.]));
    }
}
