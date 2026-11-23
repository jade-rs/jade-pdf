//! 所有headerの静的metadata・指定の有無・誤定義・予算境界を検査する。
use super::*;
use crate::{ParseCancellation, ParseError, ParseLimits};
use proptest::prelude::*;

/// 原本のようにFontInfoとEncodingを独立した辞書として宣言する。
fn source(info: &str) -> String {
    format!(
        "%!PS-AdobeFont-1.0: Owned 1.0\n11 dict begin /FontType 1 def /FontName /Owned def /FontMatrix [.001 0 0 .001 0 0] readonly def /FontBBox {{-10 -20 600 700}} readonly def /PaintType 0 def /FontInfo 16 dict dup begin {info} end readonly def /Encoding 256 array 0 1 255 {{1 index exch /.notdef put}} for dup 65 /A put readonly def currentdict end currentfile eexec\n"
    )
}

/// 属性を読み、元のNoticeを省略せず借用し、Encodingのglyph名も保持する。
#[test]
fn static_metadata_and_encoding_are_preserved() {
    let bytes = source(
        r"/Notice (Copyright \050c\051 Owner) readonly def /Weight (Bold) def /ItalicAngle -12.5 def /isFixedPitch false def /UnderlinePosition -100 def /UnderlineThickness 50 def /FSType 8 def /ascent 750 def",
    );
    let actual = parse(&DecodeBudget::default(), bytes.as_bytes()).unwrap();
    assert_eq!(actual.name, b"Owned");
    assert_eq!(actual.matrix, [0.001, 0., 0., 0.001, 0., 0.]);
    assert_eq!(actual.bbox, [-10., -20., 600., 700.]);
    assert_eq!(actual.encoding.slots[65], Some(b"A".as_slice()));
    assert_eq!(
        actual.info.notice,
        Some(br"Copyright \050c\051 Owner".as_slice())
    );
    assert_eq!(actual.info.weight, Some(b"Bold".as_slice()));
    assert_eq!(actual.info.italic_angle, Some(-12.5));
    assert_eq!(actual.info.fixed, Some(false));
    assert_eq!(actual.info.underline_position, Some(-100.));
    assert_eq!(actual.info.underline_thickness, Some(50.));
    assert_eq!(actual.info.fs_type, Some(8));
    assert_eq!(actual.info.ascent, Some(750.));
    let plain = source("");
    assert_eq!(
        parse(&DecodeBudget::default(), plain.as_bytes())
            .unwrap()
            .info
            .fs_type,
        None
    );
    let explicit = source("/FSType 0 def");
    assert_eq!(
        parse(&DecodeBudget::default(), explicit.as_bytes())
            .unwrap()
            .info
            .fs_type,
        Some(0)
    );
    let (owned, length, _, _) = crate::type1_embedding_tests::program();
    assert_eq!(
        parse(&DecodeBudget::default(), &owned[..length])
            .unwrap()
            .name,
        b"JadeOwnedType1"
    );
}

/// commentや文字列の偽属性を採用せず、動的式・重複・暗号化境界の破損を拒否する。
#[test]
fn fake_or_dynamic_metadata_is_not_trusted() {
    let good = source("% /FSType 0 def\n/Notice (nested (text) /FSType 0 def) def /FSType 2 def");
    let actual = parse(&DecodeBudget::default(), good.as_bytes()).unwrap();
    assert_eq!(actual.info.fs_type, Some(2));
    for bad in [
        source("/FSType 0 def /FSType 2 def"),
        source("/FSType 2 2 sub def"),
        source("/FSType NaN def"),
        source("/ItalicAngle NaN def"),
        source("/UnderlineThickness -1 def"),
        source("/FSType 65536 def"),
        source("/Notice (unclosed def"),
        source("/Unknown {0} exec def"),
        source("/FSType {0} def"),
        source("").replace("currentfile eexec", "currentfile exec"),
        source("").replace("/FontType 1 def", "/FontType 3 def"),
        source("").replace("/PaintType 0 def", "/PaintType 2 def"),
        source("").replace(".001 0 0 .001", "0 0 0 0"),
        source("").replace("-10 -20 600 700", "600 -20 -10 700"),
        source("").replace("/Owned def", "/Owned def /FontName /Other def"),
        source("") + "/FSType 0 def",
    ] {
        assert!(
            parse(&DecodeBudget::default(), bad.as_bytes()).is_none(),
            "{bad}"
        );
    }
}

/// 借用文字列内のescape・改行・括弧を保持し、許可のbitを勝手に変更しない。
#[test]
fn raw_strings_and_permission_presence_are_not_normalized() {
    let bytes = source(
        r"/Notice (A\)B\\C\050D\051) def /Copyright (Owner) def /version (1.0) def /FullName (Full) def /FamilyName (Family) def /FSType 258 def",
    );
    let actual = parse(&DecodeBudget::default(), bytes.as_bytes()).unwrap();
    assert_eq!(actual.info.notice, Some(br"A\)B\\C\050D\051".as_slice()));
    assert_eq!(actual.info.copyright, Some(b"Owner".as_slice()));
    assert_eq!(actual.info.version, Some(b"1.0".as_slice()));
    assert_eq!(actual.info.full_name, Some(b"Full".as_slice()));
    assert_eq!(actual.info.family, Some(b"Family".as_slice()));
    assert_eq!(actual.info.fs_type, Some(258));
}

/// Encoding配列だけを予算へ予約し、容量不足と取消で部分情報を渡さない。
#[test]
fn metadata_budget_and_cancellation_are_retained() {
    let bytes = source("");
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_metadata_bytes: 0,
        ..Default::default()
    });
    assert!(parse(&budget, bytes.as_bytes()).is_none());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit { .. })
    ));
    let token = ParseCancellation::new();
    token.cancel();
    let budget = DecodeBudget::with_cancellation(&ParseLimits::default(), Some(&token));
    assert!(parse(&budget, bytes.as_bytes()).is_none());
    assert!(matches!(budget.check(), Err(ParseError::Cancelled)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 任意u16のFSTypeを、許諾判定より前に原本のまま読める。
    #[test]
    fn permission_bits_survive_static_parsing(flags in any::<u16>()) {
        let bytes=source(&format!("/FSType {flags} def"));
        prop_assert_eq!(parse(&DecodeBudget::default(),bytes.as_bytes()).unwrap().info.fs_type,Some(flags));
    }
}
