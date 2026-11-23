//! 論理空白専用の所有fontを用い、source fontの未確認codeへ空白を割り当てない。
use jade_pdf2docx_parser::ParsedEmbeddedFont;
mod writer;

/// source familyと衝突しない、文書内だけで使う空白font名を選ぶ。
pub(super) fn family(fonts: &[ParsedEmbeddedFont]) -> String {
    (0..=fonts.len())
        .map(|i| format!("JadeLayoutSpace-{i}"))
        .find(|name| !fonts.iter().any(|f| f.font_name.eq_ignore_ascii_case(name)))
        .expect("n source names cannot occupy n+1 distinct internal names")
}

/// U+0020だけを持つ固定幅の所有TrueType fontを生成する。
pub(super) fn font(family: &str) -> Vec<u8> {
    writer::encode(family)
}

/// 内部spaceを参照する文書だけ、確認済みadvanceを半emへ補正する互換flagを外す。
pub(super) fn preserve_advance(package: &mut docx_rs::XMLDocx, fonts: &[ParsedEmbeddedFont]) {
    let reference = format!(r#"w:ascii="{}""#, family(fonts));
    if !package
        .document
        .windows(reference.len())
        .any(|part| part == reference.as_bytes())
    {
        return;
    }
    // docx-rsが生成したsettingsだけを対象にし、他の互換設定は保持する。
    let flag = b"<w:balanceSingleByteDoubleByteWidth />";
    if let Some(offset) = package
        .settings
        .windows(flag.len())
        .position(|part| part == flag)
    {
        package.settings.drain(offset..offset + flag.len());
    }
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
        assert_eq!(
            face.permissions(),
            Some(ttf_parser::Permissions::Installable)
        );
        assert_eq!(face.ascender(), 800);
        assert_eq!(face.descender(), -200);
        assert_eq!(
            bytes.chunks(4).fold(0u32, |sum, chunk| {
                let mut word = [0; 4];
                word[..chunk.len()].copy_from_slice(chunk);
                sum.wrapping_add(u32::from_be_bytes(word))
            }),
            0xb1b0afba
        );
        let names = face
            .names()
            .into_iter()
            .filter(|n| n.name_id == 1 || n.name_id == 6)
            .filter_map(|n| n.to_string())
            .collect::<Vec<_>>();
        assert_eq!(names, ["JadeLayoutSpace-0", "JadeLayoutSpace-0"]);
        assert_eq!(bytes, font("JadeLayoutSpace-0"));
    }

    /// 非対象の文書では、本文に内部名が書かれていてもsettingsを変更しない。
    #[test]
    fn ordinary_documents_keep_their_compatibility_settings() {
        let mut package = docx_rs::Docx::new()
            .add_paragraph(
                docx_rs::Paragraph::new()
                    .add_run(docx_rs::Run::new().add_text("JadeLayoutSpace-0")),
            )
            .build();
        let before = package.settings.clone();
        preserve_advance(&mut package, &[]);
        assert_eq!(package.settings, before);
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
