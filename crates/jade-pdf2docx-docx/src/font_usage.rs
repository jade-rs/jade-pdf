//! 本文・header・footer・脚注のfont使用を一partずつ調べ、本文だけにない埋め込みを落とさない。
use crate::ConvertError;
use jade_pdf2docx_parser::ParsedEmbeddedFont;
use std::io::{Read, Seek};

/// source fontの使用flagと、生成runが選んだ内部space名。
pub(super) struct Usage {
    /// 入力と同じ順序のsource font使用有無。
    pub(super) source_fonts: Vec<bool>,
    /// bodyやpage chromeが選ぶ一意の内部space family。
    pub(super) space_family: Option<String>,
}

/// 大きい本文を複製せず、ZIPの表示用XMLを逐次読んで必要fontを記録する。
pub(super) fn read<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    fonts: &[ParsedEmbeddedFont],
) -> Result<Usage, ConvertError> {
    let mut usage = Usage {
        source_fonts: vec![false; fonts.len()],
        space_family: None,
    };
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| ConvertError::Pack(e.to_string()))?;
        let name = entry.name();
        if name != "word/document.xml"
            && name != "word/footnotes.xml"
            && !(name.ends_with(".xml")
                && (name.starts_with("word/header") || name.starts_with("word/footer")))
        {
            continue;
        }
        let mut xml = String::new();
        entry
            .read_to_string(&mut xml)
            .map_err(|e| ConvertError::Pack(e.to_string()))?;
        for (used, font) in usage.source_fonts.iter_mut().zip(fonts) {
            *used |= xml.contains(font.font_name.as_str());
        }
        if let Some(family) = crate::layout_space::selected_family(xml.as_bytes())? {
            if usage
                .space_family
                .as_ref()
                .is_some_and(|old| old != &family)
            {
                return Err(ConvertError::Pack(
                    "inconsistent owned layout-space families".into(),
                ));
            }
            usage.space_family = Some(family);
        }
    }
    Ok(usage)
}
