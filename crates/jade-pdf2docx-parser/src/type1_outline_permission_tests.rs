//! 原本の指定と明示OFL宣言を、権利bitや文字列の改変なしで検査する。
use super::*;
use crate::{ParseCancellation, ParseError, ParseLimits};
use proptest::prelude::*;

/// OFL宣言のある所有sourceを、指定したPostScript改行で作る。
fn ofl_source(eol: &str) -> Vec<u8> {
    [
        "%!PS-AdobeFont-1.0: ReservedOwned 1.0",
        "%Copyright Jade owned, with Reserved Font Name ReservedOwned.",
        "% This Font Software is licensed under the SIL Open Font License, Version 1.1.",
        "/FontName /ReservedOwned def",
    ]
    .join(eol)
    .into_bytes()
}

/// 元のFSTypeを欠落・0・editable・subset禁止として区別し、bitを勝手に落とさない。
#[test]
fn explicit_embedding_flags_are_preserved_without_upgrading_unknown_rights() {
    let clear = b"%!PS owned\n/FontType 1 def";
    assert!(matches!(
        read(&DecodeBudget::default(), clear, &Info::default(), false),
        Some(Decision::Unknown)
    ));
    for (flags, subset, allowed) in [
        (0, false, true),
        (8, true, true),
        (0x100, false, true),
        (0x108, false, true),
        (0x100, true, false),
        (0x108, true, false),
        (2, false, false),
        (4, false, false),
        (0x200, false, false),
        (12, false, false),
        (1, false, false),
    ] {
        let info = Info {
            fs_type: Some(flags),
            ..Default::default()
        };
        match read(&DecodeBudget::default(), clear, &info, subset).unwrap() {
            Decision::Allowed(license) => {
                assert!(allowed);
                assert_eq!(license.flags, flags);
                assert_eq!(license.declared_flags, Some(flags));
                assert_eq!(license.basis, Basis::FsType);
                assert_eq!(license.url, "");
            }
            Decision::Prohibited => assert!(!allowed),
            Decision::Unknown => panic!("explicit flags must not become unspecified"),
        }
    }
}

/// 明示OFLを認識したときだけ、元のcopyright/RFNを残してライセンス全文を付ける。
#[test]
fn declared_ofl_keeps_original_notice_and_complete_standard_license() {
    for eol in ["\n", "\r\n", "\r"] {
        let source = ofl_source(eol);
        let info = Info {
            notice: Some(b"Original \\251 notice"),
            ..Default::default()
        };
        let Some(Decision::Allowed(license)) = read(&DecodeBudget::default(), &source, &info, true)
        else {
            panic!("explicit OFL");
        };
        assert_eq!(license.declared_flags, None);
        assert_eq!(license.flags, 0);
        assert_eq!(license.basis, Basis::OpenFontLicense);
        assert!(license.notice.windows(13).any(|b| b == b"ReservedOwned"));
        assert!(
            license
                .notice
                .windows(17)
                .any(|b| b == b"Original \xa9 notice")
        );
        assert!(license.text.contains("Original © notice"));
        assert!(
            license
                .text
                .ends_with(include_str!("../licenses/open-font-license-1.1.txt"))
        );
        assert!(!license.text.contains("<Copyright Holder>"));
        assert_eq!(
            license.url,
            "https://openfontlicense.org/open-font-license-official-text/"
        );
    }
}

/// 任意の実行式・literal・暗号化後に似た文があっても、先頭のライセンス宣言にしない。
#[test]
fn declaration_inside_postscript_program_or_literal_is_not_a_leading_comment() {
    for source in [b"/Notice (text\n% This Font Software is licensed under the SIL Open Font License, Version 1.1.\n) def".as_slice(),b"%!PS owned\n11 dict begin\n% This Font Software is licensed under the SIL Open Font License, Version 1.1.\n"] {
        assert!(matches!(read(&DecodeBudget::default(),source,&Info::default(),false),Some(Decision::Unknown)));
    }
    let source = ofl_source("\n");
    for flags in [2, 4, 0x100, 0x200, 0xffff] {
        let info = Info {
            fs_type: Some(flags),
            ..Default::default()
        };
        assert!(matches!(
            read(&DecodeBudget::default(), &source, &info, true),
            Some(Decision::Prohibited)
        ));
    }
}

/// FontInfo自体の明示宣言も扱い、無関係なfont名だけでOFLを推定しない。
#[test]
fn fontinfo_declaration_is_explicit_and_source_names_are_not_a_license() {
    let statement=b"Copyright Jade owned.\nThis Font Software is licensed under the SIL Open Font License, Version 1.1.";
    let info = Info {
        notice: Some(statement),
        ..Default::default()
    };
    assert!(matches!(
        read(&DecodeBudget::default(), b"/FontType 1 def", &info, true),
        Some(Decision::Allowed(License {
            basis: Basis::OpenFontLicense,
            ..
        }))
    ));
    let info = Info {
        full_name: Some(b"Computer Modern"),
        ..Default::default()
    };
    assert!(matches!(
        read(
            &DecodeBudget::default(),
            b"/FontName /CMR10 def",
            &info,
            true
        ),
        Some(Decision::Unknown)
    ));
}

/// 元byte値と人間向けUTF8の双方を正確に予約し、一byte不足やcancelで部分表示しない。
#[test]
fn notice_storage_is_exact_cumulative_and_cancelled_atomically() {
    let clear = b"% owned\n/FontType 1 def";
    let info = Info {
        fs_type: Some(0),
        notice: Some(b"\\251"),
        copyright: Some(b"Jade"),
        ..Default::default()
    };
    let expected = b"% owned\n\xa9\nJade\n";
    let text = expected.iter().map(|&b| char::from(b)).collect::<String>();
    let total = (1 + 4 + expected.len() + text.len()) as u64;
    for limit in [total, total - 1] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_embedded_font_bytes: limit,
            ..Default::default()
        });
        let result = read(&budget, clear, &info, false);
        if limit == total {
            let Some(Decision::Allowed(license)) = result else {
                panic!("exact budget");
            };
            assert_eq!(license.notice, expected);
            assert_eq!(license.text, text);
            assert!(budget.check().is_ok());
        } else {
            assert!(result.is_none());
            assert!(
                matches!(budget.check(),Err(ParseError::ResourceLimit {resource:"embedded font bytes",limit:l,actual})if l==limit && actual==total)
            );
        }
    }
    let token = ParseCancellation::new();
    token.cancel();
    let stopped = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
    assert!(read(&stopped, clear, &info, false).is_none());
    assert!(matches!(stopped.check(), Err(ParseError::Cancelled)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意のbit列で予約bitやreadonly指定を許可に変えず、有効な元bitを保つ。
    #[test]
    fn arbitrary_flags_do_not_gain_editable_rights(
        flags in prop_oneof![Just(0u16),Just(8),Just(0x100),Just(0x108),any::<u16>()],subset in any::<bool>()
    ) {
        let info=Info {fs_type:Some(flags),..Default::default()};
        let result=read(&DecodeBudget::default(),b"/FontType 1 def",&info,subset).unwrap();
        let expected=flags & !0x0108==0 && !(subset && flags & 0x100!=0);
        match result {
            Decision::Allowed(license)=>{prop_assert!(expected);prop_assert_eq!(license.flags,flags);prop_assert_eq!(license.declared_flags,Some(flags));},
            Decision::Prohibited=>prop_assert!(!expected),
            Decision::Unknown=>prop_assert!(false,"explicit flags were lost"),
        }
    }
}
