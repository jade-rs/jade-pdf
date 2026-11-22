use super::*;
use proptest::prelude::*;

/// 空の制御glyphと、従来の検証に必要な非空glyphを持つ所有sfnt構造を作る。
fn font(empty_code: u16) -> Vec<u8> {
    let visible_code = if empty_code == 65 { 66 } else { 65 };
    let mut mappings = [(empty_code, 1u16), (visible_code, 2u16)];
    mappings.sort_unstable();
    let mut sub = Vec::new();
    for value in [4u16, 40, 0, 6, 4, 1, 2] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    for value in [
        mappings[0].0,
        mappings[1].0,
        0xffff,
        0,
        mappings[0].0,
        mappings[1].0,
        0xffff,
        mappings[0].1.wrapping_sub(mappings[0].0),
        mappings[1].1.wrapping_sub(mappings[1].0),
        1,
        0,
        0,
        0,
    ] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    let mut cmap = Vec::new();
    for value in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&sub);
    let head = vec![0; 54];
    let loca: Vec<u8> = [0u16, 0, 0, 5]
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect();
    let glyf = vec![0; 10];
    let maxp = vec![0, 1, 0, 0, 0, 3];
    let tables = [
        (*b"cmap", cmap),
        (*b"head", head),
        (*b"loca", loca),
        (*b"glyf", glyf),
        (*b"maxp", maxp),
    ];
    let mut bytes = vec![0; 12 + tables.len() * 16];
    bytes[..4].copy_from_slice(&[0, 1, 0, 0]);
    bytes[4..6].copy_from_slice(&(tables.len() as u16).to_be_bytes());
    for (index, (tag, data)) in tables.iter().enumerate() {
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        let record = 12 + index * 16;
        bytes[record..record + 4].copy_from_slice(tag);
        let offset = bytes.len() as u32;
        bytes[record + 8..record + 12].copy_from_slice(&offset.to_be_bytes());
        bytes[record + 12..record + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend_from_slice(data);
    }
    bytes
}

/// Windows/OpenTypeの非描画制御文字のnull glyphを、可視文字欠落と誤認しない。
#[test]
fn null_and_other_controls_may_have_empty_outlines() {
    for code in [0, 8, 0x1d, 0x7f, 0x85] {
        assert!(subset_unicode_cmap_is_complete(&font(code)), "U+{code:04X}");
    }
}

/// ラテン・日本語・結合記号・format文字など、従来の可視文字検証は保つ。
#[test]
fn visible_and_format_character_missing_outlines_remain_rejected() {
    for code in [65, 0x4e2d, 0x0301, 0x200d, 0xfffd] {
        assert!(
            !subset_unicode_cmap_is_complete(&font(code)),
            "U+{code:04X}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// 任意BMPの空glyphは、空白または制御文字だけを許す。
    #[test]
    fn empty_glyph_acceptance_follows_nonprinting_character_categories(code in 0u16..0xffff) {
        let expected = char::from_u32(u32::from(code)).is_some_and(|c| c.is_whitespace() || c.is_control());
        prop_assert_eq!(subset_unicode_cmap_is_complete(&font(code)), expected);
    }
}
