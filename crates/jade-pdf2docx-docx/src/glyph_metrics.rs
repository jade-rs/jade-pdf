//! 確認できる一つのglyphだけを測り、複雑なshapingを推測した幅で置き換えない。

/// 通常cmapまたは単一required rligで一glyphになる列のadvanceをem単位で返す。
pub(super) fn advance(bytes: &[u8], text: &str) -> Option<f64> {
    use ttf_parser::{Face, GlyphId, Tag};
    let face = Face::parse(bytes, 0).ok()?;
    // GPOS・kern・可変font・AATの位置調整は、この小さい測定器では評価しない。
    for tag in [b"GPOS", b"kern", b"fvar", b"morx", b"mort", b"kerx"] {
        if face.raw_face().table(Tag::from_bytes(tag)).is_some() {
            return None;
        }
    }
    let mut glyphs = [GlyphId(0); 64];
    let mut len = 0;
    for scalar in text.chars() {
        if len == glyphs.len() {
            return None;
        }
        let glyph = face.glyph_index(scalar)?;
        if glyph.0 == 0 {
            return None;
        }
        glyphs[len] = glyph;
        len += 1;
    }
    let glyphs = &glyphs[..len];
    let glyph = if face.raw_face().table(Tag::from_bytes(b"GSUB")).is_some() {
        required_ligature(face.tables().gsub?, glyphs)?
    } else if glyphs.len() == 1 {
        glyphs[0]
    } else {
        return None;
    };
    let width = face.glyph_hor_advance(glyph)?;
    (width > 0).then_some(f64::from(width) / f64::from(face.units_per_em()))
}

/// DFLTの単一required rligだけを評価し、未対応lookupや部分的な合字は採用しない。
fn required_ligature(
    table: ttf_parser::opentype_layout::LayoutTable<'_>,
    glyphs: &[ttf_parser::GlyphId],
) -> Option<ttf_parser::GlyphId> {
    use ttf_parser::{Tag, gsub::SubstitutionSubtable};
    if glyphs.is_empty()
        || table.scripts.len() != 1
        || table.features.len() != 1
        || table.lookups.len() != 1
    {
        return None;
    }
    let script = table.scripts.get(0)?;
    let language = script.default_language?;
    let feature = table.features.get(0)?;
    if script.tag != Tag::from_bytes(b"DFLT")
        || !script.languages.is_empty()
        || language.required_feature != Some(0)
        || !language.feature_indices.is_empty()
        || feature.tag != Tag::from_bytes(b"rlig")
        || feature.lookup_indices.len() != 1
        || feature.lookup_indices.get(0)? != 0
    {
        return None;
    }
    let lookup = table.lookups.get(0)?;
    if lookup.flags.0 != 0
        || lookup.mark_filtering_set.is_some()
        || lookup.subtables.is_empty()
        || lookup.subtables.len() > 256
    {
        return None;
    }
    let mut found = None;
    for index in 0..lookup.subtables.len() {
        let SubstitutionSubtable::Ligature(subtable) = lookup.subtables.get(index)? else {
            return None;
        };
        if let Some(coverage) = subtable.coverage.get(glyphs[0]) {
            let set = subtable.ligature_sets.get(coverage)?;
            if set.len() > 256 {
                return None;
            }
            for i in 0..set.len() {
                let ligature = set.get(i)?;
                if usize::from(ligature.components.len()) + 1 == glyphs.len()
                    && ligature
                        .components
                        .into_iter()
                        .eq(glyphs[1..].iter().copied())
                {
                    if ligature.glyph.0 == 0 || found.is_some_and(|old| old != ligature.glyph) {
                        return None;
                    }
                    found = Some(ligature.glyph);
                }
            }
        }
    }
    found.or_else(|| (glyphs.len() == 1).then_some(glyphs[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 所有space fontの既知advanceだけを使い、未知字形と複数字形は拒否する。
    #[test]
    fn measures_only_a_verified_single_glyph() {
        let bytes = crate::layout_space::font("JadeLayoutSpace-0");
        assert_eq!(advance(&bytes, " "), Some(1.));
        for text in ["", "A", "  ", "\u{00a0}", "\u{fffd}"] {
            assert_eq!(advance(&bytes, text), None);
        }
        for end in 0..bytes.len().min(100) {
            assert_eq!(advance(&bytes[..end], " "), None);
        }
    }

    /// 二つのUnicodeが一glyphへ必須置換されるときだけ、その一glyphのadvanceを返す。
    #[test]
    fn required_ligature_is_one_advance_and_optional_or_broken_is_rejected() {
        let mut gsub = Vec::new();
        for word in [1u16, 0, 10, 28, 42, 1] {
            gsub.extend_from_slice(&word.to_be_bytes());
        }
        gsub.extend_from_slice(b"DFLT");
        for word in [8u16, 4, 0, 0, 0, 0, 1] {
            gsub.extend_from_slice(&word.to_be_bytes());
        }
        gsub.extend_from_slice(b"rlig");
        for word in [
            8u16, 0, 1, 0, 1, 4, 4, 0, 1, 8, 1, 8, 1, 14, 1, 1, 1, 1, 4, 1, 2, 1,
        ] {
            gsub.extend_from_slice(&word.to_be_bytes());
        }
        let bytes = with_table(b"GSUB", &gsub);
        assert_eq!(advance(&bytes, "  "), Some(1.));
        assert_eq!(advance(&bytes, " "), Some(1.));
        assert_eq!(advance(&bytes, "   "), None);
        gsub[24..26].copy_from_slice(&0xffffu16.to_be_bytes());
        assert_eq!(advance(&with_table(b"GSUB", &gsub), "  "), None);
        assert_eq!(advance(&with_table(b"GSUB", b"broken"), " "), None);
        assert_eq!(advance(&with_table(b"GPOS", b"broken"), " "), None);
    }

    /// テスト側で表directoryを独立に組み、所有fontへ検査対象のtableを足す。
    fn with_table(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let source = crate::layout_space::font("JadeLayoutSpace-0");
        let reader = ttf_parser::Face::parse(&source, 0).unwrap();
        let mut tables = reader
            .raw_face()
            .table_records
            .into_iter()
            .map(|r| (r.tag.to_bytes(), reader.raw_face().table(r.tag).unwrap()))
            .collect::<Vec<_>>();
        tables.push((*tag, data));
        tables.sort_by_key(|(tag, _)| *tag);
        let mut result = vec![0; 12 + tables.len() * 16];
        result[..4].copy_from_slice(&0x10000u32.to_be_bytes());
        result[4..6].copy_from_slice(&(tables.len() as u16).to_be_bytes());
        for (index, (tag, bytes)) in tables.into_iter().enumerate() {
            let entry = 12 + 16 * index;
            let offset = result.len();
            result[entry..entry + 4].copy_from_slice(&tag);
            result[entry + 8..entry + 12].copy_from_slice(&(offset as u32).to_be_bytes());
            result[entry + 12..entry + 16].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
            result.extend_from_slice(bytes);
            while result.len() % 4 != 0 {
                result.push(0);
            }
        }
        result
    }
}
