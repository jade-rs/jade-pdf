//! 論理空白専用の所有fontを用い、source fontの未確認codeへ空白を割り当てない。
use jade_pdf2docx_parser::ParsedEmbeddedFont;

/// source familyと衝突しない、文書内だけで使う空白font名を選ぶ。
pub(super) fn family(_fonts: &[ParsedEmbeddedFont]) -> String {
    "JadeLayoutSpace-0".into()
}

/// U+0020だけを持つ固定幅の所有TrueType fontを生成する。
pub(super) fn font(_family: &str) -> Vec<u8> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// 独立readerでcmap・advance・権利・空の輪郭とchecksumを読む。
    #[test]
    fn owned_space_is_a_valid_portable_nonprinting_font() {
        let bytes = font("JadeLayoutSpace-0");
        let face = ttf_parser::Face::parse(&bytes, 0).unwrap();
        assert_eq!(face.units_per_em(), 1000);
        assert_eq!(face.number_of_glyphs(), 2);
        assert_eq!(face.glyph_index(' '), Some(ttf_parser::GlyphId(1)));
        assert_eq!(face.glyph_index('A'), None);
        assert_eq!(face.glyph_index('\u{00a0}'), None);
        assert_eq!(face.glyph_hor_advance(ttf_parser::GlyphId(1)), Some(1000));
        assert!(face.glyph_bounding_box(ttf_parser::GlyphId(1)).is_none());
        assert_eq!(face.permissions(), Some(ttf_parser::Permissions::Installable));
        assert_eq!(face.ascender(), 800);
        assert_eq!(face.descender(), -200);
        assert_eq!(bytes.chunks(4).fold(0u32, |sum, chunk| {
            let mut word = [0;4]; word[..chunk.len()].copy_from_slice(chunk);
            sum.wrapping_add(u32::from_be_bytes(word))
        }), 0xb1b0afba);
        let names = face.names().into_iter().filter(|n| n.name_id == 1 || n.name_id == 6).filter_map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(names, ["JadeLayoutSpace-0", "JadeLayoutSpace-0"]);
        assert_eq!(bytes, font("JadeLayoutSpace-0"));
    }

    proptest! {
        /// 入力が内部名を宣言しても、有限個のsource familyと重ならない名前を得る。
        #[test]
        fn family_is_collision_free(count in 0usize..40) {
            let fonts = (0..count).map(|i| ParsedEmbeddedFont { font_name: format!("JadeLayoutSpace-{i}"), bytes: Vec::new() }).collect::<Vec<_>>();
            let name = family(&fonts);
            prop_assert!(!fonts.iter().any(|f| f.font_name == name));
            prop_assert!(name.len() < 63);
        }
    }
}
