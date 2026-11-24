//! テキストと画像に共通するページ末尾のsection境界。
use docx_rs::{LineSpacing, LineSpacingType, Paragraph, SectionProperty};

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

/// 表にはpPrを置けないため、表の後の最小行高の段落でsource sectionを確定する。
/// 通常の空段落のfont高や段落間隔で、不要な改ページを増やさない。
pub(super) fn after_table(
    section: Option<&SectionProperty>,
    is_last_block: bool,
) -> Option<Paragraph> {
    if !is_last_block {
        return None;
    }
    section.map(|section| empty_page(Some(section)))
}

/// 見出しだけのページにも最小のflowを置き、最終ページを含む境界とpart参照を残す。
pub(super) fn empty_page(section: Option<&SectionProperty>) -> Paragraph {
    let paragraph = Paragraph::new().size(1).line_spacing(
        LineSpacing::new()
            .before(0)
            .after(0)
            .line(1)
            .line_rule(LineSpacingType::Exact),
    );
    if let Some(section) = section {
        paragraph.section_property(section.clone())
    } else {
        paragraph
    }
}
