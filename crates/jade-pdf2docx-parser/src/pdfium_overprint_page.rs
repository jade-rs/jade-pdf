//! 一つの完全重畳groupだけを持つページで、失われた文字を元paint順へ戻す。
use crate::{
    ParseError, ParsedBounds, ParsedTextElement, decode::DecodeBudget, metadata_workspace,
    pdfium_font_tags::Tagged,
};
use pdfium_render::prelude::*;

/// 同じ検査済み入力とSDKを共有する、回復処理の借用context。
pub(crate) struct Context<'a> {
    /// 元の解析と共有する停止・容量予算。
    pub(crate) budget: &'a DecodeBudget,
    /// 元の文書を読み込んだSDK。
    pub(crate) sdk: &'a Pdfium,
    /// 元のfont programを保つSDK専用入力。
    pub(crate) bytes: &'a [u8],
    /// 実際の埋め込みを確認したsource tag。
    pub(crate) tags: &'a Tagged,
    /// 元の文書path。
    pub(crate) path: &'a std::path::Path,
    /// 回復するページ番号。
    pub(crate) page: usize,
}

/// 一文字から複数objectを復元した、ページ内のUnicodeとsource style。
pub(crate) struct Recovery {
    /// 元の前後の空白を保ったpreview。
    pub(crate) preview: String,
    /// 元paint順のglyphとsource weight。
    pub(crate) elements: Vec<ParsedTextElement>,
}

impl Context<'_> {
    /// ページ全体・描画順・幾何を確認できる一つのgroupにだけ限定して採用する。
    pub(crate) fn recover(
        &self,
        page: &PdfPage<'_>,
        characters: &PdfPageTextChars<'_>,
        excluded: &[bool],
        preview: &str,
        original: &[ParsedTextElement],
    ) -> Result<Option<Recovery>, ParseError> {
        metadata_workspace::ensure_active(self.budget)?;
        if !self.tags.has_weights() || original.len() != 1 {
            return Ok(None);
        }
        let groups =
            crate::pdfium_overprint_groups::collect(self.budget, page.objects(), self.tags)?;
        if groups.len() != 1 {
            return Ok(None);
        }
        let group = &groups[0];
        if group.targets.len() != group.page_text_objects {
            return Ok(None);
        }
        let mut scalar = preview.char_indices().filter(|(_, ch)| !ch.is_whitespace());
        let Some((start, ch)) = scalar.next() else {
            return Ok(None);
        };
        if scalar.next().is_some() || !original[0].text.trim().chars().eq(std::iter::once(ch)) {
            return Ok(None);
        }
        let mut first = None;
        for character in characters.iter() {
            metadata_workspace::ensure_active(self.budget)?;
            if excluded.get(character.index()).copied().unwrap_or(false)
                || matches!(
                    character.render_mode().ok(),
                    Some(PdfPageTextRenderMode::Invisible)
                )
            {
                continue;
            }
            let Some(value) = char::from_u32(character.unicode_value()) else {
                return Ok(None);
            };
            if value.is_whitespace() {
                continue;
            }
            if first.is_some() {
                return Ok(None);
            }
            let Some(key) = self.tags.source_key(&character.font_name()) else {
                return Ok(None);
            };
            if !group.targets.iter().any(|target| target.source_key == key) {
                return Ok(None);
            }
            first = Some((key, character));
        }
        let Some((key, character)) = first else {
            return Ok(None);
        };
        let Ok(rect) = character.loose_bounds() else {
            return Ok(None);
        };
        let bounds = ParsedBounds {
            x0: rect.left().value,
            y0: rect.bottom().value,
            x1: rect.right().value,
            y1: rect.top().value,
        };
        let baseline = crate::pdfium_text::horizontal_baseline(&character);
        let mut elements = metadata_workspace::vector(self.budget, group.targets.len())?;
        let mut matching_source = false;
        for target in &group.targets {
            let Some(mut element) = crate::pdfium_overprint_groups::probe::extract(
                self.budget,
                self.sdk,
                self.bytes,
                self.page,
                target,
                self.tags,
                self.path,
            )?
            else {
                return Ok(None);
            };
            if !same_bounds(element.bounds, bounds)
                || element.font_size.to_bits() != original[0].font_size.to_bits()
                || element.baseline_y.map(f32::to_bits) != baseline.map(f32::to_bits)
            {
                return Ok(None);
            }
            if target.source_key == key {
                if element.text != original[0].text.trim()
                    || element.font_name != original[0].font_name
                    || element.fill_color != original[0].fill_color
                    || element.font_weight != original[0].font_weight
                {
                    return Ok(None);
                }
                matching_source = true;
            }
            // 元の通常segmentと同じ幾何表現を、raw glyphとの完全一致を確認して引き継ぐ。
            element.bounds = original[0].bounds;
            element.baseline_y = original[0].baseline_y;
            elements.push(element);
        }
        if !matching_source {
            return Ok(None);
        }
        let mut recovered = String::new();
        crate::text_result_storage::extend(self.budget, &mut recovered, &preview[..start]);
        for element in &elements {
            crate::text_result_storage::extend(self.budget, &mut recovered, &element.text);
        }
        crate::text_result_storage::extend(
            self.budget,
            &mut recovered,
            &preview[start + ch.len_utf8()..],
        );
        self.budget.check()?;
        metadata_workspace::ensure_active(self.budget)?;
        Ok(Some(Recovery {
            preview: recovered,
            elements,
        }))
    }
}

/// 同じ描画位置だけを採用し、NaNや丸めによる近似一致を使わない。
fn same_bounds(a: ParsedBounds, b: ParsedBounds) -> bool {
    [a.x0, a.y0, a.x1, a.y1].map(f32::to_bits) == [b.x0, b.y0, b.x1, b.y1].map(f32::to_bits)
}
