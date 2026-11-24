//! 埋め込みfontも原本ToUnicodeもない標準Latin書体の、明示WinAnsiだけを確認する。
use crate::{
    decode::DecodeBudget, metadata, resource_resolver::ResourceResolver, text::Type1Mapping,
};

/// glyph名の対応を既存の予算付き表へ渡し、未定義slotは確認不能として保持する。
pub(crate) fn read<'bytes>(
    budget: &DecodeBudget,
    font: &'bytes [u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
) -> Option<Type1Mapping> {
    // 空または未対応のToUnicodeを「原本に存在しない」と取り違えて補完しない。
    if metadata::value(font, b"ToUnicode").is_some()
        || metadata::value(font, b"FontDescriptor").is_some()
    {
        return None;
    }
    let subtype = resolver.resolve(metadata::value(font, b"Subtype")?)?.0;
    if !metadata::is_name(subtype, b"Type1") {
        return None;
    }
    let base = resolver.resolve(metadata::value(font, b"BaseFont")?)?.0;
    if ![
        b"Courier".as_slice(),
        b"Courier-Bold",
        b"Courier-Oblique",
        b"Courier-BoldOblique",
        b"Helvetica",
        b"Helvetica-Bold",
        b"Helvetica-Oblique",
        b"Helvetica-BoldOblique",
        b"Times-Roman",
        b"Times-Bold",
        b"Times-Italic",
        b"Times-BoldItalic",
    ]
    .iter()
    .any(|name| metadata::is_name(base, name))
    {
        return None;
    }
    let encoding = resolver.resolve(metadata::value(font, b"Encoding")?)?.0;
    if !metadata::is_name(encoding, b"WinAnsiEncoding") {
        return None;
    }
    let names = crate::pdf_font_encodings::WIN_ANSI.map(Some);
    crate::text::type1_mapping(budget, None, &names, &Default::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseLimits, page_attributes::Objects};

    /// sourceの辞書だけを借用し、FontFileやToUnicodeを勝手に作らない。
    fn mapping(budget: &DecodeBudget, dictionary: &[u8]) -> Option<Type1Mapping> {
        let objects = Objects::default();
        let mut resolver = ResourceResolver::new(&objects, budget);
        read(budget, dictionary, &mut resolver)
    }

    /// 256slotの容量不足時は部分表を返さず、完成表でも未定義codeを既知文字と区別する。
    #[test]
    fn named_winansi_mapping_respects_total_entry_budget() {
        let font = b"<< /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";
        let short = DecodeBudget::new(&ParseLimits {
            max_total_tounicode_entries: 255,
            ..Default::default()
        });
        assert!(mapping(&short, font).is_none());
        assert!(short.is_stopped());
        let enough = DecodeBudget::new(&ParseLimits {
            max_total_tounicode_entries: 256,
            ..Default::default()
        });
        let result = mapping(&enough, font).unwrap();
        assert!(enough.check().is_ok());
        assert_eq!(result.cmap.lookup_byte(0x97), Some("—"));
        assert_ne!(result.unknown[0] & 2, 0);
        assert_eq!(result.unknown[0x97 / 64] & (1u64 << (0x97 % 64)), 0);
    }

    /// 原本対応・font descriptor・独自font・別encodingの根拠を上書きしない。
    #[test]
    fn source_declarations_are_never_overridden() {
        let font = "<< /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";
        for (from, to) in [
            (" >>", " /ToUnicode null >>"),
            (" >>", " /FontDescriptor null >>"),
            ("/Helvetica", "/Custom"),
            ("/Helvetica", "/Symbol"),
            ("/WinAnsiEncoding", "/MacRomanEncoding"),
            ("/WinAnsiEncoding", "<< /BaseEncoding /WinAnsiEncoding >>"),
            ("/Type1", "/TrueType"),
        ] {
            assert!(
                mapping(&DecodeBudget::default(), font.replace(from, to).as_bytes()).is_none(),
                "{to}"
            );
        }
    }
}
