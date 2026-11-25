//! 原本の非記号TrueTypeと明示WinAnsiだけを、Annex Dのglyph名から復号する。
use crate::{
    decode::DecodeBudget, metadata, resource_resolver::ResourceResolver, text::Type1Mapping,
};

/// 記号font・独自Differences・原本ToUnicodeを補完せず、完成した共有表だけを返す。
pub(crate) fn read<'bytes>(
    budget: &DecodeBudget,
    font: &'bytes [u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
    number: Option<u32>,
) -> Option<Type1Mapping> {
    if metadata::value(font, b"ToUnicode").is_some() {
        return None;
    }
    let subtype = resolver.resolve(metadata::value(font, b"Subtype")?)?.0;
    let encoding = resolver.resolve(metadata::value(font, b"Encoding")?)?.0;
    if !metadata::is_name(subtype, b"TrueType") || !metadata::is_name(encoding, b"WinAnsiEncoding")
    {
        return None;
    }
    let descriptor = resolver
        .resolve(metadata::value(font, b"FontDescriptor")?)?
        .0;
    let flags = resolver.resolve(metadata::value(descriptor, b"Flags")?)?.0;
    let flags = crate::syntax::integer(flags)?;
    // PDF32000-1 9.6.6.4の既知の単byte経路に限定し、矛盾したsymbolic指定を採用しない。
    if flags & 32 == 0 || flags & 4 != 0 {
        return None;
    }
    let generation = number.and_then(|n| resolver.generation(n)).unwrap_or(0);
    if let Some(cached) = number.and_then(|n| budget.cached_type1(n, generation)) {
        return Some(cached);
    }
    let mapping =
        crate::text::predefined_type1_mapping(budget, &crate::pdf_font_encodings::WIN_ANSI)?;
    if let Some(number) = number {
        budget.save_type1(number, generation, mapping.clone())?;
    }
    Some(mapping)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseLimits, page_attributes::Objects};

    /// 原本辞書だけを借用し、font programやToUnicodeを偽造しない。
    fn mapping(budget: &DecodeBudget, font: &[u8]) -> Option<Type1Mapping> {
        let objects = Objects::default();
        read(
            budget,
            font,
            &mut ResourceResolver::new(&objects, budget),
            None,
        )
    }

    /// 固定表にも共有workspaceを課し、停止時は部分対応を公開しない。
    #[test]
    fn static_truetype_mapping_is_budgeted_and_undefined_slots_stay_unknown() {
        let font =
            b"<< /Subtype /TrueType /Encoding /WinAnsiEncoding /FontDescriptor << /Flags 32 >> >>";
        let short = DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: 1,
            ..Default::default()
        });
        assert!(mapping(&short, font).is_none());
        assert!(short.is_stopped());
        let enough = DecodeBudget::new(&ParseLimits {
            max_total_tounicode_entries: 0,
            max_total_tounicode_bytes: 0,
            max_total_tounicode_input_bytes: 0,
            ..Default::default()
        });
        let map = mapping(&enough, font).unwrap();
        assert_eq!(map.cmap.lookup_byte(0xa7), Some("§"));
        assert_ne!(map.unknown[0] & 2, 0);
        assert!(enough.check().is_ok());
    }

    /// nullや壊れたToUnicodeも「存在しない」と見なして上書きしない。
    #[test]
    fn authored_or_ambiguous_declarations_are_not_filled() {
        let font =
            "<< /Subtype /TrueType /Encoding /WinAnsiEncoding /FontDescriptor << /Flags 32 >> >>";
        for (from, to) in [
            ("/Flags 32", "/Flags null"),
            ("/Flags 32", "/Flags 36"),
            ("/Flags 32", "/Flags 32.0"),
            ("/FontDescriptor << /Flags 32 >>", "/FontDescriptor null"),
            ("/WinAnsiEncoding", "<< /BaseEncoding /WinAnsiEncoding >>"),
            ("/TrueType", "/Type0"),
            ("/TrueType", "/TrueType /ToUnicode null"),
            ("/TrueType", "/TrueType /ToUnicode 99 0 R"),
        ] {
            assert!(
                mapping(&DecodeBudget::default(), font.replace(from, to).as_bytes()).is_none(),
                "{to}"
            );
        }
    }
}
