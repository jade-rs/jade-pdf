//! 所有のToUnicodeとglyph名で、原文優先・合字・空白・未知対応・code spaceを検査する。
use super::*;
use crate::type1_outline::{charstring::Glyph, file::NamedGlyph, header};
use crate::{ParseCancellation, ParseError, ParseLimits, text::parse_tounicode_body};
use proptest::prelude::*;

/// A・合字・部分的に未知の名前を区別できる、所有の元fontを作る。
fn font() -> Font<'static> {
    let header=header::parse(&DecodeBudget::default(),b"11 dict begin /FontType 1 def /FontName /Owned def /FontMatrix [.001 0 0 .001 0 0] def /FontBBox [0 0 600 700] def /Encoding StandardEncoding def currentdict end currentfile eexec").unwrap();
    Font {
        header,
        glyphs: [
            b".notdef".as_slice(),
            b"A",
            b"fi",
            b"f_f_i",
            b"UnknownName",
            b"f_UnknownName",
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

/// .notdefを持つ未指定codeと、テスト対象の一codeだけを分ける。
fn slots(code: usize, gid: Option<u16>) -> Vec<Slot> {
    let mut slots = vec![
        Slot {
            glyph: Some(0),
            width: 0.,
            explicit_width: false
        };
        256
    ];
    slots[code] = Slot {
        glyph: gid,
        width: 799.8,
        explicit_width: true,
    };
    slots
}

/// Unicode列を比較しやすい文字列へ戻す。未確認はNoneのままにする。
fn text(mapping: &Mapping) -> Option<String> {
    mapping.scalars.as_ref().map(|scalars| {
        scalars
            .iter()
            .map(|&c| char::from_u32(c).unwrap())
            .collect()
    })
}

/// 元ToUnicodeの複数文字と非BMPを保持し、glyph名の互換合字やASCIIへ置換しない。
#[test]
fn original_unicode_sequences_have_priority_over_glyph_names() {
    let original=parse_tounicode_body(&DecodeBudget::default(),b"1 begincodespacerange\n<00> <FF>\nendcodespacerange\n3 beginbfchar\n<41> <03A9>\n<42> <00660069>\n<43> <D801DC00>\nendbfchar");
    let mut source = slots(65, Some(1));
    source[66].glyph = Some(2);
    source[67].glyph = None;
    let result = map(&DecodeBudget::default(), &font(), &source, &original).unwrap();
    assert_eq!(text(&result[65]), Some("Ω".into()));
    assert!(result[65].from_tounicode);
    assert_eq!(result[65].source.width, 799.8);
    assert!(result[65].source.explicit_width);
    assert_eq!(text(&result[66]), Some("fi".into()));
    assert!(result[66].from_tounicode);
    assert_eq!(text(&result[67]), Some("\u{10400}".into()));
    assert_eq!(result[67].source.glyph, None);
}

/// AGL補完は元glyph名に従い、未確認componentから部分Unicodeを返さない。
#[test]
fn glyph_name_fallback_preserves_sequences_and_unknowns() {
    let mut source = slots(1, Some(1));
    for (code, gid) in [(2, 2), (3, 3), (4, 4), (5, 5)] {
        source[code].glyph = Some(gid);
    }
    let result = map(
        &DecodeBudget::default(),
        &font(),
        &source,
        &ToUnicodeCMap::default(),
    )
    .unwrap();
    assert_eq!(text(&result[1]), Some("A".into()));
    assert_eq!(text(&result[2]), Some("\u{FB01}".into()));
    assert_eq!(text(&result[3]), Some("ffi".into()));
    assert_eq!(text(&result[4]), None);
    assert_eq!(text(&result[5]), None);
    assert_eq!(text(&result[0]), None);
    assert!(!result[1].from_tounicode);
}

/// .notdefや未知glyphの明示Unicodeも区別して保持し、空白の輪郭を勝手に作らない。
#[test]
fn empty_mapping_whitespace_and_missing_shapes_remain_distinct() {
    let original = parse_tounicode_body(
        &DecodeBudget::default(),
        b"3 beginbfchar\n<20> <0020>\n<21> <>\n<22> <2423>\nendbfchar",
    );
    let mut source = slots(32, Some(0));
    source[33].glyph = Some(1);
    source[34].glyph = Some(0);
    let result = map(&DecodeBudget::default(), &font(), &source, &original).unwrap();
    assert_eq!(text(&result[32]), Some(" ".into()));
    assert_eq!(result[32].source.glyph, Some(0));
    assert_eq!(text(&result[33]), Some("".into()));
    assert!(result[33].from_tounicode);
    assert_eq!(text(&result[34]), Some("\u{2423}".into()));
    assert_eq!(result[34].source.glyph, Some(0));
}

/// 二byteの別keyやcode spaceの外側を、一byteの明示対応として使わない。
#[test]
fn byte_lookup_respects_code_space_and_key_width() {
    let original=parse_tounicode_body(&DecodeBudget::default(),b"1 begincodespacerange\n<50> <7F>\nendcodespacerange\n3 beginbfchar\n<41> <0042>\n<0041> <0043>\n<50> <0050>\nendbfchar");
    assert_eq!(original.lookup_byte(65), None);
    assert_eq!(original.lookup_byte(80), Some("P"));
    let result = map(
        &DecodeBudget::default(),
        &font(),
        &slots(65, Some(1)),
        &original,
    )
    .unwrap();
    assert_eq!(text(&result[65]), Some("A".into()));
    assert!(!result[65].from_tounicode);
}

/// 空のbfrange宛先を飛ばして後続codeをずらさず、不完全なUTF16を部分採用しない。
#[test]
fn empty_range_targets_do_not_shift_following_unicode() {
    let original = parse_tounicode_body(
        &DecodeBudget::default(),
        b"1 beginbfrange\n<41> <43> [<0041> <> <0043>]\nendbfrange",
    );
    assert_eq!(original.lookup_byte(65), Some("A"));
    assert_eq!(original.lookup_byte(66), Some(""));
    assert_eq!(original.lookup_byte(67), Some("C"));
    let malformed = parse_tounicode_body(
        &DecodeBudget::default(),
        b"2 beginbfchar\n<> <0041>\n<44> <004100>\nendbfchar",
    );
    assert!(malformed.is_empty());
}

/// 保存先を正確に累積予約し、停止や壊れたGIDでは部分結果を公開しない。
#[test]
fn output_budget_and_invalid_bindings_are_atomic() {
    let font = font();
    let source = slots(65, Some(1));
    let capacity = 256 * std::mem::size_of::<Mapping>() + std::mem::size_of::<u32>();
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: capacity as u64,
        ..Default::default()
    });
    assert!(map(&budget, &font, &source, &ToUnicodeCMap::default()).is_some());
    assert!(map(&budget, &font, &source, &ToUnicodeCMap::default()).is_none());
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
    assert!(map(&budget, &font, &source, &ToUnicodeCMap::default()).is_none());
    assert!(matches!(budget.check(), Err(ParseError::Cancelled)));
    assert!(
        map(
            &DecodeBudget::default(),
            &font,
            &source[..255],
            &ToUnicodeCMap::default()
        )
        .is_none()
    );
    assert!(
        map(
            &DecodeBudget::default(),
            &font,
            &slots(65, Some(999)),
            &ToUnicodeCMap::default()
        )
        .is_none()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 任意codeへ割り当てたAは常に名前のAとなり、byte値によるUnicode推測をしない。
    #[test]
    fn arbitrary_code_uses_confirmed_name(code in 0usize..256) {
        let result=map(&DecodeBudget::default(),&font(),&slots(code,Some(1)),&ToUnicodeCMap::default()).unwrap();
        prop_assert_eq!(text(&result[code]),Some("A".to_string()));
    }
}
