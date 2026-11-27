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
