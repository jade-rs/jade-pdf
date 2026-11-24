//! テキストと画像に共通するページ末尾のsection境界。
use docx_rs::{Paragraph, SectionProperty};

/// 最後のblockを含む段落に境界を付け、後続画像を次ページへ取り残さない。
pub(super) fn paragraph(
    paragraph: Paragraph,
    section: Option<&SectionProperty>,
    is_last_block: bool,
) -> Paragraph {
    if is_last_block && let Some(section) = section {
        paragraph.section_property(section.clone())
    } else {
        paragraph
    }
}
