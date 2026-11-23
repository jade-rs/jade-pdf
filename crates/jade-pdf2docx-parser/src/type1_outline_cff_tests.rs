//! 所有glyphだけで、CFF1 containerの識別・notice・保存量を検査する。
use super::*;
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 検証用の所有metadataを、借用glyph配列と組み合わせる。
fn font<'a>(glyphs: &'a [Entry<'a>]) -> Font<'a> {
    Font {
        name: b"JadeOwnedCFF",
        notice: b"Copyright Jade owned; permission retained",
        bbox: [0, 0, 600, 700],
        italic_angle: -9.46,
        is_fixed_pitch: true,
        glyphs,
    }
}

/// 標準SIDとcustom名を混同せず、標準名をString INDEXへ重複追加しない。
#[test]
fn standard_string_ids_match_the_cff_specification() {
    for (name, id) in [
        (b".notdef".as_slice(), 0),
        (b"A", 34),
        (b"fi", 109),
        (b"Semibold", 390),
    ] {
        assert_eq!(standard_sid(name), Some(id));
    }
    assert_eq!(standard_sid(b"JadeOwnedCFF"), None);
}

/// 完成した空glyphでも、notice・font名・Private幅があるCFFにする。
#[test]
fn cff_header_names_notice_and_width_private_are_preserved() {
    let glyphs = [
        Entry {
            name: b".notdef",
            program: &[28, 130, 88, 14],
        },
        Entry {
            name: b"A",
            program: &[28, 130, 88, 139, 139, 21, 239, 139, 5, 14],
        },
    ];
    let font = font(&glyphs);
    let bytes = encode(&DecodeBudget::default(), &font).unwrap();
    assert_eq!(bytes.get(..4), Some([1, 0, 4, 4].as_slice()));
    assert!(bytes.windows(font.name.len()).any(|s| s == font.name));
    assert!(bytes.windows(font.notice.len()).any(|s| s == font.notice));
    assert_eq!(bytes, encode(&DecodeBudget::default(), &font).unwrap());
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert!(encode(&budget, &font).is_none());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "embedded font bytes",
            ..
        })
    ));
}

/// 二重名・空program・不正なname/metadataを、黙って別glyphへ変えない。
#[test]
fn invalid_cff_identity_or_metadata_is_rejected() {
    for glyphs in [
        vec![],
        vec![Entry {
            name: b"A",
            program: &[14],
        }],
        vec![Entry {
            name: b".notdef",
            program: &[],
        }],
        vec![
            Entry {
                name: b".notdef",
                program: &[14],
            },
            Entry {
                name: b".notdef",
                program: &[14],
            },
        ],
    ] {
        assert!(encode(&DecodeBudget::default(), &font(&glyphs)).is_none());
    }
    let glyphs = [Entry {
        name: b".notdef",
        program: &[14],
    }];
    let mut owned = font(&glyphs);
    owned.name = b"bad name";
    assert!(encode(&DecodeBudget::default(), &owned).is_none());
    owned.name = b"JadeOwnedCFF";
    owned.italic_angle = f64::NAN;
    assert!(encode(&DecodeBudget::default(), &owned).is_none());
    owned.italic_angle = 0.;
    owned.bbox = [100, 0, 0, 700];
    assert!(encode(&DecodeBudget::default(), &owned).is_none());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// noticeの大きさと傾斜値に関係なく、完成CFFは上限内で決定的に再現できる。
    #[test]
    fn cff_container_is_deterministic(notice in proptest::collection::vec(32_u8..127,0..512),angle in -20_i32..20) {
        let glyphs=[Entry { name:b".notdef",program:&[28,130,88,14] }];
        let mut owned=font(&glyphs);owned.notice=&notice;owned.italic_angle=f64::from(angle)/3.;
        let first=encode(&DecodeBudget::default(),&owned).unwrap();
        prop_assert!(first.len()<=crate::parser::MAX_EMBEDDED_FONT_BYTES);
        prop_assert_eq!(&first,&encode(&DecodeBudget::default(),&owned).unwrap());
    }
}
