//! 所有外形だけで、OpenTypeの必要table・metric・許諾・style属性を検査する。
use super::*;
use crate::type1_outline::{
    charstring::{Command, Glyph},
    type2,
};
use crate::{ParseError, ParseLimits};
use proptest::prelude::*;

/// 高さ700・幅600の所有三角形を、二glyphのfontへまとめる。
fn fixture(budget: &DecodeBudget, flags: u16, bold: bool, italic: bool) -> Option<Vec<u8>> {
    let glyph = Glyph {
        advance: 600.,
        commands: vec![
            Command::Move([0., 0.]),
            Command::Line([300., 700.]),
            Command::Line([600., 0.]),
            Command::Close,
        ],
    };
    let program = type2::encode(&DecodeBudget::default(), &glyph).unwrap();
    let glyphs = [
        cff::Entry {
            name: b".notdef",
            program: &[28, 130, 88, 14],
        },
        cff::Entry {
            name: b"A",
            program: &program,
        },
    ];
    let font = cff::Font {
        name: b"JadeOwnedOTF",
        notice: b"Jade owned original notice",
        bbox: [0, 0, 600, 700],
        italic_angle: if italic { -12.5 } else { 0. },
        is_fixed_pitch: false,
        glyphs: &glyphs,
    };
    let metrics = [
        Metric {
            advance: 600,
            left: 0,
            bounds: None,
        },
        Metric {
            advance: 600,
            left: 0,
            bounds: Some([0, 0, 600, 700]),
        },
    ];
    let mappings = [cmap::Mapping {
        scalar: 65,
        glyph: 1,
    }];
    let info = Info {
        metrics: &metrics,
        mappings: &mappings,
        ascent: 700,
        descent: -200,
        line_gap: 0,
        weight: if bold { 700 } else { 400 },
        bold,
        italic,
        embedding_flags: flags,
        underline_position: -100,
        underline_thickness: 50,
        x_height: 450,
        cap_height: 700,
        license: "Jade owned permission; original notice retained",
        license_url: "https://example.invalid/owned",
    };
    encode(budget, &font, &info)
}

/// tableをdirectoryから独立に検索して借用する。
fn table<'a>(font: &'a [u8], tag: &[u8; 4]) -> &'a [u8] {
    for index in 0..u16_at(font, 4) as usize {
        let row = 12 + 16 * index;
        if &font[row..row + 4] == tag {
            let offset = u32_at(font, row + 8) as usize;
            let length = u32_at(font, row + 12) as usize;
            return &font[offset..offset + length];
        }
    }
    panic!("missing table {tag:?}")
}
/// 大端16bitを読む。
fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap())
}
/// 大端32bitを読む。
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
}
/// Windows name recordのUTF16を独立に復元する。
fn name(font: &[u8], id: u16) -> String {
    let bytes = table(font, b"name");
    let start = u16_at(bytes, 4) as usize;
    for index in 0..u16_at(bytes, 2) as usize {
        let row = 6 + 12 * index;
        if u16_at(bytes, row) == 3 && u16_at(bytes, row + 6) == id {
            let len = u16_at(bytes, row + 8) as usize;
            let offset = start + u16_at(bytes, row + 10) as usize;
            let chars = bytes[offset..offset + len]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect::<Vec<_>>();
            return String::from_utf16(&chars).unwrap();
        }
    }
    panic!("missing name ID{id}")
}

/// 九つの必須tableを持ち、元の幅・許諾・styleを変更しない。
#[test]
fn owned_font_has_complete_tables_and_source_identity() {
    let bytes = fixture(&DecodeBudget::default(), 8, true, true).unwrap();
    assert_eq!(&bytes[..4], b"OTTO");
    assert_eq!(u16_at(&bytes, 4), 9);
    for tag in [
        b"CFF ", b"OS/2", b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"post",
    ] {
        assert!(!table(&bytes, tag).is_empty());
    }
    let head = table(&bytes, b"head");
    assert_eq!(u16_at(head, 18), 1000);
    assert_eq!(u16_at(head, 44), 3);
    assert_eq!(table(&bytes, b"maxp"), &[0, 0, 80, 0, 0, 2]);
    assert_eq!(u16_at(table(&bytes, b"hhea"), 34), 2);
    assert_eq!(table(&bytes, b"hmtx"), &[2, 88, 0, 0, 2, 88, 0, 0]);
    let os2 = table(&bytes, b"OS/2");
    assert_eq!(u16_at(os2, 0), 4);
    assert_eq!(u16_at(os2, 4), 700);
    assert_eq!(u16_at(os2, 8), 8);
    assert_eq!(u16_at(os2, 62), 161);
    assert_eq!(u16_at(os2, 86), 450);
    assert_eq!(u16_at(os2, 88), 700);
    assert_eq!(u32_at(table(&bytes, b"post"), 0), 0x30000);
    assert_eq!(u32_at(table(&bytes, b"post"), 4) as i32, -819200);
    assert_eq!(name(&bytes, 0), "Jade owned original notice");
    assert_eq!(name(&bytes, 6), "JadeOwnedOTF");
    assert_eq!(name(&bytes, 2), "Bold Italic");
    assert_eq!(
        name(&bytes, 13),
        "Jade owned permission; original notice retained"
    );
    assert_eq!(name(&bytes, 14), "https://example.invalid/owned");
}

/// 不許可やbitmap限定を許可へ変更せず、容量不足も黙って成功させない。
#[test]
fn permission_and_storage_failures_do_not_produce_a_font() {
    for flags in [1, 2, 0x200, 12] {
        assert!(fixture(&DecodeBudget::default(), flags, false, false).is_none());
    }
    let target = DecodeBudget::new(&ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    assert!(fixture(&target, 0, false, false).is_none());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "embedded font bytes",
            ..
        })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 許可bitと四つの書体属性を、head・OS/2・nameへ一貫して保存する。
    #[test]
    fn intrinsic_style_and_embedding_bits_are_consistent(which in 0_usize..3,bold in any::<bool>(),italic in any::<bool>()) {
        let flags=[0,4,8][which];let bytes=fixture(&DecodeBudget::default(),flags,bold,italic).unwrap();
        prop_assert_eq!(u16_at(table(&bytes,b"OS/2"),8),flags);
        prop_assert_eq!(u16_at(table(&bytes,b"head"),44),u16::from(bold)+2*u16::from(italic));
        let selection=128+32*u16::from(bold)+u16::from(italic)+if !bold && !italic {64}else{0};
        prop_assert_eq!(u16_at(table(&bytes,b"OS/2"),62),selection);
    }
}
