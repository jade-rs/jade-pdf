//! 単一Mac cmapのsymbolic TrueTypeを、PDFの明示文字対応でWord用に復元する回帰。
use super::{DecodeBudget, SfntEmbeddingPermission, embedded_fonts, sfnt_embedding_permission};
use crate::text;

#[path = "../../jade-pdf2docx-docx/src/layout_space/writer.rs"]
mod owned_writer;

/// 単一symbolic fontの所有関係が明示された最小PDF metadata。
const SOURCE: &[u8] = b"1 0 obj << /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+OwnedMac /FontDescriptor 2 0 R /ToUnicode 4 0 R /FirstChar 1 /LastChar 1 /Widths [1000] >> endobj\n2 0 obj << /Type /FontDescriptor /FontName /ABCDEF+OwnedMac /Flags 4 /FontFile2 3 0 R >> endobj";

/// 単byte1をAにする、明示code space付きのToUnicode。
const MAPPING: &[u8] =
    b"1 begincodespacerange <00> <FF> endcodespacerange\n1 beginbfchar <01> <0041> endbfchar";

/// 公開処理が使うfont collectorを、指定した独立fontとPDF metadataで呼ぶ。
fn reuse(bytes: &[u8], source: &[u8], mapping: &[u8]) -> Vec<crate::ParsedEmbeddedFont> {
    let cmap = text::parse_tounicode_body(&DecodeBudget::default(), mapping);
    let budget = DecodeBudget::default();
    let fonts = embedded_fonts::build(&budget, source, |_| Some(bytes.to_vec()), |_| cmap.clone());
    budget.check().unwrap();
    fonts
}

/// 所有する矩形をgid1へ移し、PDFの単byte1だけを対応させる。
fn owned_mac_font() -> Vec<u8> {
    let mut bytes = owned_writer::encode("OwnedMac");
    let cmap = table_offset(&bytes, b"cmap");
    let loca = table_offset(&bytes, b"loca");
    let hhea = table_offset(&bytes, b"hhea");
    bytes[loca + 2..loca + 4].fill(0);
    bytes[hhea + 14..hhea + 16].copy_from_slice(&500u16.to_be_bytes());
    bytes[cmap..cmap + 44].fill(0);
    for (index, value) in [0u16, 1, 1, 0, 0, 12, 6, 12, 0, 1, 1, 1]
        .into_iter()
        .enumerate()
    {
        bytes[cmap + index * 2..cmap + index * 2 + 2].copy_from_slice(&value.to_be_bytes());
    }
    let count = usize::from(u16::from_be_bytes(bytes[4..6].try_into().unwrap()));
    for index in 0..count {
        let entry = 12 + 16 * index;
        if &bytes[entry..entry + 4] == b"cmap" {
            bytes[entry + 12..entry + 16].copy_from_slice(&24u32.to_be_bytes());
        }
    }
    checksums(&mut bytes);
    bytes
}

/// 所有する固定fontのdirectoryから検査対象のoffsetを独立に読む。
fn table_offset(bytes: &[u8], tag: &[u8; 4]) -> usize {
    let count = usize::from(u16::from_be_bytes(bytes[4..6].try_into().unwrap()));
    (0..count)
        .find_map(|index| {
            let entry = 12 + 16 * index;
            (&bytes[entry..entry + 4] == tag).then(|| {
                u32::from_be_bytes(bytes[entry + 8..entry + 12].try_into().unwrap()) as usize
            })
        })
        .unwrap()
}

/// 各表とfont全体のSFNT checksumを再計算する。
fn checksums(bytes: &mut [u8]) {
    let head = table_offset(bytes, b"head");
    bytes[head + 8..head + 12].fill(0);
    let count = usize::from(u16::from_be_bytes(bytes[4..6].try_into().unwrap()));
    for index in 0..count {
        let entry = 12 + 16 * index;
        let offset = u32::from_be_bytes(bytes[entry + 8..entry + 12].try_into().unwrap()) as usize;
        let length = u32::from_be_bytes(bytes[entry + 12..entry + 16].try_into().unwrap()) as usize;
        let sum = checksum(&bytes[offset..offset + length]);
        bytes[entry + 4..entry + 8].copy_from_slice(&sum.to_be_bytes());
    }
    let adjustment = 0xb1b0afbau32.wrapping_sub(checksum(bytes));
    bytes[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    assert_eq!(checksum(bytes), 0xb1b0afba);
}

/// 末尾のpaddingをゼロとしてbig-endian wordの総和を求める。
fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// cmapはUnicodeではないが、PDF code1とToUnicodeのAを介した字形を再利用する。
#[test]
fn allowed_symbolic_mac_subset_retains_its_verified_source_glyph() {
    let bytes = owned_mac_font();
    assert!(matches!(
        sfnt_embedding_permission(&bytes),
        SfntEmbeddingPermission::Allowed
    ));
    assert!(!crate::font_validation::subset_unicode_cmap_is_complete(
        &bytes
    ));
    let loca = table_offset(&bytes, b"loca");
    assert_eq!(&bytes[loca..loca + 6], &[0, 0, 0, 0, 0, 12]);
    let glyf = table_offset(&bytes, b"glyf");
    assert_eq!(&bytes[glyf..glyf + 2], &[0, 1]);
    let source = SOURCE;
    let cmap = text::parse_tounicode_body(
        &DecodeBudget::default(),
        b"1 begincodespacerange <00> <FF> endcodespacerange\n1 beginbfchar <01> <0041> endbfchar",
    );
    assert_eq!(cmap.lookup_byte(1), Some("A"));
    let budget = DecodeBudget::default();
    let fonts = embedded_fonts::build(
        &budget,
        source,
        |number| (number == 3).then(|| bytes.clone()),
        |number| {
            assert_eq!(number, 2);
            cmap.clone()
        },
    );
    budget.check().unwrap();
    assert_eq!(
        fonts.len(),
        1,
        "verified symbolic Mac subset must be reusable"
    );
    assert!(crate::font_validation::subset_unicode_cmap_is_complete(
        &fonts[0].bytes
    ));
    assert_eq!(checksum(&fonts[0].bytes), 0xb1b0afba);
    let restored = &fonts[0].bytes;
    let restored_loca = table_offset(restored, b"loca");
    let restored_glyf = table_offset(restored, b"glyf");
    assert_eq!(
        &restored[restored_loca..restored_loca + 6],
        &bytes[loca..loca + 6]
    );
    assert_eq!(
        &restored[restored_glyf..restored_glyf + 24],
        &bytes[glyf..glyf + 24]
    );
}

/// f64が受理してもPDF realではない指数表記を、確認済みの字幅として扱わない。
#[test]
fn scientific_notation_is_not_a_verified_pdf_width() {
    let source = String::from_utf8(SOURCE.to_vec())
        .unwrap()
        .replace("[1000]", "[1e3]");
    assert!(reuse(&owned_mac_font(), source.as_bytes(), MAPPING).is_empty());
}

/// 二つの原本codeを、.notdefと実字形の異なる同じUnicodeへ曖昧に結ばない。
#[test]
fn notdef_and_visible_glyph_cannot_alias_the_same_unicode() {
    let mut font = owned_mac_font();
    let cmap = table_offset(&font, b"cmap");
    for (at, value) in [(14, 14u16), (18, 0), (20, 2), (22, 0), (24, 1)] {
        font[cmap + at..cmap + at + 2].copy_from_slice(&value.to_be_bytes());
    }
    let count = usize::from(u16::from_be_bytes(font[4..6].try_into().unwrap()));
    for index in 0..count {
        let entry = 12 + 16 * index;
        if &font[entry..entry + 4] == b"cmap" {
            font[entry + 12..entry + 16].copy_from_slice(&26u32.to_be_bytes());
        }
    }
    checksums(&mut font);
    let source = String::from_utf8(SOURCE.to_vec())
        .unwrap()
        .replace("/FirstChar 1", "/FirstChar 0")
        .replace("[1000]", "[500 1000]");
    let mapping = b"1 begincodespacerange <00> <FF> endcodespacerange\n2 beginbfchar <00> <0041> <01> <0041> endbfchar";
    assert!(reuse(&font, source.as_bytes(), mapping).is_empty());
}

/// 未確認のFont種別・flag・encoding・共有参照を既存fallbackから昇格させない。
#[test]
fn ambiguous_pdf_font_contexts_keep_fallback() {
    let original = String::from_utf8(SOURCE.to_vec()).unwrap();
    let mut cases = Vec::new();
    for (before, after) in [
        ("/Flags 4", "/Flags 32"),
        ("/Flags 4", "/Flags 36"),
        ("/Flags 4", ""),
        ("/Flags 4", "/Flags 4 /#46lags 4"),
        ("/TrueType", "/Type1"),
        ("/TrueType", "/CIDFontType2"),
        (
            "/Subtype /TrueType",
            "/Subtype /TrueType /Subtype /TrueType",
        ),
        ("/FontDescriptor 2 0 R", "/FontDescriptor 2 1 R"),
        ("/ToUnicode 4 0 R", "/ToUnicode 4 1 R"),
        ("/ToUnicode 4 0 R", ""),
        ("/FirstChar 1", "/Encoding /WinAnsiEncoding /FirstChar 1"),
        ("/FirstChar 1", "/Encoding /MacRomanEncoding /FirstChar 1"),
        ("/FirstChar 1", "/Encoding null /FirstChar 1"),
    ] {
        cases.push(original.replace(before, after));
    }
    cases.push(format!("{original}\n5 0 obj << /Type /Font /Subtype /TrueType /FontDescriptor 2 0 R /ToUnicode 4 0 R /FirstChar 1 /LastChar 1 /Widths [1000] >> endobj"));
    cases.push(format!("{original}\n2 0 obj << /Type /FontDescriptor /FontName /ABCDEF+OwnedMac /Flags 4 /FontFile2 3 0 R >> endobj"));
    let font = owned_mac_font();
    for source in cases {
        assert_ne!(source.as_bytes(), SOURCE);
        assert!(
            reuse(&font, source.as_bytes(), MAPPING).is_empty(),
            "{source}"
        );
    }
}

/// PDF幅の形式・個数・実際のadvanceが合わなければ字形を公開しない。
#[test]
fn unverified_pdf_width_arrays_keep_fallback() {
    let original = String::from_utf8(SOURCE.to_vec()).unwrap();
    let font = owned_mac_font();
    for widths in [
        "[]",
        "[1000 1000]",
        "[999]",
        "[-1000]",
        "[NaN]",
        "[inf]",
        "[1e3]",
        "[.]",
        "3 0 R",
    ] {
        let source = original.replace("[1000]", widths);
        assert!(
            reuse(&font, source.as_bytes(), MAPPING).is_empty(),
            "{widths}"
        );
    }
    for widths in ["[+1000]", "[1000.]", "[1000.0005]"] {
        let source = original.replace("[1000]", widths);
        assert_eq!(
            reuse(&font, source.as_bytes(), MAPPING).len(),
            1,
            "{widths}"
        );
    }
}

/// 多文字・非BMP・code space外を一部だけ採用したfontを作らない。
#[test]
fn incomplete_or_non_byte_unicode_mappings_keep_fallback() {
    let font = owned_mac_font();
    for mapping in [
        "0 beginbfchar endbfchar",
        "1 beginbfchar <01> <00410042> endbfchar",
        "1 beginbfchar <01> <D83DDE00> endbfchar",
        "1 beginbfchar <01> <FFFF> endbfchar",
        "1 beginbfchar <01> <FFFE> endbfchar",
        "1 begincodespacerange <0000> <00FF> endcodespacerange 1 beginbfchar <0001> <0041> endbfchar",
        "1 begincodespacerange <02> <FF> endcodespacerange 1 beginbfchar <01> <0041> endbfchar",
        "2 beginbfchar <01> <0041> <02> <D83DDE00> endbfchar",
    ] {
        assert!(
            reuse(&font, SOURCE, mapping.as_bytes()).is_empty(),
            "{mapping}"
        );
    }
}

/// 単一Mac Roman format6と確認できない入力をUnicode fontへ修復しない。
#[test]
fn unsupported_cmaps_and_corrupt_programs_keep_fallback() {
    for (at, value) in [
        (4, 3u16),
        (6, 1),
        (12, 4),
        (14, 10),
        (16, 1),
        (18, 256),
        (20, 0),
    ] {
        let mut font = owned_mac_font();
        let cmap = table_offset(&font, b"cmap");
        font[cmap + at..cmap + at + 2].copy_from_slice(&value.to_be_bytes());
        checksums(&mut font);
        assert!(
            reuse(&font, SOURCE, MAPPING).is_empty(),
            "offset={at},value={value}"
        );
    }
    let mut font = owned_mac_font();
    let glyf = table_offset(&font, b"glyf");
    font[glyf + 3] ^= 1;
    assert!(reuse(&font, SOURCE, MAPPING).is_empty());
    let mut font = owned_mac_font();
    let os2 = table_offset(&font, b"OS/2");
    font[os2 + 8..os2 + 10].copy_from_slice(&2u16.to_be_bytes());
    checksums(&mut font);
    assert!(reuse(&font, SOURCE, MAPPING).is_empty());
}

/// font保存容量ゼロは型付きエラーを残し、完成していないfontを公開しない。
#[test]
fn repair_font_budget_failure_is_atomic() {
    let target = DecodeBudget::new(&crate::ParseLimits {
        max_total_embedded_font_bytes: 0,
        ..Default::default()
    });
    let cmap = text::parse_tounicode_body(&DecodeBudget::default(), MAPPING);
    let font = owned_mac_font();
    assert!(
        embedded_fonts::build(&target, SOURCE, |_| Some(font.clone()), |_| cmap.clone()).is_empty()
    );
    assert!(matches!(
        target.check(),
        Err(crate::ParseError::ResourceLimit {
            resource: "embedded font bytes",
            ..
        })
    ));
}

/// Unicode format4の単一glyph対応を、修復関数に依存せず読む。
fn unicode_glyph(bytes: &[u8], unicode: u16) -> Option<u16> {
    let cmap = table_offset(bytes, b"cmap");
    let sub = cmap + u32::from_be_bytes(bytes[cmap + 8..cmap + 12].try_into().unwrap()) as usize;
    let word = |at| u16::from_be_bytes(bytes[sub + at..sub + at + 2].try_into().unwrap());
    assert_eq!(word(0), 4);
    let count = usize::from(word(6)) / 2;
    for index in 0..count {
        let end = word(14 + index * 2);
        let start = word(16 + count * 2 + index * 2);
        if (start..=end).contains(&unicode) {
            assert_eq!(word(16 + count * 6 + index * 2), 0);
            return Some(unicode.wrapping_add(word(16 + count * 4 + index * 2)));
        }
    }
    None
}

use proptest::prelude::*;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// source codeとUnicodeを別々に変えても、元のgidと全体checksumを保持する。
    #[test]
    fn byte_code_and_unicode_are_composed_without_identity_assumption(code in 0u8..=255, unicode in 65u16..=90) {
        let mut font = owned_mac_font();
        let cmap = table_offset(&font,b"cmap");
        font[cmap+18..cmap+20].copy_from_slice(&u16::from(code).to_be_bytes());
        checksums(&mut font);
        let source=String::from_utf8(SOURCE.to_vec()).unwrap().replace("/FirstChar 1",&format!("/FirstChar {code}")).replace("/LastChar 1",&format!("/LastChar {code}"));
        let mapping=format!("1 begincodespacerange <00> <FF> endcodespacerange 1 beginbfchar <{code:02X}> <{unicode:04X}> endbfchar");
        let fonts=reuse(&font,source.as_bytes(),mapping.as_bytes());
        prop_assert_eq!(fonts.len(),1);
        prop_assert_eq!(unicode_glyph(&fonts[0].bytes,unicode),Some(1));
        prop_assert_eq!(checksum(&fonts[0].bytes),0xb1b0afba);
        let original=table_offset(&font,b"glyf");let restored=table_offset(&fonts[0].bytes,b"glyf");
        prop_assert_eq!(&font[original..original+24],&fonts[0].bytes[restored..restored+24]);
    }
}
