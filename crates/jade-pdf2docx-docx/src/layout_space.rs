//! 論理空白専用の所有fontを用い、source fontの未確認codeへ空白を割り当てない。
use jade_pdf2docx_parser::{ParsedEmbeddedFont, ParsedTextElement};
mod selection;
pub(super) use selection::read as selected_family;
mod writer;

/// source familyと衝突しない、文書内だけで使う空白font名を選ぶ。
pub(super) fn family(fonts: &[ParsedEmbeddedFont], texts: &[ParsedTextElement]) -> String {
    let used = fonts
        .iter()
        .map(|f| f.font_name.as_str())
        .chain(texts.iter().filter_map(|t| t.font_name.as_deref()))
        .filter_map(|name| {
            let normalized = crate::docx_font_name_from_pdf_font_name(name);
            let name = normalized.as_str();
            let prefix = name.get(..16)?;
            (name.len() < 63 && prefix.eq_ignore_ascii_case("JadeLayoutSpace-")).then_some(())?;
            name.get(16..)?.parse::<usize>().ok()
        })
        .collect::<std::collections::BTreeSet<_>>();
    let index = (0..=used.len())
        .find(|i| !used.contains(i))
        .expect("n occupied indices cannot cover n+1 candidates");
    format!("JadeLayoutSpace-{index}")
}

/// U+0020だけを持つ固定幅の所有TrueType fontを生成する。
pub(super) fn font(family: &str) -> Vec<u8> {
    writer::encode(family)
}

/// 内部spaceを参照する文書だけ、確認済みadvanceを半emへ補正する互換flagを外す。
pub(super) fn preserve_advance(package: &mut docx_rs::XMLDocx) -> Result<(), crate::ConvertError> {
    let mut selected = false;
    for part in std::iter::once(&package.document)
        .chain(package.headers.iter())
        .chain(package.footers.iter())
        .chain(std::iter::once(&package.footnotes))
    {
        if selected_family(part)?.is_some() {
            selected = true;
            break;
        }
    }
    if !selected {
        return Ok(());
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
    Ok(())
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
        preserve_advance(&mut package).unwrap();
        assert_eq!(package.settings, before);
    }

    proptest! {
            /// 入力が内部名を宣言しても、有限個のsource familyと重ならない名前を得る。
            #[test]
            fn family_is_collision_free(count in 0usize..40,unembedded in 0usize..40,upper in any::<bool>()) {
                let prefix=if upper {"JADELAYOUTSPACE-"} else {"JadeLayoutSpace-"};
                let fonts = (0..count).map(|i| ParsedEmbeddedFont { font_name: format!("{prefix}{i}"), bytes: Vec::new() }).collect::<Vec<_>>();
                let texts=(count..count+unembedded).map(|i|ParsedTextElement {
    font_weight: None,
                    font_name:Some(format!("ABCDEF+ {prefix}{i} ")),baseline_y:None,page:0,font_size:10.,fill_color:None,
                    bounds:jade_pdf2docx_parser::ParsedBounds {x0:0.,x1:10.,y0:0.,y1:10.},text:"source".into(),
                }).collect::<Vec<_>>();
                let name = family(&fonts,&texts);
                prop_assert_eq!(&name,&format!("JadeLayoutSpace-{}",count+unembedded));
                prop_assert!(!fonts.iter().any(|f| f.font_name.eq_ignore_ascii_case(&name)));
                prop_assert!(!texts.iter().any(|t| t.font_name.as_ref().unwrap().eq_ignore_ascii_case(&name)));
                prop_assert!(name.len() < 63);
            }
        }
}
