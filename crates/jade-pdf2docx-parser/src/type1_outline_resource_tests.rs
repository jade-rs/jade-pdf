//! 所有fontとPDF辞書で、Encoding優先順位・小数幅・世代参照・失敗時の原子性を検査する。
use super::*;
use crate::type1_outline::{charstring::Glyph, file::NamedGlyph, header};
use crate::{ParseCancellation, ParseError, ParseLimits, page_attributes::Objects};
use proptest::prelude::*;

/// 元のA/B対応と、各既知base encodingを区別できる所有glyph集合。
fn font() -> Font<'static> {
    let header=header::parse(&DecodeBudget::default(),b"11 dict begin /FontType 1 def /FontName /Owned def /FontMatrix [.001 0 0 .001 0 0] def /FontBBox [0 0 600 700] def /Encoding 256 array 0 1 255 {1 index exch /.notdef put} for dup 65 /A put dup 66 /B put def currentdict end currentfile eexec").unwrap();
    Font {
        header,
        glyphs: [
            b".notdef".as_slice(),
            b"A",
            b"B",
            b"Euro",
            b"Adieresis",
            b"fi",
            b"space",
            b"quoteright",
            b"quotesingle",
            b"bullet",
            b"oneoldstyle",
        ]
        .iter()
        .map(|name| NamedGlyph {
            name: name.to_vec(),
            outline: Glyph {
                advance: 600.,
                commands: vec![],
            },
        })
        .collect(),
    }
}

/// 間接参照を使わない所有PDF辞書を、共通budgetで読む。
fn direct(budget: &DecodeBudget, dictionary: &[u8]) -> Option<Vec<Slot>> {
    let objects = Objects::default();
    let mut resolver = ResourceResolver::new(&objects, budget);
    read(budget, &font(), dictionary, &mut resolver)
}

/// font内幅よりPDF幅を優先し、codeごとの同じglyphも混ぜずに保持する。
#[test]
fn builtin_names_and_fractional_pdf_widths_take_priority() {
    let actual=direct(&DecodeBudget::default(),b"<< /Subtype /Type1 /FirstChar 65 /LastChar 66 /Widths [799.8 437.5] /FontDescriptor << /MissingWidth 123.25 >> >>").unwrap();
    assert_eq!(actual.len(), 256);
    assert_eq!(actual[65].glyph, Some(1));
    assert_eq!(actual[65].width, 799.8);
    assert_eq!(actual[66].glyph, Some(2));
    assert_eq!(actual[66].width, 437.5);
    assert!(actual[65].explicit_width);
    assert!(!actual[64].explicit_width);
    assert_eq!(actual[64].width, 123.25);
    assert_eq!(actual[64].glyph, Some(0));
    let changed=direct(&DecodeBudget::default(),b"<< /Subtype /Type1 /FirstChar 65 /LastChar 66 /Widths [600 700] /Encoding << /Differences [65 /B /B] >> >>").unwrap();
    assert_eq!(changed[65].glyph, Some(2));
    assert_eq!(changed[66].glyph, Some(2));
    assert_eq!(changed[65].width, 600.);
    assert_eq!(changed[66].width, 700.);
}

/// PDFの各base encodingは既知のglyph名を使い、Differencesはさらに優先する。
#[test]
fn pdf_base_encodings_and_differences_are_distinct() {
    for (encoding, code, gid) in [
        ("StandardEncoding", 39, 7),
        ("WinAnsiEncoding", 39, 8),
        ("WinAnsiEncoding", 128, 3),
        ("WinAnsiEncoding", 129, 9),
        ("MacRomanEncoding", 128, 4),
        ("MacExpertEncoding", 87, 5),
        ("MacExpertEncoding", 49, 10),
    ] {
        let source = format!(
            "<< /Subtype /Type1 /FirstChar {code} /LastChar {code} /Widths [500] /Encoding /{encoding} >>"
        );
        let actual = direct(&DecodeBudget::default(), source.as_bytes()).unwrap();
        assert_eq!(actual[code].glyph, Some(gid), "{encoding} {code}");
    }
    let changed=direct(&DecodeBudget::default(),b"<< /Subtype /Type1 /FirstChar 65 /LastChar 65 /Widths [500] /Encoding << /BaseEncoding /WinAnsiEncoding /Differences [65 /#42] >> >>").unwrap();
    assert_eq!(changed[65].glyph, Some(2));
    let missing=direct(&DecodeBudget::default(),b"<< /Subtype /Type1 /FirstChar 65 /LastChar 65 /Widths [500] /Encoding << /Differences [65 /NotInFont] >> >>").unwrap();
    assert_eq!(missing[65].glyph, None);
}

/// 有効世代の間接値を解決し、壊れた世代・循環を既定値へ置き換えない。
#[test]
fn indirect_resources_preserve_generation_and_exact_numbers() {
    let objects = [
        (1, (2, b"65".as_slice())),
        (2, (0, b"[3 1 R]".as_slice())),
        (3, (1, b"862.3".as_slice())),
        (4, (0, b"<< /Differences [1 2 R /B] >>".as_slice())),
    ]
    .into_iter()
    .collect::<Objects>();
    let budget = DecodeBudget::default();
    let mut resolver = ResourceResolver::new(&objects, &budget);
    let actual = read(
        &budget,
        &font(),
        b"<< /Subtype /Type1 /FirstChar 1 2 R /LastChar 1 2 R /Widths 2 0 R /Encoding 4 0 R >>",
        &mut resolver,
    )
    .unwrap();
    assert_eq!(actual[65].width, 862.3);
    assert_eq!(actual[65].glyph, Some(2));
    assert!(
        read(
            &budget,
            &font(),
            b"<< /Subtype /Type1 /FirstChar 1 1 R /LastChar 65 /Widths [600] >>",
            &mut resolver
        )
        .is_none()
    );
}

/// 不完全・過剰な幅表、未知encoding、壊れたDifferencesや重複値を部分成功へ変えない。
#[test]
fn malformed_mapping_and_widths_fail_atomically() {
    for body in [
        "/Widths [600]",
        "/FirstChar 65 /LastChar 66 /Widths [600]",
        "/FirstChar 65 /LastChar 65 /Widths [600 700]",
        "/FirstChar 256 /LastChar 256 /Widths [600]",
        "/FirstChar 65 /LastChar 65 /Widths [NaN]",
        "/FirstChar 65 /LastChar 65 /Widths [600] /Encoding /Unknown",
        "/FirstChar 65 /LastChar 65 /Widths [600] /Encoding << /Differences [/A] >>",
        "/FirstChar 65 /LastChar 65 /Widths [600] /Encoding << /Differences [256 /A] >>",
        "/FirstChar 65 /LastChar 65 /Widths [600] /Encoding << /Differences [255 /A /B] >>",
        "/FirstChar 65 /LastChar 65 /Widths [600] /FirstChar 66",
        "/FirstChar 65 /LastChar 65 /Widths [600] /FontDescriptor << /MissingWidth NaN >>",
    ] {
        let source = format!("<< /Subtype /Type1 {body} >>");
        assert!(
            direct(&DecodeBudget::default(), source.as_bytes()).is_none(),
            "{body}"
        );
    }
}

/// 保存先のfont容量と取消を共有し、全slotを保存できなければ結果を渡さない。
#[test]
fn storage_and_cancellation_do_not_publish_partial_slots() {
    let source = b"<< /Subtype /Type1 /FirstChar 65 /LastChar 65 /Widths [600] >>";
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert!(direct(&budget, source).is_none());
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
    assert!(direct(&budget, source).is_none());
    assert!(matches!(budget.check(), Err(ParseError::Cancelled)));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 任意codeのDifferencesと小数第一位の幅を、byte番号からglyphを推測せず保持する。
    #[test]
    fn arbitrary_codes_keep_confirmed_glyph_and_decimal_width(code in 0usize..256,tenths in 1u32..12000) {
        let width=f64::from(tenths)/10.;
        let source=format!("<< /Subtype /Type1 /FirstChar {code} /LastChar {code} /Widths [{width}] /Encoding << /Differences [{code} /A] >> >>");
        let actual=direct(&DecodeBudget::default(),source.as_bytes()).unwrap();
        prop_assert_eq!(actual[code].glyph,Some(1));prop_assert_eq!(actual[code].width,width);
    }
}
