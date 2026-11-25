//! 全候補のSDK埋め込みを確認し、一つでも未確認なら元入力へ戻す。
use super::{Kind, Tagged};
use crate::{ParseError, decode::DecodeBudget, metadata_workspace};
use pdfium_render::prelude::*;

impl Tagged {
    /// 最終入力の前に全候補を検査し、使用されない候補も採用を拒否する。
    pub(crate) fn confirm_embedded(
        &mut self,
        budget: &DecodeBudget,
        sdk: &Pdfium,
        max_pages: usize,
    ) -> Result<bool, ParseError> {
        metadata_workspace::ensure_active(budget)?;
        if !self.has_weights() {
            return Ok(true);
        }
        let verified = {
            let Ok(document) = sdk.load_pdf_from_byte_slice(&self.bytes, None) else {
                return Ok(false);
            };
            crate::limits::check_limit(
                "page count",
                document.pages().len() as u64,
                max_pages as u64,
            )?;
            let mut seen = metadata_workspace::vector(budget, self.tags.len())?;
            seen.resize(self.tags.len(), false);
            let mut valid = true;
            for page in document.pages().iter() {
                if !self.visit_fonts(budget, page.objects(), 0, &mut seen)? {
                    valid = false;
                    break;
                }
            }
            valid
                && self
                    .tags
                    .iter()
                    .enumerate()
                    .all(|(i, tag)| !matches!(tag.kind, Kind::Weight(_)) || seen[i])
        };
        metadata_workspace::ensure_active(budget)?;
        self.weights_confirmed = verified;
        Ok(verified)
    }

    /// Formを共有するobjectも深さ予算内で調べ、名前だけでは埋め込みを確定しない。
    fn visit_fonts<'a>(
        &self,
        budget: &DecodeBudget,
        objects: &'a impl PdfPageObjectsCommon<'a>,
        depth: usize,
        seen: &mut [bool],
    ) -> Result<bool, ParseError> {
        let _ = budget.check_form_depth(depth);
        budget.check()?;
        metadata_workspace::ensure_active(budget)?;
        for object in objects.iter() {
            metadata_workspace::ensure_active(budget)?;
            if let Some(text) = object.as_text_object() {
                let font = text.font();
                let name = font.name();
                if let Some(index) = self.tag_index(&name)
                    && matches!(self.tags[index].kind, Kind::Weight(_))
                {
                    if !font.is_embedded().is_ok_and(|value| value) {
                        return Ok(false);
                    }
                    seen[index] = true;
                }
            } else if let Some(form) = object.as_x_object_form_object()
                && !self.visit_fonts(budget, form, depth + 1, seen)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
