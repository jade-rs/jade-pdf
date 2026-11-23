//! 原本PDFから作ったglyph計画の幅と文字列を、独立した原本glyphへ照合する。
use super::*;

/// code単位の幅・Unicode列と全元glyphを検査し、未解決の文字対応衝突も報告する。
pub(super) fn compare(
    budget: &DecodeBudget,
    decoded: &crate::type1_outline::file::Font<'_>,
    font: &Font,
    unicode: Vec<crate::type1_outline::unicode::Mapping>,
) -> serde_json::Value {
    let reference = font.pdf_resource.as_ref().unwrap();
    let plan = crate::type1_outline::plan::build(budget, decoded, unicode).unwrap();
    for (gid, original) in decoded.glyphs.iter().enumerate() {
        assert_eq!(usize::from(plan.glyphs[gid].original), gid);
        assert!(plan.glyphs[gid].pdf_width.is_none());
        assert!(
            font.glyphs
                .iter()
                .any(|g| g.name.as_bytes() == original.name)
        );
    }
    let mut rows = Vec::new();
    let mut unicode_glyphs = std::collections::BTreeMap::new();
    let mut conflicts = Vec::new();
    for (code, (actual, expected)) in plan.codes.iter().zip(&reference.slots).enumerate() {
        assert_eq!(actual.mapping.scalars, expected.scalars);
        assert_eq!(actual.mapping.from_tounicode, expected.from_tounicode);
        assert_eq!(actual.mapping.source.width, expected.width);
        assert_eq!(
            actual.mapping.source.explicit_width,
            expected.explicit_width
        );
        let shape = actual.glyph.map(|gid| {
            let binding = &plan.glyphs[usize::from(gid)];
            let original = &decoded.glyphs[usize::from(binding.original)];
            assert_eq!(original.name, expected.name.as_bytes());
            let independent = font
                .glyphs
                .iter()
                .find(|g| g.name == expected.name)
                .unwrap();
            let actual_width = binding.pdf_width.unwrap_or(original.outline.advance);
            assert_eq!(
                actual_width, expected.width,
                "{} code{code}",
                font.source_name
            );
            let width = binding.pdf_width.unwrap_or(independent.advance);
            assert!(
                (width - expected.width).abs() <= 1e-7,
                "{} code{code} {width} != {}",
                font.source_name,
                expected.width
            );
            (gid, binding.original, width)
        });
        assert_eq!(
            shape.is_some(),
            expected.present && expected.name != ".notdef"
        );
        if let (Some((gid, _, _)), Some(text)) = (shape, &actual.mapping.scalars)
            && !text.is_empty()
            && let Some(prior) = unicode_glyphs.insert(text.clone(), gid)
            && prior != gid
        {
            conflicts.push(serde_json::json!({"code":code,"scalars":text,"first_glyph":prior,"second_glyph":gid}));
        }
        rows.push(serde_json::json!({"code":code,"shape":shape,"name":expected.name,"scalars":actual.mapping.scalars,"from_tounicode":actual.mapping.from_tounicode}));
    }
    println!(
        "{}: {} original glyphs, {} planned glyphs; all256 code widths/sequences retained; {} Unicode/GID conflicts",
        font.source_name,
        decoded.glyphs.len(),
        plan.glyphs.len(),
        conflicts.len()
    );
    serde_json::json!({"source":font.source_name,"original_glyphs":decoded.glyphs.len(),"planned_glyphs":plan.glyphs.len(),"unicode_glyph_conflicts":conflicts,"codes":rows})
}
